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
//! until the delay-insertion pass (stage 3 of §14.5) lands.
//!
//! The v1 algorithm is deliberately minimal:
//!
//! - **Net collection.** Each cell driver, each output driver, and
//!   every unused input becomes a `(source_coord, [sink_coord…])`
//!   entry. Source coordinates are `NetRef::Input(i) →
//!   input_pad(i, region)` (left edge, z = 1 + i, saturating at
//!   `depth-1` for degenerate regions) and `NetRef::Cell(j) →
//!   cells[j].coord`. Output pad coordinates are the right edge,
//!   z = 1 + k, saturating similarly.
//! - **Steiner tree.** Kou-Markowsky-style rectilinear minimum
//!   spanning tree over the `{source} ∪ sinks` terminal set — Kruskal
//!   with union-find on the complete Manhattan graph, then each MST
//!   edge is rendered as an L-shape (x-then-z-then-y, deterministic
//!   for regression stability) into the occupancy set.
//! - **Occupancy.** A per-scope `HashSet<CellCoord>` seeded with
//!   every cell coord, every input pad, and every output pad, then
//!   grown by each drawn L-shape. Duplicate visits share (Steiner
//!   fanout is the whole point). Cross-net overlap is tolerated in
//!   v1; the crossing-legalization pass (stage 4 of §14.5) is what
//!   promotes those to a `RouteLayer::Bridge` / `Via` escape in a
//!   later PR.
//! - **`wire_length` attribution.** For every cell, `wire_length =
//!   sum over drivers of Manhattan(driver-source-coord, cell.coord)`.
//!   The tree-total path is not attributed per-sink today because the
//!   downstream delay-insertion pass (stage 3, §14.4) uses the sum of
//!   Manhattan distances as its input, and re-tree-walking here would
//!   double the work without shifting the delay decision.
//! - **Congestion.** After every net is laid, `occupancy.len() +
//!   cell_footprint > reserved_area` fires `E_ROUTE_CONGESTION`
//!   against the reservation span. The primary message differs from
//!   the placement-pass version so a downstream reader can tell whether
//!   the pessimistic cell-only budget or the actual routed layout was
//!   the trigger. Failed scopes are elided from the output list — the
//!   same fail-loud policy [`crate::placement::compile_placement`]
//!   applies to placement failures.
//!
//! Future stages fill the intentional gaps: attenuation-limit
//! (`E_ATTENUATION_LIMIT`, dust segments > 15) belongs to delay
//! insertion, `RouteLayer::Bridge` / `Via` escape belongs to crossing
//! legalization, and the input / output pad coordinates the routing
//! pass picks today become a `PlacementIr` field
//! (`input_pads` / `output_pads`) once a subsequent PR needs them
//! outside routing — that migration is `#[non_exhaustive]`-safe on
//! both types.

use std::collections::HashMap;
use std::collections::HashSet;

use cairn_lang_core::check::Severity;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIr, ScopedPlacementIrEntry,
};

/// Per-cell footprint used by the post-routing congestion budget. Kept
/// in sync with [`crate::placement::CELL_FOOTPRINT`] so a scope that
/// placed at the cell-only budget boundary needs at most one Manhattan
/// segment of new wire to flip to `E_ROUTE_CONGESTION` at this stage —
/// i.e. the routing pass carries the same footprint model the
/// placement pass used, and adds wire occupancy on top.
pub const CELL_FOOTPRINT: u32 = crate::placement::CELL_FOOTPRINT;

