//! Placement IR → routed Placement IR lowering (Steiner routing).
//!
//! Stage 2 of the five-stage place-and-route pipeline `spec/redstone`
//! §14.5 lays out (Placement → Steiner routing → Delay insertion →
//! Crossing legalization → Edition legalization). Walks every
//! [`crate::placement_ir::PlacedCellNode`] in each scope's Placement
//! IR, lays a rectilinear Manhattan Steiner tree per driver net inside
//! the enclosing scope's [`crate::placement_ir::CircuitRegionReservation`],
//! and rewrites every cell's [`crate::placement_ir::PlacedCellNode::wire_length`]
//! from `None` to `Some(sum of driver-to-cell Manhattan distances)`.
//! [`crate::placement_ir::PlacedCellNode::delay_ticks`] stays `None`
//! at this stage; the delay-insertion pass
//! ([`crate::delay::compile_delay`], stage 3 of §14.5) promotes it
//! to `Some(_)`.
//!
//! The v1 algorithm is deliberately minimal:
//!
//! - **Net collection.** Each cell driver produces one sink entry on
//!   its driver's net (`NetRef::Input(i)` or `NetRef::Cell(j)`), and
//!   each output driver produces one sink entry on its actuator's
//!   net. Unused inputs (a sensor whose signal reaches no cell or
//!   output) still contribute their pad coordinate to the occupancy
//!   set — otherwise a downstream congestion re-check would
//!   understate the routed area — but they add no net because there
//!   is nothing to route from them. Source coordinates are
//!   `NetRef::Input(i) → input_pad(i, region)` (left edge, z = 1 + i,
//!   saturating at `depth-1` for pathological regions — see
//!   [`input_pad`]) and `NetRef::Cell(j) → cells[j].coord`. Output
//!   pad coordinates are the right edge, z = 1 + k, saturating
//!   similarly.
//! - **Steiner tree.** Rectilinear minimum spanning tree over the
//!   `{source} ∪ sinks` terminal set — Kruskal with union-find on
//!   the complete Manhattan graph, then each MST edge is rendered as
//!   an L-shape (x-then-z-then-y, deterministic for regression
//!   stability) into the occupancy set. This is the Kou-Markowsky-Berman
//!   (KMB) approximation truncated at its second stage; the third
//!   KMB stage (Steiner-point insertion on the drawn edges) is not
//!   run here because the delay-insertion pass (stage 3, §14.4) only
//!   consumes the per-sink Manhattan sum, so the extra work would
//!   not shift a downstream decision.
//! - **Occupancy.** A per-scope `HashSet<CellCoord>` seeded with
//!   every cell coord, every input pad, and every output pad, then
//!   grown by each drawn L-shape. Duplicate visits share (Steiner
//!   fanout is the whole point). If seeding itself trips a duplicate
//!   — a pad collapsed onto a cell coord or another pad because the
//!   reservation cannot fit the pad row — the pass fires
//!   `E_ROUTE_CONGESTION` immediately with a "pad layout" primary
//!   rather than a silent misroute. Cross-net overlap between
//!   distinct signals during Steiner draw is tolerated in v1; the
//!   crossing-legalization pass (stage 4 of §14.5) is what owns
//!   those, refusing a scope whose `void=<N>` reservation is too
//!   thin to absorb them with `E_CROSSING_CONGESTION`.
//! - **`wire_length` attribution.** For every cell, `wire_length =
//!   sum over drivers of Manhattan(driver-source-coord, cell.coord)`.
//!   The tree-total path is not attributed per-sink today because the
//!   downstream delay-insertion pass (stage 3, §14.4) consumes the
//!   sum of Manhattan distances, and re-tree-walking here would
//!   double the work without shifting the delay decision.
//! - **Congestion.** After every net is laid,
//!   `cells.len() * CELL_FOOTPRINT + wire_only_coords > reserved_area`
//!   fires `E_ROUTE_CONGESTION` against the reservation span. The
//!   primary message differs from the placement-pass version so a
//!   downstream reader can tell whether the pessimistic cell-only
//!   budget or the actual routed layout was the trigger. Failed
//!   scopes are elided from the output list so a partial
//!   `wire_length` never reaches the delay-insertion pass — a partial
//!   attribution would let stage 3 compute delays against a layout
//!   that no downstream stage can materialise into voxels, silently
//!   corrupting `assert latency(...)` verification per §14.7.
//!
//! One intentional gap is left here: the input / output pad
//! coordinates the routing pass derives on the fly are not stored, and
//! would become a `PlacementIr` field (`input_pads` / `output_pads`)
//! if a consumer ever needs them outside routing — that migration is
//! `#[non_exhaustive]`-safe on both types. The escape layers are not
//! such a gap: `RouteLayer::Bridge` has its producer in
//! [`crate::crossing::compile_crossing`] (stage 4 of §14.5), but only
//! for implicit buffer-repeater coords — v1 lifts no wire coord onto
//! an escape layer, and `RouteLayer::Via` has no producer at all.
//! Attenuation accounting has landed as
//! [`crate::delay::compile_delay`] (stage 3): the
//! delay pass re-derives per-driver Manhattan segments from the same
//! `NetRef → source coord` mapping used here, counts implicit buffer
//! repeaters for segments beyond the 15-block dust attenuation limit,
//! and refuses with `E_ATTENUATION_LIMIT` when a single segment
//! exceeds the v1 sanity cap.

