//! Stage 2 of `spec/redstone` "Place-and-route": lays a Steiner tree per net, apart from the
//! others in its plane, and sets each cell's and pad's `wire_length` from the nets driving it.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::pass::{OpenScope, attribute_nodes, lower_scopes, open_scope};
use crate::placement::{CONGESTION_FIX, area_ratio_tenths};
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{net_order, sum_over_driving_nets, tile_layer_clearance};

/// Per-cell footprint of the post-routing congestion budget.
pub use crate::placement::CELL_FOOTPRINT;

/// Output of a [`compile_routing`] run: the routed scopes and the findings.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct RoutingOutput {
    /// Placement IR for every scope routing kept, with `wire_length` filled.
    pub scoped: ScopedPlacementIr,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

impl RoutingOutput {
    /// An empty output.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lowers a [`ScopedPlacementIr`] into a routed one, leaving out a scope refused with an error.
///
/// # Panics
///
/// On a `NetRef` out of range, a node not at the placement stage, or two cells on one `coord`.
#[must_use]
pub fn compile_routing(placement: &ScopedPlacementIr) -> RoutingOutput {
    let (scoped, diagnostics) = lower_scopes(placement, route_scope);
    RoutingOutput {
        scoped,
        diagnostics,
    }
}

type ScopeRouting = Result<(PlacementIr, Vec<Diagnostic>), Diagnostic>;

fn route_scope(entry: &ScopedPlacementIrEntry) -> ScopeRouting {
    let Some(OpenScope {
        mut ir,
        region,
        blocks,
        nets,
    }) = open_scope(entry, "placed", "routing")?
    else {
        return Ok((entry.ir.clone(), Vec::new()));
    };

    let mut occupancy: HashSet<CellCoord> = HashSet::with_capacity(ir.cells.len() * 4);
    occupancy.extend(blocks.iter().map(|site| site.coord));
    for net in net_order(&nets.sinks) {
        for coord in nets.trees[&net].wire_path() {
            occupancy.insert(coord);
        }
    }

    // Per net: dust shared with a sibling sink counts for both, and once in the budget below.
    attribute_nodes(
        &mut ir,
        entry,
        |_, cell| sum_over_driving_nets(&cell.drivers, |net| nets.segment(net, cell.coord)),
        |_, output| nets.segment(output.driver, output.pad),
        |phase, len, identity| phase.route_at(len, identity),
    );

    let cell_coord_set: HashSet<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();
    let wire_only: u64 = occupancy
        .iter()
        .filter(|c| !cell_coord_set.contains(c))
        .count() as u64;
    let cell_budget = (ir.cells.len() as u64).saturating_mul(u64::from(CELL_FOOTPRINT));
    let used = cell_budget.saturating_add(wire_only);
    let reserved = region.reserved_area();
    if used > reserved {
        return Err(congestion_diagnostic(entry, &region, used));
    }

    // After the refusals, so a refused scope carries no advisory.
    let advisories: Vec<Diagnostic> =
        tile_layer_clearance(&nets.sinks, &nets.trees, &nets.router, entry, &region)
            .into_iter()
            .collect();

    Ok((ir, advisories))
}

fn congestion_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    used: u64,
) -> Diagnostic {
    let reserved = reservation.reserved_area();
    // Placement guarantees `reserved_area > 0`; a hand-built IR may not.
    if reserved == 0 {
        return zero_reservation_diagnostic(entry, reservation);
    }
    let (whole, tenths) = area_ratio_tenths(used, reserved);
    let primary = format!(
        "routed netlist for {kind} `{name}` occupies ~{whole}.{tenths}x the reserved area (void={void}, region {width}x{depth})",
        kind = entry.kind.label(),
        name = entry.name,
        void = reservation.void,
        width = reservation.width,
        depth = reservation.depth,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        CONGESTION_FIX,
    )
}

fn zero_reservation_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` has a zero-area reservation (void={void}, region {width}x{depth}) — routing cannot lay any wire",
        kind = entry.kind.label(),
        name = entry.name,
        void = reservation.void,
        width = reservation.width,
        depth = reservation.depth,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        CONGESTION_FIX,
    )
}

#[cfg(test)]
mod tests;