/// Output of a [`compile_routing`] run.
///
/// Mirrors the shape of [`crate::placement::PlacementOutput`] so
/// callers see a uniform result type across every stage of the
/// place-and-route pipeline. The routed IR is a
/// [`ScopedPlacementIr`] with every non-failed scope's
/// `wire_length` promoted from `None` to `Some(_)` — no new IR type;
/// the routing pass is a field write per the phase table on
/// [`crate::placement_ir::PlacedCellNode`].
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
/// re-consulting the Intent IR: the Placement IR is self-describing
/// by the M6-PR4 contract, so the routing pass has no `IntentModule`
/// dependency.
///
/// One entry per non-empty [`PlacementIr`] whose routing succeeded;
/// scopes whose routing raises an Error-severity diagnostic (today,
/// only `E_ROUTE_CONGESTION`) are elided from the output so
/// downstream passes cannot silently accept a partial layout.
#[must_use]
pub fn compile_routing(placement: &ScopedPlacementIr) -> RoutingOutput {
    let mut out = RoutingOutput::new();
    for entry in &placement.scopes {
        match route_scope(&entry.ir) {
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

fn route_scope(source: &PlacementIr) -> ScopeRouting {
    // Defensive pass-through: a scope with no cells cannot be laid
    // out further than placement already did. `ScopedPlacementIr::push`
    // elides these on the input side (M6-PR4 invariant), so the
    // branch is a belt-and-braces for hand-built IRs.
    if source.cells.is_empty() {
        return Ok(source.clone());
    }
    let Some(region) = source.region.clone() else {
        // Same rationale: placement fires `E_NO_CIRCUIT_REGION` and
        // elides the scope before it can reach the routing pass, so
        // this branch is defensive for hand-built IRs.
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
                // in `cells[i]` satisfies `j < i`). A clamp keeps this
                // defensive against hand-built IRs.
                cell_coords
                    .get(j as usize)
                    .copied()
                    .unwrap_or(CellCoord { x: 0, y: 0, z: 0 })
            }
        }
    };

    // Collect nets: source → sink list. `HashMap` is deterministic
    // because we sort keys before iterating.
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

    // Occupancy seed: every cell coord, every input pad, every output
    // pad. Cell footprint is applied as an area budget below (not
    // per-block) because the placement pass already staked out cells
    // pessimistically at 4 blocks each.
    let mut occupancy: HashSet<CellCoord> = HashSet::with_capacity(ir.cells.len() * 4);
    for coord in &cell_coords {
        occupancy.insert(*coord);
    }
    for i in 0..ir.inputs.len() {
        occupancy.insert(input_pad(i, &region));
    }
    for k in 0..ir.outputs.len() {
        occupancy.insert(output_pad(k, &region));
    }

    // Process nets in a deterministic order: fanout descending, tie by
    // NetRef key ascending. Higher-fanout nets should stake claim on
    // the shortest L-shape first so a lower-fanout net does not steer
    // the shared segments through a longer L. Deterministic tie-break
    // pins the regression story.
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
        route_net(source_coord, sinks, &mut occupancy);
    }

    // Attribute wire_length per cell: sum of Manhattan(driver source,
    // cell coord) over drivers. The Steiner-shared tree total is used
    // for congestion accounting above; per-sink Manhattan is what the
    // delay-insertion pass (§14.4) will consume, so attributing it
    // here saves a second walk in the follow-up PR. Compute into a
    // side vector first so `ir.cells` can be borrowed immutably while
    // the driver sources are looked up, then commit in a mutable
    // pass.
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
    for (cell, len) in ir.cells.iter_mut().zip(wire_lengths) {
        cell.wire_length = Some(len);
    }

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
        return Err(congestion_diagnostic(&region, used));
    }

    Ok(ir)
}

fn net_ref_key(net: NetRef) -> (u8, u32) {
    match net {
        NetRef::Input(i) => (0, i),
        NetRef::Cell(j) => (1, j),
    }
}

/// v1 input-pad coordinate: left edge (`x=0`), first service layer
/// (`y=0`), z-axis increasing as the input index grows. Saturates at
/// `depth-1` for degenerate `depth=1` regions — those regions cannot
/// fit even one input pad past the cell row without collision, and
/// the resulting overlap surfaces as `E_ROUTE_CONGESTION` downstream
/// rather than a silent misroute. Pinning the coordinate here is a v1
/// convention; once a subsequent PR needs pad coords outside routing,
/// `input_pads` joins [`PlacementIr`] as a `#[non_exhaustive]`-safe
/// field.
fn input_pad(i: usize, region: &CircuitRegionReservation) -> CellCoord {
    let raw = u32::try_from(i.saturating_add(1)).unwrap_or(u32::MAX);
    let z = raw.min(region.depth.saturating_sub(1));
    CellCoord { x: 0, y: 0, z }
}

