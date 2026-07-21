//! Shared rectilinear-geometry helpers for the routing / delay /
//! crossing passes.
//!
//! The three passes (`spec/redstone` §14.5 stages 2, 3, and 4) all
//! need the same pad-coordinate convention, Manhattan distance,
//! deterministic net-order key, and the Kruskal-plus-L-shape Steiner
//! walker over `{source} ∪ sinks`. Keeping the primitives in one
//! module guarantees that a future PR touching the Kruskal tie-break
//! (`(weight, i, j)`), the L-shape axis order (`x → z → y`), or the
//! `NetRef` sort key updates every downstream consumer in one place —
//! the JSON dumps compared byte-for-byte by
//! `crates/cairn-lang-redstone/tests/routing.rs` and the crossing /
//! delay integration tests catch any drift.
//!
//! The `net_wire_path` primitive returns the ordered per-net path as
//! a `Vec<CellCoord>`; the routing pass drains the return value into
//! its occupancy `HashSet` while the crossing pass walks it directly
//! for buffer allocation. Neither caller cares whether the other
//! stores occupancy — the shared helper stays pure.

use crate::netlist_ir::NetRef;
use crate::placement_ir::{CellCoord, CircuitRegionReservation};

/// Deterministic net-order key matching the routing pass's tie-break:
/// `Input(_)` sorts before `Cell(_)`, then by index ascending.
pub(crate) fn net_ref_key(net: NetRef) -> (u8, u32) {
    match net {
        NetRef::Input(i) => (0, i),
        NetRef::Cell(j) => (1, j),
    }
}

/// v1 input-pad coordinate: left edge (`x=0`), first service layer
/// (`y=0`), z-axis increasing as the input index grows. Saturates at
/// `depth-1` whenever the input count would push z past the region's
/// z-extent (`inputs.len() + 1 > depth`); the resulting overlap is
/// caught at seeding time and surfaces as `E_ROUTE_CONGESTION`
/// rather than a silent misroute. Pinning the coordinate here is a
/// v1 convention; once a consumer outside this crate needs the pad
/// coords, `input_pads` joins [`crate::placement_ir::PlacementIr`]
/// as a `#[non_exhaustive]`-safe field.
pub(crate) fn input_pad(i: usize, region: &CircuitRegionReservation) -> CellCoord {
    let raw = u32::try_from(i.saturating_add(1)).unwrap_or(u32::MAX);
    let z = raw.min(region.depth.saturating_sub(1));
    CellCoord::new(0, 0, z)
}

/// v1 output-pad coordinate: right edge (`x=width-1`), same
/// saturating z-axis convention as [`input_pad`].
pub(crate) fn output_pad(k: usize, region: &CircuitRegionReservation) -> CellCoord {
    let raw = u32::try_from(k.saturating_add(1)).unwrap_or(u32::MAX);
    let z = raw.min(region.depth.saturating_sub(1));
    let x = region.width.saturating_sub(1);
    CellCoord::new(x, 0, z)
}

/// Manhattan (L¹) distance between two cell coordinates.
pub(crate) fn manhattan(a: CellCoord, b: CellCoord) -> u32 {
    let dx = a.x.max(b.x) - a.x.min(b.x);
    let dy = a.y.max(b.y) - a.y.min(b.y);
    let dz = a.z.max(b.z) - a.z.min(b.z);
    dx.saturating_add(dy).saturating_add(dz)
}

