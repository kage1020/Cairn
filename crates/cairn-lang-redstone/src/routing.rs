//! Placement IR → routed Placement IR lowering (Steiner routing).
//!
//! Stage 2 of the five-stage place-and-route pipeline `spec/redstone`
//! §14.5 lays out (Placement → Steiner routing → Delay insertion →
//! Crossing legalization → Edition legalization). Walks every
//! [`crate::placement_ir::PlacedCellNode`] in each scope's Placement
//! IR, lays a rectilinear Steiner tree per driver net inside
//! the enclosing scope's [`crate::placement_ir::CircuitRegionReservation`],
//! and rewrites every cell's [`crate::placement_ir::PlacedCellNode::wire_length`]
//! from `None` to `Some(sum over the nets driving it of the routed
//! length into the cell)`.
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
//! - **Steiner tree.** Rectilinear tree over the `{source} ∪ sinks`
//!   terminal set, grown one sink at a time by
//!   [`crate::routing_geometry::Router`]: the nearest sink still
//!   unconnected is attached to the wire already laid by the cheapest
//!   path that runs through no block, and neither over another net's
//!   dust nor one step from it in its own plane, and
//!   the search behind that is what keeps dust out of the cell bodies
//!   and pads the reservation already holds. Every sink is a leaf,
//!   because a component consumes the signal that reaches it rather
//!   than handing it on. Where nothing is in the way the path is the
//!   x-then-z-then-y L-shape the downstream stages were built around.
//! - **One net at a time.** The nets are laid in
//!   [`crate::routing_geometry::net_order`], and each goes round the
//!   dust of the ones before it and the coords beside that dust. Two
//!   nets on one coord would be one strand of dust carrying two
//!   signals, and so would two nets one step apart, because dust joins
//!   the dust next to it; §14.5 calls the way out an escape, and here
//!   it is the same search climbing to a bridge layer that already
//!   went round a cell body. Beside is per-plane: what a strand at
//!   `y + 1` reads is the physical tile layer's question, not this
//!   pass's. Doing it at this stage
//!   rather than at stage 4 is what gets the climb measured: the
//!   `wire_length` below and the delay pass's tick count are both read
//!   off the routed tree.
//! - **Unroutable sinks.** A sink with no free path from its driver —
//!   every way out walled in by a component, by an earlier net's dust
//!   or the coords beside it, or by the edge of the reservation — fires `E_ROUTE_CONGESTION`
//!   with its own primary naming the two coords, and the scope is
//!   elided. Refused before the area arithmetic below, because the area
//!   can be ample and the one coord the wire needs still be taken. This
//!   is what a crossing becomes: a layout with nowhere for the second
//!   net to go is refused rather than shorted.
//! - **Occupancy.** A per-scope `HashSet<CellCoord>` seeded with
//!   every cell coord, every input pad, and every output pad, then
//!   grown by each routed tree. Duplicate visits share (fanout is the
//!   whole point). If seeding itself trips a duplicate
//!   — a pad collapsed onto a cell coord or another pad because the
//!   reservation cannot fit the pad row — the pass fires
//!   `E_ROUTE_CONGESTION` immediately with a "pad layout" primary
//!   rather than a silent misroute. Between distinct signals there is
//!   no overlap left to count: the trees are laid around each other.
//! - **`wire_length` attribution.** For every cell, `wire_length =
//!   sum over the distinct nets driving it of the routed length from
//!   that net's source into this cell` — `route_to`, the same measure
//!   stage 3 counts buffer repeaters against. Distinct nets rather
//!   than drivers: two ports reading one signal are fed by one strand
//!   of dust. The tree total is not attributed per-sink either: dust a
//!   cell shares with a sibling sink feeds both, and the congestion
//!   budget below is where the shared total is counted once.
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
//! `#[non_exhaustive]`-safe on both types. `RouteLayer::Bridge` has one
//! producer, this pass, whose wire climbs off the ground layer to get
//! past a block or past another net. A buffer repeater the crossing
//! pass places inherits the layer of the route coord it stands on, so
//! a lifted repeater is a repeater on lifted wire rather than a second
//! producer. The layer is stamped through
//! [`crate::placement_ir::CellCoord::new`], so one voxel has one
//! key. `RouteLayer::Via` has no producer at all: a climb is a step
//! between two coords rather than a coord of its own, so there is
//! nothing for the variant to name.
//! Attenuation accounting has landed as
//! [`crate::delay::compile_delay`] (stage 3): the
//! delay pass re-derives the same per-net routed segments from the
//! `NetRef → source coord` mapping used here, counts implicit buffer
//! repeaters for segments beyond the 15-block dust attenuation limit,
//! and refuses with `E_ATTENUATION_LIMIT` when a single segment
//! exceeds the v1 sanity cap.