use std::collections::HashMap;
use std::collections::HashSet;

use cairn_lang_core::check::Severity;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CellIdentity, CircuitRegionReservation, PlacementIr, ScopedPlacementIr,
    ScopedPlacementIrEntry,
};
use crate::routing_geometry::{input_pad, manhattan, net_ref_key, net_wire_path, output_pad};

/// Per-cell footprint used by the post-routing congestion budget.
/// Re-exports [`crate::placement::CELL_FOOTPRINT`] so a scope that
/// placed at the cell-only budget boundary needs at most one Manhattan
/// segment of new wire to flip to `E_ROUTE_CONGESTION` at this stage —
/// the routing pass carries the same footprint model the placement
/// pass used and adds wire occupancy on top.
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
/// scopes whose routing raises an Error-severity diagnostic (today,
/// only `E_ROUTE_CONGESTION`) are elided from the output so a partial
/// `wire_length` cannot pollute the delay-insertion pass downstream.
#[must_use]
pub fn compile_routing(placement: &ScopedPlacementIr) -> RoutingOutput {
    let mut out = RoutingOutput::new();
    for entry in &placement.scopes {
        match route_scope(entry) {
            Ok(ir) => {
                out.scoped.scopes.push(ScopedPlacementIrEntry {
                    kind: entry.kind,
                    name: entry.name.clone(),
                    ir,
                });
            }
            Err(diagnostic) => out.diagnostics.push(diagnostic),
        }
    }
    out
}

/// Result of routing one scope: the routed IR on success, a single
/// Error-severity diagnostic on failure.
type ScopeRouting = Result<PlacementIr, Diagnostic>;

