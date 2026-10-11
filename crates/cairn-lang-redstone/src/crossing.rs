//! Delayed Placement IR → legalized Placement IR lowering (crossing
//! legalization).
//!
//! Stage 4 of the five-stage pipeline `spec/redstone` "Place-and-route"
//! lays out. Rebuilds every net's Steiner tree through the same call the
//! routing and delay passes make, and fills every cell's and actuator
//! pad's [`crate::placement_ir::PlacedCellNode::buffer_coords`] with the
//! coord of each implicit buffer repeater the delay pass counted.
//!
//! The wire itself needs no legalizing here: stage 2 lays each net round
//! the dust of the nets before it and the coords beside that dust, so a
//! scope that reaches this pass has no two signals on one strand. What
//! is left is the crossing a *repeater* would make by standing on a
//! coord it does not own. The delay pass chose those coords off the
//! same routed tree, and this pass reads them back.
//!
//! A repeater refreshes the dust it stands on, so each one stands on its
//! net's routed tree, where `crate::delay::repeater_sites` puts it. This
//! pass builds its own set from the trees it rebuilt; it is the set the
//! delay pass charged ticks from because that function is deterministic
//! and nothing between the two stages moves a cell, not because one is
//! handed to the other. Each segment records the repeaters on
//! `route_to`, the routed path from the net's source to *this* sink.
//!
//! A repeater is never contested, and never stands on a cell body or a
//! pad. A site needs a parent and exactly one child, so it is never the
//! source and never a leaf; every terminal is a leaf, because
//! `block_sites` makes cells and pads obstacles and `Router::tree` keeps
//! each path's far end out of the coords a later path may leave from.
//! So a site is dust of its own net, and stage 2 gave that net its dust
//! alone. Two segments of one net do pass the
//! same repeater (their routes share a prefix); both record it, because
//! `buffer_coords` is an attribution list rather than a block list.
//!
//! Neither [`crate::placement_ir::RouteLayer::Bridge`] nor
//! [`crate::placement_ir::RouteLayer::Via`] has a producer here: bridge
//! coords reach the legalized IR from the routing pass, and `Via` has no
//! producer anywhere.
//!
//! Failed scopes are elided from the output so a stage-5 consumer never
//! reads a partially populated `buffer_coords`. The pass is one
//! [`crate::placement_ir::PlacementPhase::legalize`] transition per
//! cell; no new IR type. Both the layer and the coord vector serde-skip
//! on their defaults, so a scope with nothing to legalize dumps as its
//! delay-pass input did apart from the `stage` tag — which is why that
//! tag exists (see [`crate::placement_ir::PlacementStage`]).

use crate::delay::{Fed, RepeaterSites, repeater_sites_of_scope};
use crate::diagnostic::Diagnostic;
use crate::pass::{
    OpenScope, Skipped, lay_nets, lower_scopes, missing_region_diagnostic, open_scope,
    source_of_net,
};
use crate::placement_ir::{
    BufferCoord, BufferSegment, CellIdentity, PlacementIr, ScopedPlacementIr,
    ScopedPlacementIrEntry,
};
/// Output of a [`compile_crossing`] run.
///
/// Mirrors [`crate::delay::DelayOutput`]'s shape so callers see a
/// uniform result type across every stage of the place-and-route
/// pipeline. The legalized IR is a [`ScopedPlacementIr`] with every
/// non-failed scope's `buffer_coords` populated with one entry per
/// implicit buffer repeater the delay pass counted, each carrying the
/// [`RouteLayer`] of the route coord it stands on. No new IR type; the
/// crossing pass is one [`PlacementPhase::legalize`] transition per
/// cell, per the producer↔variant table on that enum.
///
/// [`RouteLayer`]: crate::placement_ir::RouteLayer
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
    /// Empty output (no legalized scopes, no diagnostics).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lower a delayed [`ScopedPlacementIr`] into a legalized
