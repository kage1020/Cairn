//! Delayed Placement IR → legalized Placement IR lowering (crossing
//! legalization).
//!
//! Stage 4 of the five-stage place-and-route pipeline `spec/redstone`
//! §14.5 lays out (Placement → Steiner routing → Delay insertion →
//! Crossing legalization → Edition legalization). Walks every
//! [`crate::placement_ir::PlacedCellNode`] in each scope's delayed
//! Placement IR, re-derives every net's Manhattan Steiner tree from
//! the same `NetRef → source coord` mapping the routing and delay
//! passes use (the routing pass discards its per-scope occupancy set
//! before yielding the routed IR, and storing wire coords in the
//! shared IR would bloat every JSON dump for every consumer), and
//! carries out two tasks:
//!
//! 1. **Plane crossing detection.** A wire coord (neither cell nor
//!    pad) that ends up owned by two distinct nets is a "crossing"
//!    that would short in the Minecraft voxel model. v1 does not lift
//!    wire coords themselves onto a `Bridge` layer — the routed wire
//!    path is not stored in the IR, so an escape record would have
//!    nowhere to attach. Instead, a scope with any plane crossing
//!    against a `void < 2` reservation is refused with
//!    [`crate::DiagnosticCode::CrossingCongestion`]; `void >= 2`
//!    scopes are accepted on the grounds that the reserved service
//!    layers are wide enough for a `stage 5` block-array lowering to
//!    re-derive the same Steiner trees and lift the crossings itself.
//!    The set of crossing coords is only used inside this pass to
//!    steer buffer placement (below) — it is not surfaced on the IR.
//! 2. **Implicit buffer repeater coord assignment.** The delay pass
//!    counted `floor((s - 1) / DUST_ATTENUATION_LIMIT)` buffer
//!    repeaters per driver segment of length `s` and folded their tick
//!    contribution into `delay_ticks`; this pass materialises the
//!    concrete coord of each one into
//!    [`crate::placement_ir::PlacedCellNode::buffer_coords`]. Each
//!    driver's Manhattan L-shape is walked in the routing pass's
//!    axis order (x → z → y) and coords are picked at
//!    `k * DUST_ATTENUATION_LIMIT` (`k = 1..=buffer_count`). A
//!    candidate that collides with a cell coord, pad coord, plane
//!    crossing, or earlier buffer escapes to the first free y-layer
//!    inside the `void=<N>` budget on
//!    [`crate::placement_ir::RouteLayer::Bridge`]; if every layer is
//!    taken (or `void < 2`), the pass refuses with
//!    [`crate::DiagnosticCode::BufferCoordCollision`].
//!
//! [`crate::placement_ir::RouteLayer::Via`] has no producer in v1: the
//! bridge escape is a single coord, not a segment with distinct
//! plane / bridge endpoints, so there is no ramp to name. The variant
//! is kept in the enum for exhaustive matches against §14.5's full
//! vocabulary.
//!
//! Failed scopes are elided from the output so a `stage 5` consumer
//! never reads a partially-populated `buffer_coords` — the same
//! fail-loud policy the routing and delay passes use.
//!
//! The crossing pass is a field write on
//! [`crate::placement_ir::PlacedCellNode::buffer_coords`] and
//! [`crate::placement_ir::CellCoord::layer`] per the phase table on
//! `PlacedCellNode`; no new IR type is introduced. Both fields
//! serde-skip on their defaults, so a scope whose crossing pass
//! writes nothing dumps as the identical JSON its delay-pass input
//! did.

use std::collections::HashMap;
use std::collections::HashSet;

use cairn_lang_core::check::Severity;

use crate::delay::DUST_ATTENUATION_LIMIT;
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, RouteLayer, ScopedPlacementIr,
    ScopedPlacementIrEntry,
};
use crate::routing::{input_pad, manhattan, output_pad};