/// v1 output-pad coordinate: right edge (`x=width-1`), same saturating
/// z-axis convention as [`input_pad`].
fn output_pad(k: usize, region: &CircuitRegionReservation) -> CellCoord {
    let raw = u32::try_from(k.saturating_add(1)).unwrap_or(u32::MAX);
    let z = raw.min(region.depth.saturating_sub(1));
    let x = region.width.saturating_sub(1);
    CellCoord { x, y: 0, z }
}

fn manhattan(a: CellCoord, b: CellCoord) -> u32 {
    let dx = a.x.max(b.x) - a.x.min(b.x);
    let dy = a.y.max(b.y) - a.y.min(b.y);
    let dz = a.z.max(b.z) - a.z.min(b.z);
    dx.saturating_add(dy).saturating_add(dz)
}

fn route_net(source: CellCoord, sinks: &[CellCoord], occupancy: &mut HashSet<CellCoord>) {
    // Terminal set = source ∪ sinks, deduplicated so a fanout net
    // whose sinks include the source coordinate (a degenerate hand-built
    // case) still produces a well-formed MST.
    let mut terminals: Vec<CellCoord> = Vec::with_capacity(1 + sinks.len());
    terminals.push(source);
    for s in sinks {
        if !terminals.contains(s) {
            terminals.push(*s);
        }
    }
    if terminals.len() < 2 {
        occupancy.insert(source);
        return;
    }

    // Complete-graph edge list, sorted by (weight, i, j) for
    // deterministic MST regardless of HashSet iteration order.
    let n = terminals.len();
    let mut edges: Vec<(u32, usize, usize)> = Vec::with_capacity(n * (n - 1) / 2);
    for i in 0..n {
        for j in (i + 1)..n {
            edges.push((manhattan(terminals[i], terminals[j]), i, j));
        }
    }
    edges.sort_unstable();

    let mut parent: Vec<usize> = (0..n).collect();
    for (_, i, j) in edges {
        let ri = union_find(&mut parent, i);
        let rj = union_find(&mut parent, j);
        if ri == rj {
            continue;
        }
        parent[ri] = rj;
        draw_l_shape(terminals[i], terminals[j], occupancy);
    }
}

fn union_find(parent: &mut [usize], x: usize) -> usize {
    let mut root = x;
    while parent[root] != root {
        root = parent[root];
    }
    let mut cur = x;
    while parent[cur] != root {
        let next = parent[cur];
        parent[cur] = root;
        cur = next;
    }
    root
}

fn draw_l_shape(a: CellCoord, b: CellCoord, occupancy: &mut HashSet<CellCoord>) {
    // Deterministic axis order: x, then z, then y. The routing pass's
    // regression story pins on this order — a follow-up that picks the
    // less-congested elbow per net can only firm this up because both
    // L-shapes have identical Manhattan length by construction.
    let mut cur = a;
    occupancy.insert(cur);
    while cur.x != b.x {
        cur.x = if cur.x < b.x { cur.x + 1 } else { cur.x - 1 };
        occupancy.insert(cur);
    }
    while cur.z != b.z {
        cur.z = if cur.z < b.z { cur.z + 1 } else { cur.z - 1 };
        occupancy.insert(cur);
    }
    while cur.y != b.y {
        cur.y = if cur.y < b.y { cur.y + 1 } else { cur.y - 1 };
        occupancy.insert(cur);
    }
}

fn congestion_diagnostic(reservation: &CircuitRegionReservation, used: u64) -> Diagnostic {
    let reserved = reservation.reserved_area();
    // `reserved_area > 0` by construction — the placement pass
    // already screens `width=0` / `depth=0` / `void=0` reservations
    // out via `E_NO_CIRCUIT_REGION` before this pass runs.
    debug_assert!(
        reserved > 0,
        "reservation.reserved_area() must be > 0 to compare against routed occupancy",
    );
    let ratio_x10 = (used.saturating_mul(10)) / reserved;
    let whole = ratio_x10 / 10;
    let tenths = ratio_x10 % 10;
    let primary = format!(
        "routed netlist occupies ~{whole}.{tenths}x the reserved area (void={void}, region {width}x{depth})",
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
    debug_assert_eq!(diag.severity, Severity::Error);
    diag
}
