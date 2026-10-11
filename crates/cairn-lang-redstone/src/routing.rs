//! Placement IR → routed Placement IR lowering (Steiner routing).
//!
//! Stage 2 of the five in `spec/redstone` "Place-and-route": lays a rectilinear Steiner tree per
//! driver net inside each scope's [`crate::placement_ir::CircuitRegionReservation`] and sets every
//! cell's and actuator pad's [`crate::placement_ir::PlacedCellNode::wire_length`] to the routed
//! length of the nets driving it, summed.
//!
//! - **Nets.** Each cell driver is a sink on its driver's net and each
//!   output driver a sink on its actuator's net; the source is the input
//!   pad for `NetRef::Input` and the cell body for `NetRef::Cell`. An
//!   unused input adds its pad to the occupancy set but no net.
//! - **Trees.** `crate::routing_geometry::Router` grows each net one
//!   sink at a time by the cheapest path that runs through no block and
//!   neither over nor one step beside an earlier net's dust in its own
//!   plane, climbing to a [`crate::placement_ir::RouteLayer::Bridge`]
//!   layer where the plane offers no way round. Nets are laid in
//!   `crate::routing_geometry::net_order`, a total order, so the delay
//!   and crossing passes rebuild the same trees. Nets are kept apart here
//!   rather than at stage 4 so the climb is measured into `wire_length`
//!   and into the delay pass's tick count.
//! - **Cross-layer pairs.** A net that climbed runs over, or one step
//!   across from, the net it cleared. Separating those is the physical
//!   tile layer's obligation, so they are named once, here, by
//!   `W_ROUTE_CROSS_LAYER_CLEARANCE` rather than refused.
//! - **Refusals.** Each elides the scope so a partial `wire_length`
//!   never reaches stage 3. Three are `E_ROUTE_CONGESTION`: a pad the
//!   reservation cannot fit (its saturated z collapses onto a cell or
//!   another pad); a sink with no free path; and, after every net is
//!   laid, `cells * CELL_FOOTPRINT + wire-only coords > reserved area`.
//!   Each primary says which. Two more codes come from
//!   `crate::pass::lay_nets`: `E_ATTENUATION_LIMIT` for a sink further
//!   from its driver than the cap in a straight line, and
//!   `E_NO_CIRCUIT_REGION` for a scope with cells and no reservation.
//! - **Attribution.** `wire_length` is summed over the distinct nets
//!   driving a cell (two ports reading one signal are one strand), each
//!   measured as the routed path from the net's source into the cell —
//!   the measure stage 3 counts buffer repeaters against.
//!
//! Sensor pad coordinates are derived from the reservation where needed rather than stored.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::pass::{OpenScope, attribute_nodes, lower_scopes, open_scope};
use crate::placement::{CONGESTION_FIX, area_ratio_tenths};
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{net_order, sum_over_driving_nets, tile_layer_clearance};

/// Per-cell footprint of the post-routing congestion budget: the placement pass's, with the
/// routed wire counted on top.
pub use crate::placement::CELL_FOOTPRINT;

/// Output of a [`compile_routing`] run: the routed scopes, every cell having taken
/// [`crate::placement_ir::PlacementPhase::route`], and the findings.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct RoutingOutput {
    /// Placement IR for every scope whose routing succeeded, with
    /// every cell's `wire_length` field populated.
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

/// Lowers a [`ScopedPlacementIr`] into a routed one, reading each scope's reservation from its
/// `region`. A scope refused with an error (see the module doc) is left out, so a partial
/// `wire_length` never reaches delay insertion.
#[must_use]
pub fn compile_routing(placement: &ScopedPlacementIr) -> RoutingOutput {
    let (scoped, diagnostics) = lower_scopes(placement, route_scope);
    RoutingOutput {
        scoped,
        diagnostics,
    }
}

/// Result of routing one scope: the routed IR and its advisories, or one error. A refused scope
/// carries no advisory, since nothing will build it.
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

    // Occupancy for the congestion figure: the blocks and the dust laid over them. `lay_nets`
    // has already refused a repeated coord, so this only counts.
    let mut occupancy: HashSet<CellCoord> = HashSet::with_capacity(ir.cells.len() * 4);
    occupancy.extend(blocks.iter().map(|site| site.coord));
    for net in net_order(&nets.sinks) {
        for coord in nets.trees[&net].wire_path() {
            occupancy.insert(coord);
        }
    }

    // Per net rather than the shared tree total: dust a cell shares with
    // a sibling sink feeds both, and the congestion budget below is
    // where the shared total is counted once. An output has exactly one
    // segment, its driver to its pad.
    attribute_nodes(
        &mut ir,
        entry,
        |_, cell| sum_over_driving_nets(&cell.drivers, |net| nets.segment(net, cell.coord)),
        |_, output| nets.segment(output.driver, output.pad),
        |phase, len, identity| phase.route_at(len, identity),
    );

    // The placement pass's per-cell budget plus the wire-only coords, so a cell coord is not
    // counted twice.
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

    // Below the refusals, so a scope this pass elides carries no
    // advisory; said once, here, because this is the stage that made
    // the pairs.
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
    // `reserved_area > 0` is a placement-side invariant; a hand-built IR
    // that breaks it gets its own primary rather than a division panic.
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
