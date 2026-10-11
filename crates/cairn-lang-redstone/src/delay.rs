//! Routed Placement IR → delayed Placement IR lowering (delay insertion).
//!
//! Stage 3 of the five in `spec/redstone` "Place-and-route": rebuilds every net's routed tree
//! (routing stores only `wire_length`, since the trees are cheap to rebuild and would bloat the
//! JSON) and sets every cell's [`crate::placement_ir::PlacedCellNode::local_delay_ticks`] to its
//! base delay plus its implicit buffer ticks:
//!
//! - **Base delay** is the cell's
//!   [`crate::edition_netlist_ir::EditionCell::base_delay_ticks`]; an
//!   actuator pad has none.
//! - **Implicit buffer repeaters** are counted per driving net, not per
//!   driver: two ports reading one signal are fed by one strand of dust
//!   and by the repeaters standing on it. Dust loses one unit of signal
//!   per block from strength 15, so `repeater_sites` stands a
//!   repeater, worth [`BUFFER_REPEATER_TICKS`], after at most
//!   `DUST_ATTENUATION_LIMIT` blocks of dust since the last block that
//!   restored strength — on a straight run from a fresh source in
//!   place of every 16th block, `floor((s - 1) / 16)` of them over a
//!   segment of `s` steps, and earlier where the coord there turns,
//!   climbs or forks, which a repeater cannot carry a signal through.
//!
//!   That last block is not always the start of the net. A cell that
//!   passes on the strength it reads (a Bedrock OR, a Java comparator
//!   AND) is not a strand of dust — its body stands between the wire
//!   into it and the wire out — but it restores nothing, so for the
//!   limit the two wires count as one run, and a repeater that run
//!   needs can land on either side of the cell.
//!
//! A segment is the *routed* path from the net's source to the sink (`route_to`), not the
//! Manhattan distance: the two differ whenever the wire goes round something, and counting
//! against the route lets stage 4 put every buffer this stage paid for onto the dust it
//! refreshes. Each pass builds its own `RepeaterSites` by running the same deterministic function
//! over its own rebuilt trees and the same `ir.cells`, so the ticks one charges and the blocks the
//! other lays are the same set as long as nothing between them moves a cell. Nothing asserts that
//! at runtime; the proptest in `crossing`'s tests checks it.
//!
//! **`local_delay_ticks` is a local wire cost, not an arrival time.** It sums the buffers on every
//! net feeding the cell, and stage 4 is held to it: `local_delay_ticks - base_delay_ticks` equals
//! [`BUFFER_REPEATER_TICKS`] per block in the cell's *deduplicated* `buffer_coords`. An arrival
//! time would take the max over those nets and add each upstream driver's arrival, so the two
//! differ whenever more than one incoming net carries a buffer or a driver is a cell. Nothing in
//! this crate computes an arrival time; `assert latency(...)` (`spec/redstone` "Verification") is
//! a path latency, not this.
//!
//! `E_ATTENUATION_LIMIT` fires when a single segment's routed length exceeds the sanity cap
//! [`MAX_ATTENUATION_SEGMENT`], and when a run of dust would pass the attenuation limit (counted
//! from the last block that restored strength, or within a cell's smaller budget) with no coord
//! on it that can hold a repeater close enough. A refused scope is left out, so a partial
//! `local_delay_ticks` never reaches a later reader.
//!
//! The JSON is the routing stage's plus `local_delay_ticks` after `wire_length`, with `stage` set
//! to `delay`.

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::netlist_ir::NetRef;
use crate::pass::{OpenScope, attribute_nodes, lower_scopes, open_scope};
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::repeater_sites::{Fed, repeater_sites_of_scope};
use crate::routing_geometry::sum_over_driving_nets;
use crate::saturating_index;

pub use crate::grid::MAX_ATTENUATION_SEGMENT;
pub use crate::repeater_sites::DUST_ATTENUATION_LIMIT;

/// Tick delay of one implicit buffer repeater: a repeater at its default `delay=1`.
pub const BUFFER_REPEATER_TICKS: u32 = 1;

