//! Delayed Placement IR → legalized Placement IR lowering (crossing
//! legalization).
//!
//! Stage 4 of the five in `spec/redstone` "Place-and-route": rebuilds every net's Steiner tree
//! through the same call routing and delay insertion make, and fills every cell's and actuator
//! pad's [`crate::placement_ir::PlacedCellNode::buffer_coords`] with the coord of each implicit
//! buffer repeater the delay pass counted.
//!
//! The wire itself needs no legalizing here: stage 2 lays each net round
//! the dust of the nets before it and the coords beside that dust, so a
//! scope that reaches this pass has no two signals on one strand. What
//! is left is the crossing a *repeater* would make by standing on a
//! coord it does not own. The delay pass chose those coords off the
//! same routed tree, and this pass reads them back.
//!
//! Each segment records the repeaters `crate::repeater_sites::repeater_sites` stands on its
//! routed path (`route_to`), from a set rebuilt the way the delay pass built the one it charged
//! (see [`crate::delay`]).
//!
//! A repeater is never contested, and never stands on a cell body or a pad. A site needs a parent
//! and exactly one child, so it is never the source and never a leaf; every terminal is a leaf,
//! because `block_sites` makes cells and pads obstacles and `Router::tree` keeps each path's far
//! end out of the coords a later path may leave from. So a site is dust of its own net, and stage
//! 2 gave that net its dust alone. Two segments of one net do pass the same repeater where their
//! routes share a prefix; both record it, because `buffer_coords` is an attribution list rather
//! than a block list.
//!
//! A refused scope is left out, so a later consumer never reads a partial `buffer_coords`. A scope
//! with nothing to legalize dumps as its input did apart from the `stage` tag.

use crate::diagnostic::Diagnostic;
use crate::pass::{OpenScope, lower_scopes, open_scope};
use crate::placement_ir::{
    BufferCoord, BufferSegment, CellIdentity, PlacementIr, ScopedPlacementIr,
    ScopedPlacementIrEntry,
};
use crate::repeater_sites::{Fed, RepeaterSites, repeater_sites_of_scope};