fn route_scope(entry: &ScopedPlacementIrEntry) -> ScopeRouting {
    let source = &entry.ir;
    // Defensive pass-through: a scope with no cells cannot be laid
    // out further than placement already did. `ScopedPlacementIr::push`
    // elides these on the input side, so this branch is a
    // belt-and-braces for hand-built IRs.
    if source.cells.is_empty() {
        return Ok(source.clone());
    }
    let Some(region) = source.region.clone() else {
        // The upstream placement pass fires `E_NO_CIRCUIT_REGION` and
        // elides any scope with cells but no region before it can
        // reach the routing pass. A hand-built IR reaching here with
        // cells and no region is a caller-side bug — assert loud in
        // debug builds so a fixture regression trips fast, then fall
        // through with a pass-through in release so a downstream
        // consumer still sees deterministic output.
        debug_assert!(
            source.cells.is_empty(),
            "route_scope received a PlacementIr with cells but no region — placement should have elided it",
        );
        return Ok(source.clone());
    };

    let mut ir = source.clone();

    // Snapshot the cell coord list up front so the `wire_length`
    // rewrite that follows can index into `ir.cells` mutably without
    // re-borrowing across the `source_of_net` helper.
    let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();

    let source_of_net = |net: NetRef| -> CellCoord {
        match net {
            NetRef::Input(i) => input_pad(i as usize, &region),
            NetRef::Cell(j) => {
                // `j < ir.cells.len()` by the topological invariant
                // carried across every prior IR stage (`NetRef::Cell(j)`
                // in `cells[i]` satisfies `j < i`). Assert loud in
                // debug builds so a fixture that breaks the invariant
                // is caught immediately; in release, saturate the
                // lookup to the last cell so a caller-side bug still
                // produces deterministic output rather than a panic.
                debug_assert!(
                    (j as usize) < cell_coords.len(),
                    "NetRef::Cell({j}) out of range (cells.len()={}) — topological invariant broken",
                    cell_coords.len(),
                );
                cell_coords
                    .get(j as usize)
                    .copied()
                    .unwrap_or_else(|| *cell_coords.last().expect("cells.is_empty checked above"))
            }
        }
    };

    // Occupancy seed: every cell coord, then every input pad, then
    // every output pad. A duplicate insert means the reservation
    // cannot fit the pad row without collapsing pads onto a cell or
    // another pad — that is a real overflow the routing pass owns,
    // not a silent misroute, so fire `E_ROUTE_CONGESTION` with a
    // "pad layout" primary immediately.
    let mut occupancy: HashSet<CellCoord> = HashSet::with_capacity(ir.cells.len() * 4);
    for coord in &cell_coords {
        occupancy.insert(*coord);
    }
    for i in 0..ir.inputs.len() {
        let pad = input_pad(i, &region);
        if !occupancy.insert(pad) {
            return Err(pad_overlap_diagnostic(entry, &region, "input", i, pad));
        }
    }
    for k in 0..ir.outputs.len() {
        let pad = output_pad(k, &region);
        if !occupancy.insert(pad) {
            return Err(pad_overlap_diagnostic(entry, &region, "output", k, pad));
        }
    }

    // Collect nets: source → sink list. `HashMap` order is not
    // relied on — we sort the key set below before laying wires.
    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    for (i, cell) in ir.cells.iter().enumerate() {
        let sink = cell_coords[i];
        for driver in &cell.drivers {
            nets.entry(driver.net).or_default().push(sink);
        }
    }
    for (k, output) in ir.outputs.iter().enumerate() {
        let sink = output_pad(k, &region);
        nets.entry(output.driver).or_default().push(sink);
    }

    // Process nets in a deterministic order: fanout descending, tie
    // by NetRef key ascending. Sorting is inert against the v1
    // occupancy model (both L-shape elbows have identical Manhattan
    // length and `l_shape_path` picks a fixed axis order), but pins
    // a stable schedule so a follow-up pass that consults occupancy
    // for elbow selection has one deterministic order to slot into
    // without rewriting the caller.
    let mut net_order: Vec<NetRef> = nets.keys().copied().collect();
    net_order.sort_by(|a, b| {
        let fa = nets[a].len();
        let fb = nets[b].len();
        fb.cmp(&fa)
            .then_with(|| net_ref_key(*a).cmp(&net_ref_key(*b)))
    });

    for net in net_order {
        let sinks = &nets[&net];
        if sinks.is_empty() {
            continue;
        }
        let source_coord = source_of_net(net);
        for coord in net_wire_path(source_coord, sinks) {
            occupancy.insert(coord);
        }
    }

    attribute_wire_lengths(&mut ir, entry, &cell_coords, &source_of_net);

    // Congestion check against the actual post-routing footprint.
    // `cells.len() * CELL_FOOTPRINT` carries forward the pessimistic
    // per-cell budget the placement pass used; `wire_only` counts the
    // Steiner-shared unique wire coords the routing pass laid on top
    // (any block already staked as a cell coord is excluded so the
    // cell budget is not double-counted).
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

    Ok(ir)
}

/// Fill every cell's `wire_length` with the sum of Manhattan
/// distances from each driver's source into that cell. The
/// Steiner-shared tree total is used for congestion accounting at
/// the routing-pass level; per-sink Manhattan is what the
/// delay-insertion pass (§14.4) consumes, so attributing it here
/// saves that pass a second walk.
///
/// Computes into a side vector first so `ir.cells` can be borrowed
/// immutably while the driver sources are looked up through
/// `source_of_net`, then commits in a mutable pass. The commit is
/// loud in release too: `PlacementPhase::route_at` panics on any
/// non-`Unrouted` variant, which is what a caller who routed twice
/// hands us — the producer↔variant table on `PlacementPhase`
/// forbids it.
/// `entry` is threaded in purely so that panic can name the offending
/// cell instead of leaving the operator to walk back from the
/// backtrace.
fn attribute_wire_lengths<F>(
    ir: &mut PlacementIr,
    entry: &ScopedPlacementIrEntry,
    cell_coords: &[CellCoord],
    source_of_net: &F,
) where
    F: Fn(NetRef) -> CellCoord,
{
    let wire_lengths: Vec<u32> = ir
        .cells
        .iter()
        .zip(cell_coords.iter())
        .map(|(cell, &sink)| {
            cell.drivers.iter().fold(0u32, |acc, driver| {
                acc.saturating_add(manhattan(source_of_net(driver.net), sink))
            })
        })
        .collect();
    for (index, (cell, len)) in ir.cells.iter_mut().zip(wire_lengths).enumerate() {
        let identity = CellIdentity::new(index, cell.coord, entry);
        cell.phase.route_at(len, identity);
    }
}