/// Manhattan Steiner tree over `{source} ∪ sinks`, rendered as the
/// concatenation of every MST edge's L-shape path. Kruskal on the
/// complete Manhattan graph with `(weight, i, j)` tie-break gives a
/// deterministic MST regardless of `HashMap` iteration order.
///
/// Callers that only need occupancy drain the returned `Vec` into a
/// `HashSet`; callers that need the ordered per-net path (e.g. the
/// crossing pass for buffer allocation) walk the `Vec` directly.
/// The result is always non-empty and starts at `source` — a caller
/// that discards empty returns silently would drop the degenerate
/// (`sinks.is_empty()` or `sinks == [source]`) case where the source
/// still contributes a single-coord occupancy entry.
pub(crate) fn net_wire_path(source: CellCoord, sinks: &[CellCoord]) -> Vec<CellCoord> {
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
pub(crate) fn union_find(parent: &mut [usize], x: usize) -> usize {
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
/// pinning a canonical choice among Manhattan-equivalent L-shapes so
/// the routing and crossing passes see the same wire coords for the
/// same terminal pair. Result length equals `manhattan(a, b) + 1`.
pub(crate) fn l_shape_path(a: CellCoord, b: CellCoord) -> Vec<CellCoord> {
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

#[cfg(test)]
mod tests {
    use cairn_lang_core::error::Span;

    use super::*;

    fn reservation(width: u32, depth: u32) -> CircuitRegionReservation {
        CircuitRegionReservation {
            label: "floor".to_owned(),
            void: 1,
            width,
            depth,
            span: Span::default(),
        }
    }

    #[test]
    fn manhattan_sums_absolute_axis_deltas() {
        assert_eq!(
            manhattan(CellCoord::new(0, 0, 0), CellCoord::new(3, 4, 5)),
            12
        );
        assert_eq!(
            manhattan(CellCoord::new(5, 5, 5), CellCoord::new(5, 5, 5)),
            0
        );
        assert_eq!(
            manhattan(CellCoord::new(3, 4, 5), CellCoord::new(0, 0, 0)),
            12
        );
    }

    #[test]
    fn net_ref_key_orders_input_before_cell_then_by_index() {
        assert!(net_ref_key(NetRef::Input(u32::MAX)) < net_ref_key(NetRef::Cell(0)));
        assert!(net_ref_key(NetRef::Input(0)) < net_ref_key(NetRef::Input(1)));
        assert!(net_ref_key(NetRef::Cell(0)) < net_ref_key(NetRef::Cell(1)));
    }

    #[test]
    fn input_pad_saturates_at_depth_minus_one() {
        let region = reservation(10, 3);
        assert_eq!(input_pad(0, &region), CellCoord::new(0, 0, 1));
        assert_eq!(input_pad(1, &region), CellCoord::new(0, 0, 2));
        // depth-1 = 2 ceilings anything past the second input.
        assert_eq!(input_pad(5, &region), CellCoord::new(0, 0, 2));
    }

    #[test]
    fn output_pad_sits_on_right_edge_and_saturates_z() {
        let region = reservation(4, 3);
        assert_eq!(output_pad(0, &region), CellCoord::new(3, 0, 1));
        assert_eq!(output_pad(1, &region), CellCoord::new(3, 0, 2));
        assert_eq!(output_pad(5, &region), CellCoord::new(3, 0, 2));
    }

    #[test]
    fn l_shape_path_walks_x_then_z_then_y() {
        // Axis-order regression fence: swapping any two axes here
        // would break `crossing_congestion_fires_when_void_is_one`
        // and the crossing-pass buffer allocator that walks this Vec.
        let path = l_shape_path(CellCoord::new(0, 0, 0), CellCoord::new(2, 1, 1));
        assert_eq!(
            path,
            vec![
                CellCoord::new(0, 0, 0),
                CellCoord::new(1, 0, 0),
                CellCoord::new(2, 0, 0),
                CellCoord::new(2, 0, 1),
                CellCoord::new(2, 1, 1),
            ]
        );
    }

    #[test]
    fn l_shape_path_handles_reverse_axes() {
        let path = l_shape_path(CellCoord::new(2, 1, 1), CellCoord::new(0, 0, 0));
        assert_eq!(
            path,
            vec![
                CellCoord::new(2, 1, 1),
                CellCoord::new(1, 1, 1),
                CellCoord::new(0, 1, 1),
                CellCoord::new(0, 1, 0),
                CellCoord::new(0, 0, 0),
            ]
        );
    }

    #[test]
    fn l_shape_path_length_equals_manhattan_plus_one() {
        let a = CellCoord::new(1, 2, 3);
        let b = CellCoord::new(7, 5, 4);
        assert_eq!(l_shape_path(a, b).len(), manhattan(a, b) as usize + 1);
    }

    #[test]
    fn net_wire_path_empty_sinks_returns_source_only() {
        let src = CellCoord::new(1, 2, 3);
        assert_eq!(net_wire_path(src, &[]), vec![src]);
    }

    #[test]
    fn net_wire_path_dedups_source_appearing_in_sinks() {
        let src = CellCoord::new(0, 0, 0);
        assert_eq!(net_wire_path(src, &[src]), vec![src]);
    }

    #[test]
    fn net_wire_path_single_sink_matches_l_shape() {
        let src = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(2, 0, 1);
        assert_eq!(net_wire_path(src, &[sink]), l_shape_path(src, sink));
    }

    #[test]
    fn net_wire_path_kruskal_tie_break_picks_lex_smallest_edges() {
        // Three collinear terminals: A-B and B-C each weigh 2, A-C
        // weighs 4. A weight-only sort would leave the MST ambiguous
        // between {A-B, B-C} and {A-B, A-C} once ties break either
        // way; the (weight, i, j) tie-break pins {A-B, B-C} and the
        // A-C edge cycle-skips. Any drift in the tie-break rule
        // reorders the concatenated Vec and this test catches it.
        let a = CellCoord::new(0, 0, 0);
        let b = CellCoord::new(2, 0, 0);
        let c = CellCoord::new(4, 0, 0);
        assert_eq!(
            net_wire_path(a, &[b, c]),
            vec![
                CellCoord::new(0, 0, 0),
                CellCoord::new(1, 0, 0),
                CellCoord::new(2, 0, 0),
                CellCoord::new(2, 0, 0),
                CellCoord::new(3, 0, 0),
                CellCoord::new(4, 0, 0),
            ]
        );
    }
}
