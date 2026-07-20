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
/// v1 convention; once a subsequent PR needs pad coords outside
/// routing, `input_pads` joins [`crate::placement_ir::PlacementIr`]
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