fn congestion_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    used: u64,
) -> Diagnostic {
    let reserved = reservation.reserved_area();
    // `reserved_area > 0` is a placement-side invariant (width /
    // depth are `NonZeroU32` in the Intent IR and `void=0` is refused
    // by the placement pass). A hand-built IR that reaches here with
    // `reserved == 0` would panic on the ratio division below —
    // fall back to a divide-by-zero-free primary that still names
    // the failed scope so the caller sees a diagnostic rather than
    // an `ExitCode(101)`.
    if reserved == 0 {
        return zero_reservation_diagnostic(entry, reservation);
    }
    let ratio_x10 = (used.saturating_mul(10)) / reserved;
    let whole = ratio_x10 / 10;
    let tenths = ratio_x10 % 10;
    let primary = format!(
        "routed netlist for {kind} `{name}` occupies ~{whole}.{tenths}x the reserved area (void={void}, region {width}x{depth})",
        kind = entry.kind.label(),
        name = entry.name,
        void = reservation.void,
        width = reservation.width,
        depth = reservation.depth,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: increase `void`, enlarge region, or split into multiple `circuit` blocks",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

fn pad_overlap_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    pad_kind: &str,
    pad_index: usize,
    pad_coord: CellCoord,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` cannot fit its {pad_kind} pad #{pad_index} at ({x},{y},{z}) — the reserved area (void={void}, region {width}x{depth}) collapses I/O pads onto a cell coord or another pad",
        kind = entry.kind.label(),
        name = entry.name,
        x = pad_coord.x,
        y = pad_coord.y,
        z = pad_coord.z,
        void = reservation.void,
        width = reservation.width,
        depth = reservation.depth,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: enlarge `size=WxH` so `depth >= max(inputs, outputs) + 1`, or split into multiple `circuit` blocks",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
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
    let mut diag = Diagnostic::new(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: increase `void`, enlarge region, or split into multiple `circuit` blocks",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

#[cfg(test)]
mod tests {
    //! Crate-internal coverage for the routing pass's phase-transition
    //! commit. `tests/routing.rs` drives real synth fixtures and can
    //! only ever hand this pass a uniformly `Unrouted` IR; building a
    //! scope whose cells sit in different phases needs the
    //! `pub(crate)` `phase` field, so it lives here.

    use cairn_lang_core::Edition;
    use cairn_lang_core::error::Span;

    use super::compile_routing;
    use crate::edition_netlist_ir::EditionCell;
    use crate::logic_ir::ScopeKind;
    use crate::placement_ir::{
        CellCoord, CircuitRegionReservation, PlacedCellNode, PlacementIr, PlacementPhase,
        ScopedPlacementIr, ScopedPlacementIrEntry,
    };

    fn reservation(width: u32, depth: u32, void: u32) -> CircuitRegionReservation {
        CircuitRegionReservation {
            label: "floor".to_owned(),
            void,
            width,
            depth,
            span: Span::default(),
        }
    }

    fn scoped(kind: ScopeKind, name: &str, ir: PlacementIr) -> ScopedPlacementIr {
        let mut scoped = ScopedPlacementIr::new();
        scoped.scopes.push(ScopedPlacementIrEntry {
            kind,
            name: name.to_owned(),
            ir,
        });
        scoped
    }

    fn placed_cell(coord: CellCoord, phase: PlacementPhase) -> PlacedCellNode {
        PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers: vec![],
            coord,
            phase,
            span: Span::default(),
        }
    }

    #[test]
    #[should_panic(
        expected = "for cell #1 at (4,0,1) in struct `mixed` — routing must run exactly once per placement"
    )]
    fn route_panic_names_the_offending_cell_not_the_first_one() {
        // Re-running the whole pass always trips on `cells[0]`, which
        // would let a regression that hardcoded the index to zero — or
        // that read the coord off the wrong cell — pass unnoticed. A
        // hand-built IR whose first cell is still `Unrouted` while the
        // second is already `Routed` forces the panic past the head of
        // the loop, so both the index and the coord have to be
        // threaded from the cell actually being transitioned.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(8, 3, 2));
        ir.cells.push(placed_cell(
            CellCoord::new(0, 0, 0),
            PlacementPhase::Unrouted,
        ));
        ir.cells.push(placed_cell(
            CellCoord::new(4, 0, 1),
            PlacementPhase::Routed { wire_length: 0 },
        ));
        let _ = compile_routing(&scoped(ScopeKind::Struct, "mixed", ir));
    }
}