/// Output of a [`compile_crossing`] run: the legalized scopes, every cell having taken
/// [`PlacementPhase::legalize`], and the findings.
///
/// [`PlacementPhase::legalize`]: crate::placement_ir::PlacementPhase::legalize
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct CrossingOutput {
    /// Placement IR for every scope whose crossing legalization
    /// succeeded, with every cell's `buffer_coords` populated to match
    /// the buffer tick contribution the delay pass folded into
    /// `local_delay_ticks`.
    pub scoped: ScopedPlacementIr,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

impl CrossingOutput {
    /// An empty output.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lowers a delayed [`ScopedPlacementIr`] into a legalized one.
///
/// A scope refused with an error is left out: the [`DiagnosticCode::NoCircuitRegion`],
/// [`DiagnosticCode::RouteCongestion`] and [`DiagnosticCode::AttenuationLimit`] refusals every
/// place-and-route pass shares, or [`DiagnosticCode::AttenuationLimit`] for a stretch of dust no
/// repeater can stand on. The pass raises no warning.
///
/// [`DiagnosticCode::NoCircuitRegion`]: crate::DiagnosticCode::NoCircuitRegion
/// [`DiagnosticCode::RouteCongestion`]: crate::DiagnosticCode::RouteCongestion
/// [`DiagnosticCode::AttenuationLimit`]: crate::DiagnosticCode::AttenuationLimit
#[must_use]
pub fn compile_crossing(delayed: &ScopedPlacementIr) -> CrossingOutput {
    let (scoped, diagnostics) = lower_scopes(delayed, |entry| {
        legalize_scope(entry).map(|ir| (ir, Vec::new()))
    });
    debug_assert!(
        diagnostics
            .iter()
            .all(|d| d.severity() == d.code.severity()),
        "a diagnostic renders with its code's severity: every producer in \
         this pass has to agree with `DiagnosticCode::severity`, including \
         one written after the builders below",
    );
    CrossingOutput {
        scoped,
        diagnostics,
    }
}

/// Result of legalizing one scope: the legalized IR, or the error that refuses it. Every finding
/// this pass makes refuses the scope, so there is no warning arm.
type ScopeLegalization = Result<PlacementIr, Diagnostic>;

fn legalize_scope(entry: &ScopedPlacementIrEntry) -> ScopeLegalization {
    let Some(OpenScope {
        mut ir,
        region,
        nets,
        ..
    }) = open_scope(entry, "delayed", "crossing legalization")?
    else {
        return Ok(entry.ir.clone());
    };

    // A repeater stands only on its own net's route, and `net_trees` asserts that no net's dust
    // stands on or beside another's, so there is no coord to contest.
    let sites = repeater_sites_of_scope(&ir, entry, "delayed", &region, &nets.trees)?;
    let allocation = allocate_buffer_coords(&ir, &sites);

    for (index, (cell, buffers)) in ir.cells.iter_mut().zip(allocation.per_cell).enumerate() {
        // Loud in release too: `legalize_at` panics on any non-`Delayed`
        // variant, naming the cell that tripped it.
        let identity = CellIdentity::new(index, cell.coord, entry);
        cell.phase.legalize_at(buffers, identity);
    }

    for (index, (output, buffers)) in ir.outputs.iter_mut().zip(allocation.per_output).enumerate() {
        let identity = CellIdentity::output(index, output.pad, entry);
        output.phase.legalize_at(buffers, identity);
    }

    Ok(ir)
}

/// The buffer coords of one scope: for every cell driver segment and then every actuator segment,
/// the repeaters of `sites` its routed path runs through.
fn allocate_buffer_coords(ir: &PlacementIr, sites: &RepeaterSites<'_>) -> BufferAllocation {
    let mut per_cell: Vec<Vec<BufferCoord>> = Vec::with_capacity(ir.cells.len());
    for (cell_index, cell) in ir.cells.iter().enumerate() {
        let mut buffers_for_cell: Vec<BufferCoord> = Vec::new();
        for driver in &cell.drivers {
            // `along` is `None` for a sink that is not a terminal of
            // the net, which `collect_nets` makes unreachable — it
            // built the tree's terminal list out of this very driver
            // list.
            buffers_for_cell.extend(
                sites
                    .along(driver.net, cell.coord)
                    .unwrap_or_else(|| {
                        Fed::Cell(cell_index).is_not_a_terminal(driver.net, cell.coord)
                    })
                    .into_iter()
                    .map(|coord| BufferCoord::new(BufferSegment::Port(driver.port), coord)),
            );
        }
        // Every entry names a driver of the owning cell, which the Placement IR reader also
        // holds a dump to. True by construction here; asserted for a producer added elsewhere.
        debug_assert!(
            buffers_for_cell.iter().all(|b| matches!(
                b.port,
                BufferSegment::Port(port) if cell.drivers.iter().any(|d| d.port == port)
            )),
            "BufferCoord::port must reference a driver on cells[{cell_index}]",
        );
        per_cell.push(buffers_for_cell);
    }

    // Stage 3 charges the segment out to an actuator like a segment into a cell, so its buffers
    // need coords too. Where it shares a prefix with a cell's route, both record the same
    // repeater, which refreshes both.
    let mut per_output: Vec<Vec<BufferCoord>> = Vec::with_capacity(ir.outputs.len());
    for (output_index, output) in ir.outputs.iter().enumerate() {
        let buffers_for_output: Vec<BufferCoord> = sites
            .along(output.driver, output.pad)
            .unwrap_or_else(|| {
                Fed::Output(output_index).is_not_a_terminal(output.driver, output.pad)
            })
            .into_iter()
            .map(|coord| BufferCoord::new(BufferSegment::Out, coord))
            .collect();
        // The mirror of the cell-side contract: a buffer on the wire to
        // an actuator belongs to no input port, and saying otherwise
        // would group it under a driver of a cell it is not on.
        debug_assert!(
            buffers_for_output
                .iter()
                .all(|b| matches!(b.port, BufferSegment::Out)),
            "a buffer on the wire to outputs[{output_index}] must name the outward segment",
        );
        per_output.push(buffers_for_output);
    }

    BufferAllocation {
        per_cell,
        per_output,
    }
}

/// Where every buffer repeater of one scope goes, per cell and per actuator pad: two vectors,
/// because they commit into different nodes.
struct BufferAllocation {
    per_cell: Vec<Vec<BufferCoord>>,
    per_output: Vec<Vec<BufferCoord>>,
}

#[cfg(test)]
mod tests;
