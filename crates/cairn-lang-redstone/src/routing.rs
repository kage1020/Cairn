//! Placement IR → routed Placement IR lowering (Steiner routing).
//!
//! Stage 2 of the five-stage pipeline `spec/redstone` "Place-and-route"
//! lays out. Lays a rectilinear Steiner tree per driver net inside each
//! scope's [`crate::placement_ir::CircuitRegionReservation`] and
//! rewrites every cell's and actuator pad's
//! [`crate::placement_ir::PlacedCellNode::wire_length`] from `None` to
//! `Some(routed length of the nets driving it, summed)`.
//! [`crate::placement_ir::PlacedCellNode::local_delay_ticks`] stays
//! `None`; stage 3 ([`crate::delay::compile_delay`]) promotes it.
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
//!   and crossing passes rebuild the same trees. Keeping nets apart here
//!   rather than at stage 4 is what gets the climb measured into
//!   `wire_length` and into the delay pass's tick count.
//! - **Cross-layer pairs.** A net that climbed runs over, or one step
//!   across from, the net it cleared. Separating those is the physical
//!   tile layer's obligation, so they are named once, here, by
//!   `W_ROUTE_CROSS_LAYER_CLEARANCE` rather than refused.
//! - **Refusals.** Each elides the scope so a partial `wire_length`
//!   never reaches stage 3. Three are `E_ROUTE_CONGESTION`: a pad the
//!   reservation cannot fit (its saturated z collapses onto a cell or
//!   another pad); a sink with no free path; and, after every net is
//!   laid, `cells * CELL_FOOTPRINT + wire-only coords > reserved area`.
//!   Each primary says which, so a reader can tell the placement pass's
//!   pessimistic cell budget from the routed layout. Two more codes
//!   reach this pass through `crate::pass::lay_nets`:
//!   `E_ATTENUATION_LIMIT` for a sink further from its driver than the
//!   v1 cap in a straight line, and `E_NO_CIRCUIT_REGION` for a scope
//!   carrying cells but no reservation to place them in.
//! - **Attribution.** `wire_length` is summed over the distinct nets
//!   driving a cell (two ports reading one signal are one strand), each
//!   measured as the routed path from the net's source into the cell —
//!   the measure stage 3 counts buffer repeaters against.
//!
//! The pad coordinates are derived on the fly and not stored; they would
//! become `PlacementIr` fields if a consumer outside routing ever needed
//! them. `RouteLayer::Bridge` has one producer, this pass; `RouteLayer::Via`
//! has none, because a climb is a step between two coords rather than a
//! coord of its own.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::pass::{
    OpenScope, Skipped, attribute_nodes, lay_nets, lower_scopes, missing_region_diagnostic,
    open_scope, source_of_net,
};
use crate::placement::{CONGESTION_FIX, area_ratio_tenths};
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{net_order, sum_over_driving_nets, tile_layer_clearance};

/// Per-cell footprint used by the post-routing congestion budget.
/// Re-exports [`crate::placement::CELL_FOOTPRINT`] so this pass carries
/// the same footprint model the placement pass used and adds wire
/// occupancy on top.
pub const CELL_FOOTPRINT: u32 = crate::placement::CELL_FOOTPRINT;

/// Output of a [`compile_routing`] run.
///
/// Mirrors the shape of [`crate::placement::PlacementOutput`] so
/// callers see a uniform result type across every stage of the
/// place-and-route pipeline. The routed IR is a
/// [`ScopedPlacementIr`] with every non-failed scope's
/// `wire_length` promoted from `None` to `Some(_)` — no new IR type;
/// the routing pass is one
/// [`crate::placement_ir::PlacementPhase::route`] transition per
/// cell, per the producer↔variant table on that enum.
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
    /// Empty output (no routed scopes, no diagnostics).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lower a [`ScopedPlacementIr`] into a routed [`ScopedPlacementIr`].
///
/// Reads the reservation from every scope's `region` field rather than
/// re-consulting the Intent IR — the Placement IR is self-describing
/// by construction, so the routing pass has no `IntentModule`
/// dependency.
///
/// One entry per non-empty [`PlacementIr`] whose routing succeeded;
/// scopes whose routing raises an Error-severity diagnostic are elided
/// from the output so a partial `wire_length` cannot pollute the
/// delay-insertion pass downstream. Today those are
/// `E_ROUTE_CONGESTION`, `E_ATTENUATION_LIMIT` for a sink the v1 cap
/// already puts out of reach in a straight line, and
/// `E_NO_CIRCUIT_REGION` for a scope carrying cells with no
/// reservation.
#[must_use]
pub fn compile_routing(placement: &ScopedPlacementIr) -> RoutingOutput {
    let (scoped, diagnostics) = lower_scopes(placement, route_scope);
    RoutingOutput {
        scoped,
        diagnostics,
    }
}

/// Result of routing one scope: the routed IR plus whatever the layout
/// is advised of on success, a single Error-severity diagnostic on
/// failure. A refused scope carries no advisory: what an elided scope
/// would have left the tile layer is not an obligation anything will be
/// asked to discharge.
type ScopeRouting = Result<(PlacementIr, Vec<Diagnostic>), Diagnostic>;

fn route_scope(entry: &ScopedPlacementIrEntry) -> ScopeRouting {
    let source = &entry.ir;
    let OpenScope {
        mut ir,
        region,
        cell_coords,
        inputs,
        column,
        blocks,
    } = match open_scope(entry) {
        Err(Skipped::Empty) => return Ok((source.clone(), Vec::new())),
        // Refused rather than passed through: this pass writes
        // `wire_length`, and the producer↔variant table on
        // `PlacementPhase` promises it after this stage. The same
        // policy the delay and crossing passes already apply — one
        // answer for one shape, whichever stage meets it first.
        Err(Skipped::MissingRegion) => {
            return Err(missing_region_diagnostic(entry, "placed", "routing"));
        }
        Ok(scope) => scope,
    };

    let nets = lay_nets(
        &ir,
        &blocks,
        entry,
        "placed",
        &region,
        source_of_net(&region, column, &cell_coords, inputs),
    )?;

    // Occupancy for the congestion figure below: the blocks, then the
    // dust laid over them. The refusal a repeated coord earns is
    // `lay_nets`', asked before any of this, so what is left here is
    // counting.
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

    // Congestion against the actual post-routing footprint: the
    // placement pass's pessimistic per-cell budget, plus the wire-only
    // coords laid on top (cell coords excluded so they are not counted
    // twice).
    let cell_coord_set: HashSet<CellCoord> = cell_coords.iter().copied().collect();
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