/// [`ScopedPlacementIr`].
///
/// Reads every cell's coord and the scope's
/// [`CircuitRegionReservation`] out of the input IR — the Placement
/// IR is self-describing by construction, so the crossing pass has no
/// `IntentModule` dependency.
///
/// One entry per non-empty [`PlacementIr`] whose legalization
/// succeeded; a scope that raises [`DiagnosticCode::NoCircuitRegion`],
/// either [`DiagnosticCode::RouteCongestion`] that
/// `crate::pass::lay_nets` raises — a pad row the reservation cannot
/// hold, or a sink no route reaches — or the
/// [`DiagnosticCode::AttenuationLimit`] it raises for a sink further
/// from its driver than the v1 cap in a straight line, or the one
/// raised for a stretch of dust no repeater can stand on, is
/// elided from the output so a partial `buffer_coords` set cannot
/// pollute the downstream block-array voxel lowering. Every finding
/// this pass makes refuses its scope, so there is no warning that
/// outlives one.
///
/// [`CircuitRegionReservation`]: crate::placement_ir::CircuitRegionReservation
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

/// Result of legalizing one scope: the legalized IR on success, the
/// single Error-severity diagnostic that elides the scope on failure.
///
/// The pass raises nothing else. Every finding it can make refuses the
/// scope, so there is no warning arm to carry.
type ScopeLegalization = Result<PlacementIr, Diagnostic>;

fn legalize_scope(entry: &ScopedPlacementIrEntry) -> ScopeLegalization {
    let source = &entry.ir;
    let OpenScope {
        mut ir,
        region,
        cell_coords,
        inputs,
        column,
        blocks,
    } = match open_scope(entry) {
        Err(Skipped::Empty) => return Ok(source.clone()),
        // Same policy as the delay pass: this pass writes `buffer_coords`,
        // which the phase table promises after stage 4.
        Err(Skipped::MissingRegion) => {
            return Err(missing_region_diagnostic(
                entry,
                "delayed",
                "crossing legalization",
            ));
        }
        Ok(scope) => scope,
    };

    let nets = lay_nets(
        &ir,
        &blocks,
        entry,
        "delayed",
        &region,
        source_of_net(&region, column, &cell_coords, inputs),
    )?;

    // Every coord a repeater can be asked to stand on is a coord of its
    // own net's route, and `net_trees` asserts as it builds that no
    // net's dust stands on, or one step from, another's. That is what
    // leaves this pass with a coord to record and no coord to contest.
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

/// Buffer coord allocation for one scope: every cell driver segment,
/// then every actuator segment.
///
/// Each segment records the repeaters of `sites` its routed path runs
/// through. `sites` is this pass's own, built as the delay pass built
/// the one it charged ticks from, so stage 3 and stage 4 describe one
/// circuit. Split out of `legalize_scope` so
/// the entry function stays under clippy's `too_many_lines` budget and
/// the allocation strategy reads as a self-contained table.
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
        // Producer-side contract on [`BufferCoord::port`]: every entry
        // must name a driver that actually exists on the owning cell.
        // Trivially true today because the push site sources
        // `driver.port` from the enclosing `for driver in &cell.drivers`
        // loop, but debug-asserted so a future buffer producer (e.g.
        // fan-out duplication) added elsewhere cannot silently emit a
        // `BufferCoord` whose `port` does not match any driver — the
        // downstream voxel lowering would then group buffers under a
        // driver that does not exist.
        debug_assert!(
            buffers_for_cell.iter().all(|b| matches!(
                b.port,
                BufferSegment::Port(port) if cell.drivers.iter().any(|d| d.port == port)
            )),
            "BufferCoord::port must reference a driver on cells[{cell_index}]",
        );
        per_cell.push(buffers_for_cell);
    }

    // The segment out to an actuator is charged for buffers by stage 3
    // exactly as a segment into a cell is, so stage 4 has to give those
    // buffers coords or the two stages disagree about how many exist.
    //
    // An actuator's route leaves its driver along the same trunk a
    // cell's does, so the two pass the same repeaters wherever they
    // share a prefix — and record the same coord, because one repeater
    // standing there refreshes both.
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

/// Where every implicit buffer repeater in one scope goes: one entry
/// per cell, then one per actuator pad. Two vectors rather than one
/// because the two commit into different nodes, and a single flat list
/// would need the split re-derived at the commit site.
struct BufferAllocation {
    per_cell: Vec<Vec<BufferCoord>>,
    per_output: Vec<Vec<BufferCoord>>,
}

#[cfg(test)]
mod tests;
