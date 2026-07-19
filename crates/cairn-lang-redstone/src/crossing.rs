//! Delayed Placement IR → legalized Placement IR lowering (crossing
//! legalization).
//!
//! Stage 4 of the five-stage place-and-route pipeline `spec/redstone`
//! §14.5 lays out (Placement → Steiner routing → Delay insertion →
//! Crossing legalization → Edition legalization). Walks every
//! [`crate::placement_ir::PlacedCellNode`] in each scope's delayed
//! Placement IR, re-derives every net's Manhattan Steiner tree from
//! the same `NetRef → source coord` mapping the routing and delay
//! passes use (the routing pass discarded its per-scope occupancy set
//! before yielding the routed IR, so wire coords are cheap to re-walk
//! here and would bloat the JSON wire form if stored twice), detects
//! coords two different nets would otherwise share on the ground
//! [`crate::placement_ir::RouteLayer::Plane`], and materialises the
//! two escape hatches §14.5's "pseudo-2.5D" text names:
//!
//! - **Bridge / Via escape.** A coord shared by two distinct nets is a
//!   crossing and would short in the Minecraft voxel model. The pass
//!   lifts the coord onto the next unused y-layer of the scope's
//!   `circuit region=<label> void=<N>` reservation
//!   ([`crate::placement_ir::RouteLayer::Bridge`]), and reserves a
//!   [`crate::placement_ir::RouteLayer::Via`] transition rung where a
//!   bridge segment enters or leaves the plane. `void=1` reservations
//!   have no free y-layer to escape to, so the pass refuses with
//!   [`crate::DiagnosticCode::CrossingCongestion`] rather than emit an
//!   unrealisable layout.
//! - **Implicit buffer repeater coord assignment.** The delay pass
//!   (stage 3) counted `floor((s - 1) / DUST_ATTENUATION_LIMIT)`
//!   buffer repeaters per driver segment of length `s`, folded their
//!   tick contribution into `delay_ticks`, but deferred coord
//!   assignment because stage 4 already owns the free-block set the
//!   Bridge / Via escape draws from. This pass walks each driver's
//!   Manhattan L-shape (x-then-z-then-y, matching the routing pass's
//!   axis order so the regression story pins across stages) and picks
//!   coords at `k * DUST_ATTENUATION_LIMIT` (`k = 1..=buffer_count`).
//!   A candidate that collides with a cell coord, pad coord, existing
//!   buffer, or plane crossing escapes to the bridge layer; if that
//!   layer is also unavailable (`void < 2`) the pass refuses with
//!   [`crate::DiagnosticCode::BufferCoordCollision`].
//!
//! Failed scopes are elided from the output for the same reason the
//! routing pass elides congestion failures — a partial `buffer_coords`
//! set would let the future block-array voxel lowering (stage 5's
//! downstream consumer) materialise buffers against a layout no other
//! stage can realise.
//!
//! The crossing pass is a field write on
//! [`crate::placement_ir::PlacedCellNode::buffer_coords`] per the phase
//! table on that type; no new IR type is introduced. `--stage
//! placement` / `--stage route` / `--stage delay` JSON stays
//! byte-identical to today because `buffer_coords` is serde-skipped on
//! empty and `layer` on plane, and both fields serialise appended
//! after every previously present field in the phase table's field
//! declaration order (matching serde's compact-JSON layout) when this
//! pass writes them.

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
    // "first owner wins" choice is deterministic across runs.
    let mut plane_owners: HashMap<CellCoord, NetRef> = HashMap::new();
    let mut crossings: HashSet<CellCoord> = HashSet::new();
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
                Some(_) => {
                    crossings.insert(*coord);
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
        let mut sorted: Vec<CellCoord> = crossings.iter().copied().collect();
        sorted.sort_unstable_by_key(|c| (c.x, c.z));
        return Err(crossing_congestion_diagnostic(
            entry,
            &region,
            sorted[0],
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
        debug_assert!(
            cell.buffer_coords.is_empty(),
            "legalize_scope re-writing a PlacedCellNode whose buffer_coords is already {} entries — crossing legalization should run once per delayed IR",
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
/// placed on the plane escapes to `RouteLayer::Bridge` at `y = 1`; if
/// the bridge slot is also taken (or `void < 2`), refuse with
/// `E_BUFFER_COORD_COLLISION`. Split out of `legalize_scope` so the
/// entry function stays under clippy's `too_many_lines` budget and
/// the allocation strategy reads as a self-contained table.
fn allocate_buffer_coords<F>(
    ir: &PlacementIr,
    entry: &ScopedPlacementIrEntry,
    region: &CircuitRegionReservation,
    cell_coords: &[CellCoord],
    crossings: &HashSet<CellCoord>,
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
                // `k * DUST_ATTENUATION_LIMIT` steps from the source.
                // Saturating at the last path coord defends against
                // an off-by-one in a hand-built IR whose segment
                // disagrees with the derived path length; the delay
                // pass's `MAX_ATTENUATION_SEGMENT` cap keeps this
                // path short enough that the `usize` index cannot
                // overflow in practice.
                let idx = (k as usize).saturating_mul(DUST_ATTENUATION_LIMIT as usize);
                let candidate = *path
                    .get(idx)
                    .unwrap_or_else(|| path.last().expect("path always has >=1 coord"));
                let plane_taken = reserved.contains(&candidate)
                    || crossings.contains(&candidate)
                    || plane_buffers.contains(&candidate);
                if !plane_taken {
                    plane_buffers.insert(candidate);
                    buffers_for_cell.push(candidate);
                    continue;
                }
                let bridge_candidate =
                    CellCoord::with_layer(candidate.x, 1, candidate.z, RouteLayer::Bridge);
                if region.void < 2 || bridge_buffers.contains(&bridge_candidate) {
                    return Err(buffer_collision_diagnostic(
                        entry, region, cell_index, candidate,
                    ));
                }
                bridge_buffers.insert(bridge_candidate);
                buffers_for_cell.push(bridge_candidate);
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
/// concatenation of every MST edge's L-shape path. Deterministic
/// weight/index tie-break so a follow-up pass that consults this
/// path for elbow selection has a single canonical order to slot
/// into. Matches the routing pass's `route_net` algorithm; kept
/// crate-local so the crossing pass reads standalone.
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
/// sequence including both endpoints. Axis order is
/// x-then-z-then-y — the routing pass's regression story pins on this
/// order, and any follow-up pass that picks the less-congested elbow
/// per net can only firm this up because both L-shapes have identical
/// Manhattan length by construction. Result length equals
/// `manhattan(a, b) + 1`.
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
    reservation: &CircuitRegionReservation,
    anchor: CellCoord,
    crossing_count: usize,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` has {crossing_count} plane crossing(s) (first at ({x},{y},{z})) but the `void={void}` reservation offers no bridge layer to escape to — bridges need at least y=1, which requires void>=2",
        kind = entry.kind.label(),
        name = entry.name,
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
        // because every segment is under `DUST_ATTENUATION_LIMIT`.
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
