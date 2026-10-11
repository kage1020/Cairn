//! Stage 3 of `spec/redstone` "Place-and-route": sets each cell's `local_delay_ticks` to its base
//! delay plus the buffer repeaters on the routed nets feeding it. That is a local wire cost summed
//! over the nets, not an arrival time.

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

const _: () = assert!(
    BUFFER_REPEATER_TICKS >= 1,
    "BUFFER_REPEATER_TICKS must be at least 1 — a default repeater delays by one tick",
);

const _: () = assert!(
    MAX_ATTENUATION_SEGMENT > DUST_ATTENUATION_LIMIT + 1,
    "MAX_ATTENUATION_SEGMENT must exceed DUST_ATTENUATION_LIMIT + 1 steps so implicit buffers have a band to cover on a segment from a fresh source",
);

/// Output of a [`compile_delay`] run: the delayed scopes and the findings.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct DelayOutput {
    /// Placement IR for every scope delay insertion kept, with `local_delay_ticks` filled.
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

/// Lowers a routed [`ScopedPlacementIr`] into a delayed one, leaving out a scope refused with an
/// error.
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

/// Summed over the driving nets, not maxed: `local_delay_ticks` is a local wire cost.
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

/// The fewest buffer repeaters on a segment of `steps` steps from a fresh source; exact only on a
/// straight, unbranched run.
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