/// At zero, delay insertion would charge nothing for the repeaters crossing legalization still
/// lays.
const _: () = assert!(
    BUFFER_REPEATER_TICKS >= 1,
    "BUFFER_REPEATER_TICKS must be at least 1 — a default repeater delays by one tick",
);

/// The cap must exceed the longest segment a fresh source carries without a buffer,
/// `DUST_ATTENUATION_LIMIT + 1` steps, or every segment that [`repeater_sites`] would buffer is
/// refused instead.
///
/// [`repeater_sites`]: crate::repeater_sites
const _: () = assert!(
    MAX_ATTENUATION_SEGMENT > DUST_ATTENUATION_LIMIT + 1,
    "MAX_ATTENUATION_SEGMENT must exceed DUST_ATTENUATION_LIMIT + 1 steps so implicit buffers have a band to cover on a segment from a fresh source",
);

/// Output of a [`compile_delay`] run: the delayed scopes, every cell having taken
/// [`crate::placement_ir::PlacementPhase::delay`], and the findings.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct DelayOutput {
    /// Placement IR for every scope whose delay insertion succeeded,
    /// with every cell's `local_delay_ticks` field populated.
    pub scoped: ScopedPlacementIr,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

impl DelayOutput {
    /// An empty output.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lowers a routed [`ScopedPlacementIr`] into a delayed one.
///
/// A scope refused with an error is left out: `E_ATTENUATION_LIMIT` from this pass, or the
/// `E_ROUTE_CONGESTION` and `E_NO_CIRCUIT_REGION` refusals every place-and-route pass shares.
#[must_use]
pub fn compile_delay(routed: &ScopedPlacementIr) -> DelayOutput {
    let (scoped, diagnostics) = lower_scopes(routed, |entry| {
        delay_scope(entry).map(|ir| (ir, Vec::new()))
    });
    DelayOutput {
        scoped,
        diagnostics,
    }
}

/// Result of delaying one scope: the delayed IR on success, a single
/// Error-severity diagnostic on failure.
type ScopeDelay = Result<PlacementIr, Diagnostic>;

fn delay_scope(entry: &ScopedPlacementIrEntry) -> ScopeDelay {
    let Some(OpenScope {
        mut ir,
        region,
        nets,
        ..
    }) = open_scope(entry, "routed", "delay insertion")?
    else {
        return Ok(entry.ir.clone());
    };

    // Refuse before writing `local_delay_ticks`, so a failed scope leaves
    // no partial attribution behind.
    for (cell_index, cell) in ir.cells.iter().enumerate() {
        for (driver_index, driver) in cell.drivers.iter().enumerate() {
            let segment = nets.segment(driver.net, cell.coord);
            if segment > MAX_ATTENUATION_SEGMENT {
                return Err(attenuation_diagnostic(
                    entry,
                    &region,
                    cell_index,
                    driver_index,
                    segment,
                ));
            }
        }
    }
    // An actuator wired straight to a sensor across a wide region hits
    // the same cap without touching a cell.
    for (output_index, output) in ir.outputs.iter().enumerate() {
        let segment = nets.segment(output.driver, output.pad);
        if segment > MAX_ATTENUATION_SEGMENT {
            return Err(attenuation_output_diagnostic(
                entry,
                &region,
                output_index,
                segment,
            ));
        }
    }

    let sites = repeater_sites_of_scope(&ir, entry, "routed", &region, &nets.trees)?;
    attribute_local_delay_ticks(&mut ir, entry, &|net, sink, fed| {
        let buffers = sites
            .along(net, sink)
            .unwrap_or_else(|| fed.is_not_a_terminal(net, sink));
        saturating_index(buffers.len())
    });

    Ok(ir)
}

/// Sets every cell's `local_delay_ticks` to its base delay plus the buffer ticks on each driving
/// net ([`sum_over_driving_nets`]), and every actuator pad's to the buffer ticks on its own
/// segment.
///
/// `buffers_on(net, sink, fed)` counts the repeaters `net`'s signal passes through on its way to
/// `sink`, the coord of `fed`. Summed over nets rather than maxed, because the figure is a local
/// wire cost, not an arrival time (see the module doc).
fn attribute_local_delay_ticks<F>(
    ir: &mut PlacementIr,
    entry: &ScopedPlacementIrEntry,
    buffers_on: &F,
) where
    F: Fn(NetRef, CellCoord, Fed) -> u32,
{
    attribute_nodes(
        ir,
        entry,
        |index, cell| {
            let buffer_ticks = sum_over_driving_nets(&cell.drivers, |net| {
                buffers_on(net, cell.coord, Fed::Cell(index)).saturating_mul(BUFFER_REPEATER_TICKS)
            });
            cell.cell.base_delay_ticks().saturating_add(buffer_ticks)
        },
        |index, output| {
            buffers_on(output.driver, output.pad, Fed::Output(index))
                .saturating_mul(BUFFER_REPEATER_TICKS)
        },
        |phase, ticks, identity| phase.delay_at(ticks, identity),
    );
}

/// The fewest buffer repeaters on a segment of `steps` steps from a
/// fresh source to its sink.
///
/// The segment holds `steps - 1` blocks of dust: its two ends are the
/// source and the sink. A source at strength 15 loses one unit per
/// block, so it carries [`DUST_ATTENUATION_LIMIT`] blocks of dust — the
/// last at strength 1, which still drives the sink — and a segment of
/// up to `DUST_ATTENUATION_LIMIT + 1` steps needs no buffer. A repeater
/// stands in place of the block of dust past the limit, reads the one
/// before it, and restores strength, so each one carries the signal
/// `DUST_ATTENUATION_LIMIT + 1` steps further: a 17-step segment needs
/// one, a 33-step one two. Saturating so a pathological segment from a
/// hand-built IR cannot overflow.
///
/// Exact on a straight, unbranched run, and a floor on any other:
/// [`repeater_sites`] is what places them, and a coord a repeater
/// cannot stand on moves the repeater earlier. So this only sizes the
/// chain a refused segment would have needed.
///
/// [`repeater_sites`]: crate::repeater_sites
pub(crate) fn buffer_count_for_segment(steps: u32) -> u32 {
    steps.saturating_sub(1) / (DUST_ATTENUATION_LIMIT + 1)
}

fn attenuation_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    cell_index: usize,
    driver_index: usize,
    segment: u32,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` has a driver segment of {segment} steps into cell #{cell_index} port #{driver_index} — exceeds the v1 attenuation limit of {cap} steps (dust decays 1/block, so this segment would need at least {buffers} buffer repeaters to materialize)",
        kind = entry.kind.label(),
        name = entry.name,
        cap = MAX_ATTENUATION_SEGMENT,
        buffers = buffer_count_for_segment(segment),
    );
    error_with_footer(
        DiagnosticCode::AttenuationLimit,
        reservation.span.clone(),
        primary,
        "Fix: enlarge `region=` so no driver→cell segment exceeds the cap, pin cell placement closer to its drivers, or split the logic across several scopes, each with its own `circuit` line",
    )
}

fn attenuation_output_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    output_index: usize,
    segment: u32,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` has a driver segment of {segment} steps into output pad #{output_index} — exceeds the v1 attenuation limit of {cap} steps (dust decays 1/block, so this segment would need at least {buffers} buffer repeaters to materialize)",
        kind = entry.kind.label(),
        name = entry.name,
        cap = MAX_ATTENUATION_SEGMENT,
        buffers = buffer_count_for_segment(segment),
    );
    error_with_footer(
        DiagnosticCode::AttenuationLimit,
        reservation.span.clone(),
        primary,
        "Fix: enlarge `region=` so no driver→sink segment exceeds the cap, pin actuator placement closer to its drivers, or split the logic across several scopes, each with its own `circuit` line",
    )
}

#[cfg(test)]
mod tests;