/// Output of a [`compile_crossing`] run.
///
/// Mirrors [`crate::delay::DelayOutput`]'s shape so callers see a
/// uniform result type across every stage of the place-and-route
/// pipeline. The legalized IR is a [`ScopedPlacementIr`] with every
/// non-failed scope's `buffer_coords` populated with one entry per
/// implicit buffer repeater the delay pass counted, each tagged with
/// the [`RouteLayer`] the pass chose. No new IR type; the crossing
/// pass is a field write per the phase table on
/// [`crate::placement_ir::PlacedCellNode`].
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct CrossingOutput {
    /// Placement IR for every scope whose crossing legalization
    /// succeeded, with every cell's `buffer_coords` populated to match
    /// the buffer tick contribution the delay pass folded into
    /// `delay_ticks`.
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
/// succeeded; scopes that raise an Error-severity diagnostic
/// ([`DiagnosticCode::CrossingCongestion`] /
/// [`DiagnosticCode::BufferCoordCollision`] /
/// [`DiagnosticCode::NoCircuitRegion`]) are elided from the output so a
/// partial `buffer_coords` set cannot pollute the downstream
/// block-array voxel lowering.
#[must_use]
pub fn compile_crossing(delayed: &ScopedPlacementIr) -> CrossingOutput {
    let mut out = CrossingOutput::new();
    for entry in &delayed.scopes {
        match legalize_scope(entry) {
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

/// Result of legalizing one scope: the legalized IR on success, a
/// single Error-severity diagnostic on failure.
type ScopeLegalization = Result<PlacementIr, Diagnostic>;

fn legalize_scope(entry: &ScopedPlacementIrEntry) -> ScopeLegalization {
    let source = &entry.ir;
    // Same missing-region policy as `delay::compile_delay`: the
    // placement pass elides scopes with cells or output drivers but no
    // region, so a hand-built IR reaching here in that shape is a
    // caller-side bug — refuse loud with `E_NO_CIRCUIT_REGION` so
    // downstream consumers see a consistent taxonomy. Scopes with
    // neither cells nor outputs still pass through so a module
    // without any redstone survives the crossing pipeline as-is.
    let Some(region) = source.region.clone() else {
        if source.cells.is_empty() && source.outputs.is_empty() {
            return Ok(source.clone());
        }
        return Err(missing_region_diagnostic(entry));
    };
    if source.cells.is_empty() && source.outputs.is_empty() {
        return Ok(source.clone());
    }

    let mut ir = source.clone();
    let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();

    // Netlist synthesis guarantees the topological invariant
    // (`NetRef::Cell(j)` inside `cells[i]` satisfies `j < i`), so
    // panicking loud on an out-of-range access beats silently
    // sinking into a fall-back coord. Same reasoning as the delay
    // pass's stricter branch — this pass writes `buffer_coords`, and
    // the phase table on `PlacedCellNode` promises populated
    // `buffer_coords` after stage 4, so a silent under-population
    // would let the downstream voxel lowering read a stage-3 shape
    // from a stage-4 output.
    let source_of_net = |net: NetRef| -> CellCoord {
        match net {
            NetRef::Input(i) => input_pad(i as usize, &region),
            NetRef::Cell(j) => *cell_coords.get(j as usize).unwrap_or_else(|| {
                panic!(
                    "NetRef::Cell({j}) out of range (cells.len()={}) — topological invariant broken by caller-side hand-built IR",
                    cell_coords.len(),
                )
            }),
        }
    };

    let nets = collect_nets(&ir, &region);
    let mut net_order: Vec<NetRef> = nets.keys().copied().collect();
    net_order.sort_by(|a, b| {
        let fa = nets[a].len();
        let fb = nets[b].len();
        fb.cmp(&fa)
            .then_with(|| net_ref_key(*a).cmp(&net_ref_key(*b)))
    });

    // Reserved coords are cell bodies and I/O pads — they carry the
    // net's endpoint semantic, not a wire pass-through, so two nets
    // touching the same reserved coord is not a crossing (that is
    // the standard cell → downstream cell chain). Only pure wire
    // coords participate in crossing detection.
    let mut reserved: HashSet<CellCoord> = HashSet::new();
    for coord in &cell_coords {
        reserved.insert(*coord);
    }
    for i in 0..ir.inputs.len() {
        reserved.insert(input_pad(i, &region));
    }
    for k in 0..ir.outputs.len() {
        reserved.insert(output_pad(k, &region));
    }

    // Per-net wire path derived from the same MST + L-shape draw the
    // routing pass uses. Kept as a `Vec<CellCoord>` (not a `HashSet`)
    // so buffer coord allocation below can walk driver-source-to-sink
    // L-shapes in the same axis order.
    let mut wire_paths: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    for net in &net_order {
        let src = source_of_net(*net);
        let sinks = &nets[net];
        wire_paths.insert(*net, net_wire_path(src, sinks));
    }

    // Detect plane crossings: two distinct nets sharing a
    // non-reserved coord. Iteration order is `net_order` so the
    // "first owner wins" choice and the identity of the "second net"
    // recorded at each crossing coord are both deterministic across
    // runs. The `(first, second)` pair is preserved so the crossing
    // diagnostic can name the two nets responsible.
    let mut plane_owners: HashMap<CellCoord, NetRef> = HashMap::new();
    let mut crossings: HashMap<CellCoord, (NetRef, NetRef)> = HashMap::new();
    for net in &net_order {
        for coord in &wire_paths[net] {
            if reserved.contains(coord) {
                continue;
            }
            match plane_owners.get(coord).copied() {
                None => {
                    plane_owners.insert(*coord, *net);
                }
                Some(existing) if existing == *net => {
                    // Same net's Steiner fanout — normal.
                }
                Some(first) => {
                    // First-crossing pair wins; a third net hitting
                    // the same coord does not overwrite. Sufficient
                    // for the v1 diagnostic which anchors on one
                    // coord anyway.
                    crossings.entry(*coord).or_insert((first, *net));
                }
            }
        }
    }

    // A bridge escape needs at least one y-layer above the plane
    // (`y = 1`), which requires `void >= 2`. Refuse loud so the
    // caller is redirected to `void=<N>` rather than seeing a silent
    // plane short.
    if !crossings.is_empty() && region.void < 2 {
        // Deterministic pick: smallest crossing coord by (x, z) so
        // the diagnostic anchor does not drift across runs.
        let mut anchors: Vec<(CellCoord, (NetRef, NetRef))> =
            crossings.iter().map(|(c, owners)| (*c, *owners)).collect();
        anchors.sort_unstable_by_key(|(c, _)| (c.x, c.z, c.y));
        let (anchor, anchor_owners) = anchors[0];
        return Err(crossing_congestion_diagnostic(
            entry,
            &ir,
            &region,
            anchor,
            anchor_owners,
            crossings.len(),
        ));
    }

    let buffer_coords_per_cell = allocate_buffer_coords(
        &ir,
        entry,
        &region,
        &cell_coords,
        &crossings,
        &reserved,
        &source_of_net,
    )?;

    for (cell, buffers) in ir.cells.iter_mut().zip(buffer_coords_per_cell) {
        // Loud in release too: the phase table on `PlacedCellNode`
        // forbids a re-run of stage 4, and silently overwriting a
        // populated `buffer_coords` would let a caller who chained
        // `compile_crossing(&legalized.scoped)` produce a
        // stale-but-plausible IR.
        assert!(
            cell.buffer_coords.is_empty(),
            "legalize_scope re-writing a PlacedCellNode whose buffer_coords is already {} entries — crossing legalization must run at most once per delayed IR",
            cell.buffer_coords.len(),
        );
        cell.buffer_coords = buffers;
    }

    Ok(ir)
}

/// Buffer coord allocation: for every cell driver segment, walk the
/// source-to-sink L-shape and pick coords at
/// `k * DUST_ATTENUATION_LIMIT` (`k = 1..=buffer_count`). A collision
/// with a reserved coord, a plane crossing, or another buffer already
/// placed on the plane escapes to the first free `RouteLayer::Bridge`
/// y-layer inside the reservation's `void=<N>` budget (`y in
/// 1..void`); if every bridge y-layer at the candidate `(x, z)` is
/// taken (or `void < 2` so no bridge layer exists at all), refuse
/// with `E_BUFFER_COORD_COLLISION`. Split out of `legalize_scope` so
/// the entry function stays under clippy's `too_many_lines` budget
/// and the allocation strategy reads as a self-contained table.
fn allocate_buffer_coords<F>(
    ir: &PlacementIr,
    entry: &ScopedPlacementIrEntry,
    region: &CircuitRegionReservation,
    cell_coords: &[CellCoord],
    crossings: &HashMap<CellCoord, (NetRef, NetRef)>,
    reserved: &HashSet<CellCoord>,
    source_of_net: &F,
) -> Result<Vec<Vec<CellCoord>>, Diagnostic>
where
    F: Fn(NetRef) -> CellCoord,
{
    let mut plane_buffers: HashSet<CellCoord> = HashSet::new();
    let mut bridge_buffers: HashSet<CellCoord> = HashSet::new();
    let mut per_cell: Vec<Vec<CellCoord>> = Vec::with_capacity(ir.cells.len());
    for (cell_index, cell) in ir.cells.iter().enumerate() {
        let sink = cell_coords[cell_index];
        let mut buffers_for_cell: Vec<CellCoord> = Vec::new();
        for driver in &cell.drivers {
            let src = source_of_net(driver.net);
            let path = l_shape_path(src, sink);
            let segment = manhattan(src, sink);
            let buffer_count = if segment <= DUST_ATTENUATION_LIMIT {
                0
            } else {
                (segment - 1) / DUST_ATTENUATION_LIMIT
            };
            for k in 1..=buffer_count {
                // `path.len() == segment + 1` and `k * DUST_ATTENUATION_LIMIT
                // <= buffer_count * DUST_ATTENUATION_LIMIT <= segment - 1`,
                // so `idx` is always a valid path index. Loud in
                // release: a silent saturating fallback would place
                // the buffer at the sink coord and let a caller-side
                // bug (segment / buffer_count / path.len() drift)
                // materialise a buffer at the cell body without any
                // diagnostic.
                let idx = (k as usize).saturating_mul(DUST_ATTENUATION_LIMIT as usize);
                let candidate = *path.get(idx).unwrap_or_else(|| {
                    panic!(
                        "buffer index {idx} out of range (path.len()={}) for cell #{cell_index} driver — segment / buffer_count / path.len() invariant broken by caller-side hand-built IR",
                        path.len(),
                    )
                });
                let plane_taken = reserved.contains(&candidate)
                    || crossings.contains_key(&candidate)
                    || plane_buffers.contains(&candidate);
                if !plane_taken {
                    plane_buffers.insert(candidate);
                    buffers_for_cell.push(candidate);
                    continue;
                }
                let mut escaped = None;
                for y in 1..region.void {
                    let bridge_candidate =
                        CellCoord::with_layer(candidate.x, y, candidate.z, RouteLayer::Bridge);
                    if !bridge_buffers.contains(&bridge_candidate) {
                        bridge_buffers.insert(bridge_candidate);
                        escaped = Some(bridge_candidate);
                        break;
                    }
                }
                match escaped {
                    Some(bridge) => buffers_for_cell.push(bridge),
                    None => {
                        return Err(buffer_collision_diagnostic(
                            entry, region, cell_index, candidate,
                        ));
                    }
                }
            }
        }
        per_cell.push(buffers_for_cell);
    }
    Ok(per_cell)
}

/// Collect nets from the placed IR: source (implicit via [`NetRef`]) →
/// list of sink coords (cell coords for cell-driver sinks, output pad
/// coords for actuator sinks). Same shape the routing pass uses; kept
/// crate-local so the crossing pass does not depend on a routing-side
/// helper whose signature might drift.
fn collect_nets(
    ir: &PlacementIr,
    region: &CircuitRegionReservation,
) -> HashMap<NetRef, Vec<CellCoord>> {
    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    for cell in &ir.cells {
        let sink = cell.coord;
        for driver in &cell.drivers {
            nets.entry(driver.net).or_default().push(sink);
        }
    }
    for (k, output) in ir.outputs.iter().enumerate() {
        let sink = output_pad(k, region);
        nets.entry(output.driver).or_default().push(sink);
    }
    nets
}

/// Deterministic net-order key matching the routing pass's tie-break:
/// `Input(_)` sorts before `Cell(_)`, then by index ascending. Kept
/// crate-local so both passes can be understood in isolation.
fn net_ref_key(net: NetRef) -> (u8, u32) {
    match net {
        NetRef::Input(i) => (0, i),
        NetRef::Cell(j) => (1, j),
    }
}

/// Manhattan Steiner tree over `{source} ∪ sinks`, rendered as the
/// concatenation of every MST edge's L-shape path. Kruskal on the
/// complete Manhattan graph with `(weight, i, j)` tie-break gives a
/// deterministic MST regardless of `HashMap` iteration order.
/// Matches [`crate::routing::compile_routing`]'s per-net algorithm so
/// the two passes see the same wire path for the same input; the
/// helper is duplicated here rather than shared because merging the
/// two would demand a common crate module, and the current copies
/// are read-only from the routing side.
fn net_wire_path(source: CellCoord, sinks: &[CellCoord]) -> Vec<CellCoord> {
    let mut terminals: Vec<CellCoord> = Vec::with_capacity(1 + sinks.len());
    terminals.push(source);
    for s in sinks {
        if !terminals.contains(s) {
            terminals.push(*s);
        }
    }
    if terminals.len() < 2 {
        return vec![source];
    }
    let n = terminals.len();
    let mut edges: Vec<(u32, usize, usize)> = Vec::with_capacity(n * (n - 1) / 2);
    for i in 0..n {
        for j in (i + 1)..n {
            edges.push((manhattan(terminals[i], terminals[j]), i, j));
        }
    }
    edges.sort_unstable();

    let mut parent: Vec<usize> = (0..n).collect();
    let mut path: Vec<CellCoord> = Vec::new();
    for (_, i, j) in edges {
        let ri = union_find(&mut parent, i);
        let rj = union_find(&mut parent, j);
        if ri == rj {
            continue;
        }
        parent[ri] = rj;
        path.extend(l_shape_path(terminals[i], terminals[j]));
    }
    path
}

/// Iterative path-compressed union-find over an index-keyed forest.
/// Used by [`net_wire_path`] for Kruskal MST.
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

/// One L-shape between two coords, returned as the ordered coord
/// sequence including both endpoints. Axis order is x-then-z-then-y,
/// matching [`crate::routing::compile_routing`]'s draw so both passes
/// see the same wire coords for the same terminal pair. Both L-shape
/// orderings have identical Manhattan length; the axis order fixes a
/// canonical choice. Result length equals `manhattan(a, b) + 1`.
fn l_shape_path(a: CellCoord, b: CellCoord) -> Vec<CellCoord> {
    let mut path = Vec::with_capacity((manhattan(a, b) as usize).saturating_add(1));
    let mut cur = a;
    path.push(cur);
    while cur.x != b.x {
        cur.x = if cur.x < b.x { cur.x + 1 } else { cur.x - 1 };
        path.push(cur);
    }
    while cur.z != b.z {
        cur.z = if cur.z < b.z { cur.z + 1 } else { cur.z - 1 };
        path.push(cur);
    }
    while cur.y != b.y {
        cur.y = if cur.y < b.y { cur.y + 1 } else { cur.y - 1 };
        path.push(cur);
    }
    path
}

fn crossing_congestion_diagnostic(
    entry: &ScopedPlacementIrEntry,
    ir: &PlacementIr,
    reservation: &CircuitRegionReservation,
    anchor: CellCoord,
    anchor_owners: (NetRef, NetRef),
    crossing_count: usize,
) -> Diagnostic {
    let (first, second) = anchor_owners;
    let primary = format!(
        "routed netlist for {kind} `{name}` has {crossing_count} plane crossing(s), including {first_label} vs {second_label} at ({x},{y},{z}) — but the `void={void}` reservation offers no bridge layer to escape to (bridges need at least y=1, which requires void>=2)",
        kind = entry.kind.label(),
        name = entry.name,
        first_label = net_label(first, ir),
        second_label = net_label(second, ir),
        x = anchor.x,
        y = anchor.y,
        z = anchor.z,
        void = reservation.void,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::CrossingCongestion,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: increase `void` so bridges have a y-layer above the plane, enlarge `region=` so fewer wires cross, or split the logic across multiple `circuit` blocks",
    );
    debug_assert_eq!(diag.severity, Severity::Error);
    diag
}

/// Human-facing label for a [`NetRef`] used inside diagnostic prose.
/// Resolves `Input(i)` to the sensor's dotted `sig.<name>` when the
/// scope carries one at that index, falling back to the raw `input pad
/// #i` form for a hand-built IR whose input row is shorter than the
/// synthesis path implies. Cell drivers surface as `cell #j` because
/// the Netlist IR does not carry a source-level name for a synthesised
/// gate.
fn net_label(net: NetRef, ir: &PlacementIr) -> String {
    match net {
        NetRef::Input(i) => ir
            .inputs
            .get(i as usize)
            .map_or_else(|| format!("input pad #{i}"), |input| input.name.to_string()),
        NetRef::Cell(j) => format!("cell #{j}"),
    }
}

fn buffer_collision_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    cell_index: usize,
    candidate: CellCoord,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` cannot place an implicit buffer repeater for cell #{cell_index}: candidate coord ({x},{y},{z}) is taken on the plane and the `void={void}` reservation offers no bridge layer to escape to",
        kind = entry.kind.label(),
        name = entry.name,
        x = candidate.x,
        y = candidate.y,
        z = candidate.z,
        void = reservation.void,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::BufferCoordCollision,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: increase `void` so buffers can fall onto a bridge layer, or enlarge `region=` so buffer candidates have room on the plane",
    );
    debug_assert_eq!(diag.severity, Severity::Error);
    diag
}

fn missing_region_diagnostic(entry: &ScopedPlacementIrEntry) -> Diagnostic {
    let span = entry
        .ir
        .cells
        .first()
        .map(|c| c.span.clone())
        .unwrap_or_default();
    let primary = format!(
        "delayed netlist for {kind} `{name}` reached crossing legalization carrying cells or output drivers but no `circuit region=<label> void=<N>` reservation — the placement pass should have elided this scope",
        kind = entry.kind.label(),
        name = entry.name,
    );
    let mut diag = Diagnostic::new(DiagnosticCode::NoCircuitRegion, span, primary);
    diag = diag.with_footer(
        "Fix: add a `circuit region=<label> void=<N>` line to the enclosing scope, or run `--stage placement` first to see the underlying error",
    );
    debug_assert_eq!(diag.severity, Severity::Error);
    diag
}

#[cfg(test)]
mod tests {
    //! Crate-internal unit tests for crossing-legalization behaviours
    //! that `tests/crossing.rs` cannot reach through synth fixtures
    //! alone:
    //! - the plane-crossing branch (needs two nets whose Steiner
    //!   trees actually overlap on a non-endpoint coord, which no
    //!   realistic single-scope `.crn` produces yet — the
    //!   `redstone-door` example has one cell and one net);
    //! - the buffer-coord bridge escape (needs a segment past 15
    //!   blocks and a candidate that collides with a reserved coord,
    //!   which again the fixtures do not exercise);
    //! - the `E_CROSSING_CONGESTION` / `E_BUFFER_COORD_COLLISION`
    //!   diagnostic codes and the `E_NO_CIRCUIT_REGION` refusal;
    //! - the topological invariant panic mirroring
    //!   [`crate::delay::compile_delay`].
    //!
    //! Uses crate-internal struct construction (all `PlacedCellNode` /
    //! `PlacementIr` / `CircuitRegionReservation` fields are `pub`;
    //! `#[non_exhaustive]` blocks only external crates), keeping the
    //! integration-test surface in `tests/crossing.rs` focused on
    //! synth fixtures.

    use cairn_lang_core::Edition;
    use cairn_lang_core::error::Span;

    use super::compile_crossing;
    use crate::diagnostic::DiagnosticCode;
    use crate::edition_netlist_ir::EditionCell;
    use crate::logic_ir::ScopeKind;
    use crate::netlist_ir::{CellPortDriver, NetRef, NetlistOutput, PortName};
    use crate::placement_ir::{
        CellCoord, CircuitRegionReservation, PlacedCellNode, PlacementIr, RouteLayer,
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

    fn placed_cell(
        cell: EditionCell,
        coord: CellCoord,
        drivers: Vec<CellPortDriver>,
    ) -> PlacedCellNode {
        PlacedCellNode {
            cell,
            drivers,
            coord,
            wire_length: Some(0),
            delay_ticks: Some(0),
            buffer_coords: Vec::new(),
            span: Span::default(),
        }
    }

    #[test]
    fn no_scopes_input_yields_no_scopes_output() {
        // A module without any redstone (upstream stages already
        // elided every empty scope through `ScopedPlacementIr::push`)
        // survives crossing legalization as a no-op, matching the
        // delay pass's shape.
        let legalized = compile_crossing(&ScopedPlacementIr::new());
        assert!(legalized.diagnostics.is_empty());
        assert!(legalized.scoped.scopes.is_empty());
    }

    #[test]
    fn empty_scope_passes_through_untouched() {
        // A caller-side hand-built input that hands the pass a scope
        // with no cells and no outputs must return that scope verbatim
        // — the "empty" elision is `ScopedPlacementIr::push`'s job on
        // the input side, not this pass's. Mirrors the delay pass's
        // pass-through so a downstream reader sees consistent
        // behaviour across stages.
        let ir = PlacementIr::new(Edition::Java);
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "empty", ir));
        assert!(legalized.diagnostics.is_empty());
        assert_eq!(legalized.scoped.scopes.len(), 1);
        assert!(legalized.scoped.scopes[0].ir.cells.is_empty());
    }

    #[test]
    fn single_net_no_buffer_is_untouched() {
        // 1 cell driven from Input(0), Manhattan segment <= 15 → no
        // buffer coord, no crossing. `buffer_coords` stays empty.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(0, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "single", ir));
        assert!(legalized.diagnostics.is_empty());
        let cell = &legalized.scoped.scopes[0].ir.cells[0];
        assert!(
            cell.buffer_coords.is_empty(),
            "segment <= 15 blocks needs no buffer, got {:?}",
            cell.buffer_coords,
        );
        assert_eq!(
            cell.coord.layer,
            RouteLayer::Plane,
            "cell coord stays on plane",
        );
    }

    #[test]
    fn long_segment_places_buffer_on_plane() {
        // A driver segment of 16 blocks (Manhattan) trips the
        // attenuation limit once → exactly one buffer coord at
        // `k=1 * DUST_ATTENUATION_LIMIT = 15` steps along the L-shape.
        // No collision → the buffer sits on `RouteLayer::Plane`.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(16, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "long", ir));
        assert!(
            legalized.diagnostics.is_empty(),
            "clean fixture: {:?}",
            legalized.diagnostics,
        );
        let cell = &legalized.scoped.scopes[0].ir.cells[0];
        assert_eq!(
            cell.buffer_coords.len(),
            1,
            "16-block segment needs exactly one buffer, got {:?}",
            cell.buffer_coords,
        );
        assert_eq!(
            cell.buffer_coords[0].layer,
            RouteLayer::Plane,
            "no collision → buffer stays on plane",
        );
    }

    #[test]
    fn crossing_congestion_fires_when_void_is_one() {
        // Two nets whose Steiner trees overlap on a plane coord with
        // `void=1`: no bridge layer to escape to → refuse. Built by
        // hand because a single-scope `.crn` with genuine crossings
        // does not exist in the example set yet.
        //
        // Input pads land at (0, 0, 1) and (0, 0, 2) per
        // `routing::input_pad` (z = 1 + i, saturating at depth-1).
        // Placing cell 0 at (3, 0, 3) and cell 1 at (3, 0, 1) makes
        // the two L-shapes (x-then-z-then-y) share the wire coord
        // (3, 0, 2) — neither cell nor pad, so it counts as a
        // genuine crossing. void=1 rejects the bridge escape.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 5, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["b".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 3),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "crossed", ir));
        assert!(
            legalized
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::CrossingCongestion),
            "void=1 must trip E_CROSSING_CONGESTION: {:?}",
            legalized.diagnostics,
        );
        assert!(
            legalized.scoped.scopes.is_empty(),
            "failed scope must elide",
        );
    }

    #[test]
    fn crossing_stays_silent_when_void_allows_bridge() {
        // Same fixture but with `void=2` — a bridge layer is
        // available, so the crossing legalizes silently and the
        // scope survives with cells intact. Buffer coords stay empty
        // because every segment is under `DUST_ATTENUATION_LIMIT`
        // (short segments do not need any buffer coord at all, and
        // v1 does not lift the wire crossing itself onto Bridge —
        // the crossing set is only used to steer buffer placement).
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 5, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["b".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 3),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "crossed", ir));
        assert!(
            legalized.diagnostics.is_empty(),
            "void=2 gives bridges room: {:?}",
            legalized.diagnostics,
        );
        assert_eq!(legalized.scoped.scopes.len(), 1);
        assert_eq!(legalized.scoped.scopes[0].ir.cells.len(), 2);
        // v1 does not lift the wire crossing onto Bridge — cells and
        // buffers stay on Plane. Locked here so a change that grows
        // wire-layer materialisation on the IR trips this test rather
        // than silently reshaping the wire form.
        for cell in &legalized.scoped.scopes[0].ir.cells {
            assert_eq!(
                cell.coord.layer,
                RouteLayer::Plane,
                "cell coord stays on plane; got {:?}",
                cell.coord,
            );
            assert!(
                cell.buffer_coords.is_empty(),
                "short segments need no buffer coord; got {:?}",
                cell.buffer_coords,
            );
        }
    }

    #[test]
    fn crossing_diagnostic_names_both_conflicting_nets() {
        // Same crossing fixture as the void=1 refusal, this time
        // pinned to the primary text so a diagnostic-consumer can
        // rely on the "left vs right at coord" shape. Uses the
        // `NetlistInput.name` values `sig.a` / `sig.b` so the
        // human-facing label resolution path is exercised.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 5, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["b".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 3),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "crossed", ir));
        let diag = legalized
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::CrossingCongestion)
            .expect("crossing must fire");
        assert!(
            diag.primary.contains("sig.a"),
            "primary must name the first net (sig.a): {}",
            diag.primary,
        );
        assert!(
            diag.primary.contains("sig.b"),
            "primary must name the second net (sig.b): {}",
            diag.primary,
        );
    }

    #[test]
    fn buffer_collision_fires_when_bridge_slot_taken() {
        // Fan-out on Input(0) to three cells at x = 16, 17, 18. All
        // three drivers compute their buffer candidate at (15,0,1)
        // — the 15-step point of the shared prefix. With `void=2`
        // the bridge has exactly one y-layer (y=1); the first
        // buffer lands on plane, the second escapes to bridge y=1,
        // and the third has nowhere left to go → refuse with
        // `E_BUFFER_COORD_COLLISION`.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        for x in 16..19u32 {
            ir.cells.push(placed_cell(
                EditionCell::JavaRepeaterOr,
                CellCoord::new(x, 0, 1),
                vec![CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                }],
            ));
        }
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "packed", ir));
        assert!(
            legalized
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::BufferCoordCollision),
            "third driver must trip E_BUFFER_COORD_COLLISION: {:?}",
            legalized.diagnostics,
        );
        assert!(
            legalized.scoped.scopes.is_empty(),
            "failed scope must elide",
        );
    }

    #[test]
    fn bridge_escape_uses_next_free_y_layer() {
        // Fan-out on Input(0): two cells at x=16 and x=17 both take
        // their buffer coord at (15,0,1) — the 15-step point of the
        // Input(0)-to-sink L-shape is the same for both because the
        // shared prefix is 15 blocks long. The first cell's buffer
        // lands on plane; the second's plane candidate is taken, so
        // it escapes to Bridge at y=1 (first free bridge layer in
        // the `void=3` budget).
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, 3));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(16, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(17, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "escaped", ir));
        assert!(
            legalized.diagnostics.is_empty(),
            "void=3 lets the second buffer escape: {:?}",
            legalized.diagnostics,
        );
        let cells = &legalized.scoped.scopes[0].ir.cells;
        assert_eq!(cells[0].buffer_coords.len(), 1);
        assert_eq!(cells[1].buffer_coords.len(), 1);
        assert_eq!(
            cells[0].buffer_coords[0].layer,
            RouteLayer::Plane,
            "first buffer sits on plane",
        );
        assert_eq!(
            cells[1].buffer_coords[0].layer,
            RouteLayer::Bridge,
            "second buffer escapes to bridge",
        );
        assert_eq!(
            cells[1].buffer_coords[0].y, 1,
            "bridge escape lands on first free y-layer (y=1)",
        );
    }

    #[test]
    #[should_panic(expected = "must run at most once per delayed IR")]
    fn re_running_crossing_pass_panics_loudly() {
        // Chaining `compile_crossing(&legalized.scoped)` is forbidden
        // by the phase table on `PlacedCellNode`. Loud in release so
        // a caller cannot silently double-populate `buffer_coords`.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(16, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        let first = compile_crossing(&scoped(ScopeKind::Struct, "twice", ir));
        let _second = compile_crossing(&first.scoped);
    }

    #[test]
    fn missing_region_with_cells_fires_no_circuit_region() {
        // Hand-built `PlacementIr` with cells but no region reaches
        // crossing because it skipped placement. Mirrors the delay
        // pass's hardening.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(0, 0, 0),
            vec![],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "roomless", ir));
        let diag = legalized
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::NoCircuitRegion)
            .expect("missing region with cells must fire E_NO_CIRCUIT_REGION");
        assert!(
            diag.primary.contains("struct `roomless`"),
            "primary must name the scope, got {:?}",
            diag.primary,
        );
        assert!(
            legalized.scoped.scopes.is_empty(),
            "failed scope must elide",
        );
    }

    #[test]
    fn missing_region_empty_scope_passes_through() {
        // No region + no cells + no outputs = harmless empty scope
        // that the pass returns verbatim. Prior stages will not
        // produce this shape, but a hand-built input reaching here
        // must not trip the `NoCircuitRegion` refusal — that guard
        // only fires when the scope carries cells or outputs.
        let ir = PlacementIr::new(Edition::Java);
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "harmless", ir));
        assert!(legalized.diagnostics.is_empty());
        assert_eq!(legalized.scoped.scopes.len(), 1);
        assert!(legalized.scoped.scopes[0].ir.region.is_none());
    }

    #[test]
    fn output_only_scope_with_missing_region_refuses() {
        // Outputs but no cells with no region: still a phase-table
        // violation because `PlacementIr::is_empty` returns false as
        // long as outputs exist. Should refuse with `NoCircuitRegion`
        // for parity with the delay pass.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.outputs.push(NetlistOutput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["x".into()]),
            driver: NetRef::Input(0),
            span: Span::default(),
        });
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "outputless", ir));
        assert!(
            legalized
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::NoCircuitRegion),
            "missing region with outputs must refuse: {:?}",
            legalized.diagnostics,
        );
    }

    #[test]
    #[should_panic(expected = "topological invariant broken")]
    fn out_of_range_net_ref_cell_panics_loudly() {
        // Mirrors `delay.rs`'s equivalent guard: a hand-built IR with
        // `NetRef::Cell(u32::MAX)` violates the synthesis-side
        // topological invariant, and the crossing pass panics loud so
        // a caller-side bug cannot produce silently wrong
        // `buffer_coords`.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 3, 2));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(0, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(u32::MAX),
            }],
        ));
        let _ = compile_crossing(&scoped(ScopeKind::Struct, "broken", ir));
    }
}