use std::collections::HashSet;

use cairn_lang_core::check::Severity;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CellIdentity, CircuitRegionReservation, PlacementIr, ScopedPlacementIr,
    ScopedPlacementIrEntry,
};
use crate::routing_geometry::{
    BlockKind, BlockSite, Router, block_sites, collect_nets, input_pad, net_order, net_trees,
    sum_over_driving_nets, unroutable,
};

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
    // Defensive pass-through: a scope with neither cells nor actuator
    // pads has nothing to lay out. `ScopedPlacementIr::push` elides
    // these on the input side, so this branch is a belt-and-braces for
    // hand-built IRs. An identity wire — outputs but no cells — is not
    // one of them: its segment runs from a sensor pad to an actuator
    // pad and is routed like any other.
    if source.cells.is_empty() && source.outputs.is_empty() {
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
            source.cells.is_empty() && source.outputs.is_empty(),
            "route_scope received a PlacementIr with cells or pads but no region — placement should have refused it",
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

    // Occupancy seed: every block standing in the reservation, in the
    // order `block_sites` lists them. A pad landing on a coord already
    // taken means the reservation cannot fit the pad row without
    // collapsing it onto a cell or another pad — that is a real
    // overflow the routing pass owns, not a silent misroute, so fire
    // `E_ROUTE_CONGESTION` with a "pad layout" primary immediately.
    // Two cells cannot collide: a cell's x is `1 + 2 * topological
    // index`, so no two of them are the same column.
    let blocks = block_sites(&ir, &region);
    let mut occupancy: HashSet<CellCoord> = HashSet::with_capacity(ir.cells.len() * 4);
    for site in &blocks {
        if !occupancy.insert(site.coord) && site.kind != BlockKind::Cell {
            return Err(pad_overlap_diagnostic(entry, &region, site));
        }
    }

    // Nets and their routed trees come from `routing_geometry`, which
    // the delay and crossing passes call with the same arguments — the
    // same blocks, so the same wire. The tree is the only description
    // of where a net's dust runs, so stage 3's buffer count and stage
    // 4's buffer coords are measured against the wire this stage
    // actually laid.
    //
    // `net_order` decides the trees: `net_trees` lays the nets in it,
    // each going round the dust of the ones before it, so the order is
    // part of the geometry rather than a tidy-up on the way out. It is
    // a total order over the nets of a scope, which is what lets the
    // three passes that rebuild the trees be told the same thing.
    let router = Router::new(&region, &blocks);
    let nets = collect_nets(&ir);
    let trees = net_trees(&nets, &router, source_of_net);
    // A sink with no free route — nothing between it and its driver but
    // blocks and the dust of nets already laid — is a layout this
    // reservation cannot hold, and saying so here is what keeps a wire
    // drawn through a comparator, or through another signal, out of
    // the IR. Refused before the area
    // arithmetic below, because the area is not what is wrong.
    if let Some(diagnostic) = unroutable(&nets, &trees, entry, &region, source_of_net) {
        return Err(diagnostic);
    }
    for net in net_order(&nets) {
        for coord in trees[&net].wire_path() {
            occupancy.insert(coord);
        }
    }

    // The routed length from a driver's source to one of its sinks —
    // the same measure the delay pass counts buffer repeaters against.
    // `route_to` answers `None` only for a sink that is not a terminal
    // of the net, which `collect_nets` rules out: it built the tree's
    // terminal list from this driver list.
    let segment_of = |net: NetRef, sink: CellCoord| -> u32 {
        let route = trees
            .get(&net)
            .and_then(|tree| tree.route_to(sink))
            .unwrap_or_else(|| {
                panic!(
                    "sink ({x},{y},{z}) is not a terminal of the net driving it — the driver list and the collected nets disagree",
                    x = sink.x,
                    y = sink.y,
                    z = sink.z,
                )
            });
        u32::try_from(route.len().saturating_sub(1)).unwrap_or(u32::MAX)
    };

    attribute_wire_lengths(&mut ir, entry, &cell_coords, &segment_of);

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

/// Fill every cell's `wire_length` with the routed length of each of
/// the nets driving it, summed.
///
/// Per-net rather than the shared tree total, which is what
/// the congestion budget counts: a cell's figure answers "how much
/// dust feeds this cell", and dust shared with a sibling sink feeds
/// both. Per-net rather than per-driver for the same reason one step
/// closer in — two ports reading one signal are the same strand
/// arriving twice; see [`sum_over_driving_nets`]. Routed rather than
/// Manhattan because the two are different
/// numbers whenever the wire goes round something, and a record
/// carrying a straight-line `wire_length` beside a `delay_ticks`
/// charged for the routed one describes no single layout.
///
/// Computes into a side vector first so `ir.cells` can be borrowed
/// immutably while the driver routes are measured through
/// `segment_of`, then commits in a mutable pass. The commit is
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
    segment_of: &F,
) where
    F: Fn(NetRef, CellCoord) -> u32,
{
    let wire_lengths: Vec<u32> = ir
        .cells
        .iter()
        .zip(cell_coords.iter())
        .map(|(cell, &sink)| sum_over_driving_nets(&cell.drivers, |net| segment_of(net, sink)))
        .collect();
    for (index, (cell, len)) in ir.cells.iter_mut().zip(wire_lengths).enumerate() {
        let identity = CellIdentity::new(index, cell.coord, entry);
        cell.phase.route_at(len, identity);
    }

    // An output has exactly one segment — its driver to its pad — so
    // there is nothing to sum, but it is measured the same way and
    // recorded in the same field.
    let output_lengths: Vec<u32> = ir
        .outputs
        .iter()
        .map(|output| segment_of(output.driver, output.pad))
        .collect();
    for (index, (output, len)) in ir.outputs.iter_mut().zip(output_lengths).enumerate() {
        let identity = CellIdentity::output(index, output.pad, entry);
        output.phase.route_at(len, identity);
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
    site: &BlockSite,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` cannot fit its {pad_kind} pad #{pad_index} at ({x},{y},{z}) — the reserved area (void={void}, region {width}x{depth}) collapses I/O pads onto a cell coord or another pad",
        kind = entry.kind.label(),
        name = entry.name,
        pad_kind = site.kind.as_str(),
        pad_index = site.index,
        x = site.coord.x,
        y = site.coord.y,
        z = site.coord.z,
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
    use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
    use crate::placement_ir::{
        CellCoord, CircuitRegionReservation, PlacedCellNode, PlacedOutputNode, PlacementIr,
        PlacementPhase, ScopedPlacementIr, ScopedPlacementIrEntry,
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

    /// `wire_length` reports the dust that feeds a cell, so it counts
    /// the routed path and not the straight line between the driver's
    /// source and the cell. The sink here is 14 blocks from its pad
    /// with something standing halfway, so it is fed by 16 blocks of
    /// dust; a record carrying 14 beside a `delay_ticks` charged for
    /// 16 describes no single layout.
    ///
    /// The blocker drives nothing — a block is all the fixture needs,
    /// and a driver would make it a second sink of the same net.
    /// No net in the example corpus has dust drawn inside a component.
    ///
    /// The property the router exists for, checked against every `.crn`
    /// that ships rather than against generated layouts: a coord of a
    /// net's wire is either free space, that net's own source, or one
    /// of its own sinks. Anything else is a strand of redstone inside a
    /// comparator or a pressure plate.
    ///
    /// A unit test rather than one in `tests/routing.rs` because the
    /// wire coords are crate-internal — the routed IR carries lengths,
    /// not paths.

    #[test]
    fn no_example_draws_dust_inside_a_component() {
        use std::collections::{HashMap, HashSet};
        use std::path::PathBuf;

        use cairn_lang_core::{lower, parse};

        use crate::routing_geometry::{
            Router, block_sites, collect_nets, input_pad, manhattan, net_trees,
        };

        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples");
        // What the corpus is worth is what it still routes. A count of
        // wire coords would stay comfortably positive while an example
        // slid into `E_ROUTE_CONGESTION` and vanished from the walk, so
        // the guards below are the scopes placement laid against the
        // scopes routing kept, and the number of strands that actually
        // go round something.
        let mut placed_scopes = 0usize;
        let mut routed_scopes = 0usize;
        let mut detours = 0usize;
        for file in std::fs::read_dir(&dir).expect("read examples") {
            let path = file.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("crn") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read example");
            let Ok(module) = parse(&source) else { continue };
            let intent = lower(&module);
            let synth = crate::synth::synthesize(&intent);
            for edition in [Edition::Java, Edition::Bedrock] {
                let netlist = crate::netlist::compile_netlist(&synth.scoped);
                let edition_netlist =
                    crate::edition_netlist::compile_edition_netlist(&netlist, edition);
                let placed = crate::placement::compile_placement(&edition_netlist, &intent);
                let laid = compile_routing(&placed.scoped);
                placed_scopes += placed.scoped.scopes.len();
                routed_scopes += laid.scoped.scopes.len();
                for entry in &laid.scoped.scopes {
                    let ir = &entry.ir;
                    let Some(region) = ir.region.clone() else {
                        continue;
                    };
                    let coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();
                    let blocks = block_sites(ir, &region);
                    let occupied: HashSet<CellCoord> =
                        blocks.iter().map(|site| site.coord).collect();
                    let router = Router::new(&region, &blocks);
                    let nets = collect_nets(ir);
                    let trees = net_trees(&nets, &router, |net| match net {
                        NetRef::Input(i) => input_pad(i as usize, &region),
                        NetRef::Cell(j) => coords[j as usize],
                    });
                    let where_it_is = format!(
                        "{}: {edition:?} {} `{}`",
                        path.display(),
                        entry.kind.label(),
                        entry.name,
                    );
                    let mut owner: HashMap<CellCoord, NetRef> = HashMap::new();
                    for (net, tree) in &trees {
                        let source = tree.wire_path()[0];
                        let mine: HashSet<CellCoord> =
                            nets[net].iter().copied().chain([source]).collect();
                        for coord in tree.wire_path() {
                            assert!(
                                !occupied.contains(&coord) || mine.contains(&coord),
                                "{where_it_is} draws {net:?} through {coord:?}",
                            );
                        }
                        claim(&mut owner, *net, &router.dust(tree), &where_it_is);
                        for sink in &nets[net] {
                            let route = tree.route_to(*sink).expect("a sink of this net");
                            let walked =
                                u32::try_from(route.len() - 1).expect("a route fits in u32");
                            if walked > manhattan(source, *sink) {
                                detours += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(
            routed_scopes, placed_scopes,
            "routing refused a scope the corpus used to lay out, and this walk \
             cannot see what it no longer visits",
        );
        assert!(
            routed_scopes >= 4,
            "the corpus has to keep placing circuits for this to mean anything: \
             {routed_scopes} scope(s)",
        );
        assert!(
            detours > 0,
            "no strand in the corpus goes round anything, so nothing here would \
             notice a router that drew straight through",
        );
    }

    /// One net's dust, checked against the nets already claimed and
    /// then added to them.
    ///
    /// Two nets on one coord, or one step apart in one plane, are two
    /// signals on one strand of dust. The proptest in
    /// `routing_geometry` holds this over generated boxes; the walk
    /// below holds it over the geometry the placement pass actually
    /// produces, which is where the corpus's shorts used to come from.
    ///
    /// `owner` holds dust and the reach is asked about per coord, so
    /// two strands two apart — each reaching the coord between them —
    /// are not mistaken for one.
    fn claim(
        owner: &mut std::collections::HashMap<CellCoord, NetRef>,
        net: NetRef,
        dust: &[CellCoord],
        where_it_is: &str,
    ) {
        use crate::routing_geometry::beside;

        for coord in dust {
            for taken in std::iter::once(*coord).chain(beside(*coord)) {
                if let Some(other) = owner.get(&taken)
                    && *other != net
                {
                    panic!(
                        "{where_it_is} runs {net:?} through {coord:?}, which \
                         {other:?} stands on or reaches",
                    );
                }
            }
        }
        for coord in dust {
            owner.insert(*coord, net);
        }
    }

    /// A pad the reservation cannot fit is refused, and the scope is
    /// elided rather than routed against a collapsed pad row.
    ///
    /// `input_pad` and `output_pad` saturate their z at `depth - 1`, so
    /// a reservation too shallow for the pad row lands two pads on one
    /// coord, or a pad on a cell body. Both are tested here as pure
    /// functions elsewhere; what those functions saturate *into* is
    /// this refusal, and nothing reached it before — a false negative
    /// is not a missing diagnostic, it is two nets sharing a source
    /// coord and a `wire_length` measured against it.
    ///
    /// Hand-built because the placement pass refuses a scope whose row
    /// is short before routing sees it, so the shapes below are only
    /// reachable through a caller assembling the IR itself.
    #[test]
    fn a_pad_row_the_reservation_cannot_fit_is_refused() {
        /// One shallow reservation, and the pad the refusal has to
        /// name once its z saturates onto something already there.
        struct Row {
            region: CircuitRegionReservation,
            inputs: usize,
            kind: &'static str,
            index: usize,
            coord: &'static str,
        }

        let rows = [
            Row {
                // depth 2 leaves two rows for the pad column, so
                // input #2 saturates onto input #1.
                region: reservation(4, 2, 1),
                inputs: 3,
                kind: "input",
                index: 2,
                coord: "(0,0,1)",
            },
            Row {
                // depth 1 leaves none at all, so the actuator pad lands
                // on the cell at the right-hand edge.
                region: reservation(2, 1, 1),
                inputs: 1,
                kind: "output",
                index: 0,
                coord: "(1,0,0)",
            },
        ];

        for Row {
            region,
            inputs,
            kind,
            index,
            coord,
        } in rows
        {
            let mut ir = PlacementIr::new(Edition::Java);
            let pad = crate::routing_geometry::output_pad(0, &region);
            ir.region = Some(region);
            for name in 0..inputs {
                ir.inputs.push(crate::netlist_ir::NetlistInput {
                    name: cairn_lang_core::ast::DottedRef::new(
                        "sig".into(),
                        vec![format!("s{name}")],
                    ),
                    span: Span::default(),
                });
            }
            ir.cells.push(PlacedCellNode {
                cell: EditionCell::JavaRepeaterOr,
                drivers: vec![CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                }],
                coord: CellCoord::new(1, 0, 0),
                phase: PlacementPhase::Unrouted,
                span: Span::default(),
            });
            ir.outputs.push(PlacedOutputNode::new(
                cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["out".into()]),
                NetRef::Cell(0),
                pad,
                Span::default(),
            ));

            let routed = compile_routing(&scoped(ScopeKind::Struct, "shallow", ir));
            let refusal = routed
                .diagnostics
                .iter()
                .find(|d| d.code == crate::DiagnosticCode::RouteCongestion)
                .unwrap_or_else(|| {
                    panic!(
                        "a {kind} pad on a taken coord must refuse: {:?}",
                        routed.diagnostics
                    )
                });
            assert!(
                refusal.primary.contains(&format!("{kind} pad #{index}")),
                "the refusal names which pad could not fit: {}",
                refusal.primary,
            );
            assert!(
                refusal.primary.contains(coord) && refusal.primary.contains("collapses I/O pads"),
                "and where, and why: {}",
                refusal.primary,
            );
            assert!(
                routed.scoped.scopes.is_empty(),
                "the failed scope is elided rather than routed against a collapsed pad row",
            );
        }
    }

    /// A scope with several unwireable sinks says how many, so sizing
    /// it is one decision rather than a fix-one-recompile loop.
    ///
    /// `Router::tree` strands every sink still unconnected the moment
    /// one round of the search comes back empty, so `unreachable()`
    /// holds them in batches; naming only the first would send the
    /// author round again for each.
    #[test]
    fn a_scope_with_several_unwireable_sinks_counts_them() {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(4, 2, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        // The pad lands at (0,0,1). Walling the row at (1,0,0) and the
        // course above the two sinks leaves them nothing to be fed
        // from, and `void=1` reserves no layer to come in over.
        for coord in [
            CellCoord::new(1, 0, 0),
            CellCoord::new(2, 0, 1),
            CellCoord::new(3, 0, 1),
        ] {
            ir.cells.push(placed_cell(coord, PlacementPhase::Unrouted));
        }
        for coord in [CellCoord::new(2, 0, 0), CellCoord::new(3, 0, 0)] {
            ir.cells.push(PlacedCellNode {
                cell: EditionCell::JavaRepeaterOr,
                drivers: vec![CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                }],
                coord,
                phase: PlacementPhase::Unrouted,
                span: Span::default(),
            });
        }

        let routed = compile_routing(&scoped(ScopeKind::Struct, "boxed", ir));
        let refusal = routed
            .diagnostics
            .iter()
            .find(|d| d.code == crate::DiagnosticCode::RouteCongestion)
            .unwrap_or_else(|| panic!("two walled-in sinks must refuse: {:?}", routed.diagnostics));
        assert!(
            refusal.primary.contains("cannot reach (2,0,0)"),
            "the first sink in net order is the anchor: {}",
            refusal.primary,
        );
        assert!(
            refusal
                .primary
                .contains("1 more of this scope's sinks cannot be reached either"),
            "and the rest are counted rather than left for the next run: {}",
            refusal.primary,
        );
    }

    /// A sink the reservation cannot reach is refused by every pass
    /// that would measure it, naming the same two coords.
    ///
    /// Stage 2 elides the scope, so stages 3 and 4 never see one in a
    /// real run. They rebuild the trees from the IR, though, and a
    /// caller who skipped stage 2 would hand them one. The tree answers
    /// for a stranded sink — one step, straight to the source — so
    /// without the check the later stages would not panic; they would
    /// write a tick count and a buffer list for a circuit that cannot
    /// be wired, and nothing in the dump would say so.
    #[test]
    fn an_unreachable_sink_is_refused_by_every_pass_that_measures_it() {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(3, 2, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        // The pad lands at (0,0,0). Walling (1,0,0) and (2,0,1) in
        // leaves the sink at (2,0,0) with no free neighbour, and
        // `void=1` reserves no layer to come in over the top.
        for coord in [CellCoord::new(1, 0, 0), CellCoord::new(2, 0, 1)] {
            ir.cells.push(placed_cell(coord, PlacementPhase::Unrouted));
        }
        ir.cells.push(PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers: vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            coord: CellCoord::new(2, 0, 0),
            phase: PlacementPhase::Unrouted,
            span: Span::default(),
        });

        let routed = compile_routing(&scoped(ScopeKind::Struct, "boxed", ir.clone()));
        let refusal = routed
            .diagnostics
            .iter()
            .find(|d| d.code == crate::DiagnosticCode::RouteCongestion)
            .unwrap_or_else(|| panic!("a walled-in sink must refuse: {:?}", routed.diagnostics));
        assert!(
            refusal.primary.contains("cannot reach (2,0,0)")
                && refusal.primary.contains("from the driver at (0,0,0)"),
            "the refusal names both ends: {}",
            refusal.primary,
        );
        assert!(
            refusal
                .primary
                .contains("a wire passes through none of the three"),
            "the refusal says why a route cannot be found, not only that one \
             was not: {}",
            refusal.primary,
        );
        assert!(
            routed.scoped.scopes.is_empty(),
            "the failed scope is elided rather than half-attributed",
        );

        // The same layout handed straight to stage 3, and then to
        // stage 4, as a caller who skipped stage 2 would. Each rebuilds
        // the trees, and each has to reach the same verdict: the
        // alternative is a dump whose ticks and buffer coords describe
        // a circuit nothing can build.
        for cell in &mut ir.cells {
            cell.phase = PlacementPhase::Routed { wire_length: 0 };
        }
        let delayed = crate::delay::compile_delay(&scoped(ScopeKind::Struct, "boxed", ir.clone()));
        let delay_refusal = delayed
            .diagnostics
            .iter()
            .find(|d| d.code == crate::DiagnosticCode::RouteCongestion)
            .unwrap_or_else(|| panic!("stage 3 must refuse too: {:?}", delayed.diagnostics));
        assert_eq!(delay_refusal.primary, refusal.primary);
        assert!(delayed.scoped.scopes.is_empty());

        for cell in &mut ir.cells {
            cell.phase = PlacementPhase::Delayed {
                wire_length: 0,
                delay_ticks: 0,
            };
        }
        let legalized = crate::crossing::compile_crossing(&scoped(ScopeKind::Struct, "boxed", ir));
        let crossing_refusal = legalized
            .diagnostics
            .iter()
            .find(|d| d.code == crate::DiagnosticCode::RouteCongestion)
            .unwrap_or_else(|| panic!("stage 4 must refuse too: {:?}", legalized.diagnostics));
        assert_eq!(crossing_refusal.primary, refusal.primary);
        assert!(legalized.scoped.scopes.is_empty());
    }

    #[test]
    fn wire_length_counts_the_routed_path() {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(16, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            CellCoord::new(7, 0, 0),
            PlacementPhase::Unrouted,
        ));
        ir.cells.push(PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers: vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            coord: CellCoord::new(14, 0, 0),
            phase: PlacementPhase::Unrouted,
            span: Span::default(),
        });
        let routed = compile_routing(&scoped(ScopeKind::Struct, "walled", ir));
        assert!(routed.diagnostics.is_empty(), "{:?}", routed.diagnostics);
        let lengths: Vec<Option<u32>> = routed.scoped.scopes[0]
            .ir
            .cells
            .iter()
            .map(PlacedCellNode::wire_length)
            .collect();
        assert_eq!(
            lengths,
            vec![Some(0), Some(16)],
            "the sink is 14 blocks away and 16 blocks of wire from its driver",
        );
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
