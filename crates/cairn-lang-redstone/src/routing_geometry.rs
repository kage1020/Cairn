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
//! [`NetTree`] is the one derivation of where a net's dust runs.
//! Stage 2 drains [`NetTree::wire_path`] into its occupancy set, stage
//! 3 measures [`NetTree::route_to`] to count buffer repeaters, and
//! stage 4 walks the same route to place them. Keeping the tree as
//! terminals-plus-edges rather than as a flat coord list is what makes
//! the second projection possible: "every coord the net occupies" and
//! "the dust the signal travels from the source to *this* sink" are
//! different questions, and answering the second with a fresh
//! [`l_shape_path`] from source to sink — as the crossing pass once
//! did — puts buffer repeaters on coords the net does not own.
//!
//! The trees are recomputed per stage rather than stored on
//! [`crate::placement_ir::PlacementIr`]. They are a pure function of
//! the cells, outputs, and reservation the IR already carries, so a
//! stored copy would be a cache with a staleness mode this has not,
//! and it would put every coord of every net into the JSON dump each
//! stage emits.

use std::collections::{HashMap, VecDeque};

use crate::netlist_ir::NetRef;
use crate::placement_ir::{CellCoord, CircuitRegionReservation, PlacementIr};

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

/// Every net in a scope: driver → the sinks it feeds. A cell driver
/// sinks at the cell body; an output driver sinks at the actuator's
/// pad.
///
/// Shared by the routing, delay, and crossing passes so the three
/// cannot disagree about what a net *is*. Each pass supplies its own
/// `NetRef → source coord` mapping (they differ only in how loud they
/// are about a hand-built IR that breaks the topological invariant),
/// but the sink side is this one function.
pub(crate) fn collect_nets(
    ir: &PlacementIr,
    region: &CircuitRegionReservation,
) -> HashMap<NetRef, Vec<CellCoord>> {
    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    for cell in &ir.cells {
        for driver in &cell.drivers {
            nets.entry(driver.net).or_default().push(cell.coord);
        }
    }
    for (k, output) in ir.outputs.iter().enumerate() {
        nets.entry(output.driver)
            .or_default()
            .push(output_pad(k, region));
    }
    nets
}

/// Deterministic net processing order: fanout descending, ties broken
/// by [`net_ref_key`] ascending. `HashMap` iteration order never
/// reaches an output.
pub(crate) fn net_order(nets: &HashMap<NetRef, Vec<CellCoord>>) -> Vec<NetRef> {
    let mut order: Vec<NetRef> = nets.keys().copied().collect();
    order.sort_by(|a, b| {
        nets[b]
            .len()
            .cmp(&nets[a].len())
            .then_with(|| net_ref_key(*a).cmp(&net_ref_key(*b)))
    });
    order
}

/// The routed Steiner tree of every net, keyed by driver.
pub(crate) fn net_trees<F>(
    nets: &HashMap<NetRef, Vec<CellCoord>>,
    source_of_net: F,
) -> HashMap<NetRef, NetTree>
where
    F: Fn(NetRef) -> CellCoord,
{
    nets.iter()
        .map(|(net, sinks)| (*net, net_tree(source_of_net(*net), sinks)))
        .collect()
}

/// One net's rectilinear Steiner tree: the terminal set, whose first
/// entry is always the net's source, and the MST edges over it in
/// Kruskal acceptance order.
///
/// Held as terminals-plus-edges rather than as a flat coord list
/// because the tree has to answer two different questions and give
/// answers that agree — see the module doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetTree {
    terminals: Vec<CellCoord>,
    edges: Vec<(usize, usize)>,
}

impl NetTree {
    /// Every coord the net occupies: each MST edge rendered as its
    /// L-shape, concatenated in acceptance order.
    ///
    /// Shared endpoints repeat where two edges meet. Callers that want
    /// occupancy drain the result into a set; the duplicates are kept
    /// so the concatenation stays byte-stable against any future
    /// dedupe choice. Always non-empty and starting at the source — a
    /// caller that discarded an empty return would drop the degenerate
    /// (no sinks, or the only sink is the source) case where the
    /// source still occupies its own coord.
    pub(crate) fn wire_path(&self) -> Vec<CellCoord> {
        if self.edges.is_empty() {
            return vec![self.terminals[0]];
        }
        let mut path: Vec<CellCoord> = Vec::new();
        for &(i, j) in &self.edges {
            path.extend(l_shape_path(self.terminals[i], self.terminals[j]));
        }
        path
    }

    /// The dust the signal actually travels from the source to `sink`:
    /// the unique tree path between those two terminals, rendered edge
    /// by edge with the shared endpoints collapsed, so the result is a
    /// walk in which every step moves one block.
    ///
    /// This is what a buffer repeater has to sit on, and it is not in
    /// general `l_shape_path(source, sink)`: an MST drops the direct
    /// edge whenever two others are cheaper, and the signal then
    /// detours through the intervening terminal. `route_to(source)` is
    /// the single-coord path.
    ///
    /// `None` when `sink` is not one of this net's terminals — a
    /// caller asking about a sink on a different net.
    pub(crate) fn route_to(&self, sink: CellCoord) -> Option<Vec<CellCoord>> {
        let target = self.terminals.iter().position(|t| *t == sink)?;
        let chain = self.terminal_chain(target)?;
        let mut path = vec![self.terminals[chain[0]]];
        for pair in chain.windows(2) {
            path.extend(
                l_shape_path(self.terminals[pair[0]], self.terminals[pair[1]])
                    .into_iter()
                    .skip(1),
            );
        }
        Some(path)
    }

    /// Terminal indices from the source to `target`, both ends
    /// included. `None` only if the edge set does not span the
    /// terminals, which Kruskal over a complete graph rules out.
    fn terminal_chain(&self, target: usize) -> Option<Vec<usize>> {
        if target == 0 {
            return Some(vec![0]);
        }
        let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); self.terminals.len()];
        for &(i, j) in &self.edges {
            adjacency[i].push(j);
            adjacency[j].push(i);
        }
        let mut previous: Vec<Option<usize>> = vec![None; self.terminals.len()];
        let mut seen = vec![false; self.terminals.len()];
        seen[0] = true;
        let mut queue: VecDeque<usize> = VecDeque::from([0usize]);
        while let Some(node) = queue.pop_front() {
            for &next in &adjacency[node] {
                if seen[next] {
                    continue;
                }
                seen[next] = true;
                previous[next] = Some(node);
                queue.push_back(next);
            }
        }
        if !seen[target] {
            return None;
        }
        let mut chain = vec![target];
        let mut node = target;
        while let Some(prev) = previous[node] {
            chain.push(prev);
            node = prev;
        }
        chain.reverse();
        Some(chain)
    }
}

/// Manhattan Steiner tree over `{source} ∪ sinks`: Kruskal on the
/// complete Manhattan graph with a `(weight, i, j)` tie-break, which
/// gives a deterministic MST regardless of `HashMap` iteration order.
///
/// Sinks are deduplicated against the terminals already collected, so
/// a net whose only sink is its own source yields an edgeless tree.
pub(crate) fn net_tree(source: CellCoord, sinks: &[CellCoord]) -> NetTree {
    let mut terminals: Vec<CellCoord> = Vec::with_capacity(1 + sinks.len());
    terminals.push(source);
    for s in sinks {
        if !terminals.contains(s) {
            terminals.push(*s);
        }
    }
    let n = terminals.len();
    if n < 2 {
        return NetTree {
            terminals,
            edges: Vec::new(),
        };
    }
    let mut candidates: Vec<(u32, usize, usize)> = Vec::with_capacity(n * (n - 1) / 2);
    for i in 0..n {
        for j in (i + 1)..n {
            candidates.push((manhattan(terminals[i], terminals[j]), i, j));
        }
    }
    candidates.sort_unstable();

    let mut parent: Vec<usize> = (0..n).collect();
    let mut edges: Vec<(usize, usize)> = Vec::with_capacity(n - 1);
    for (_, i, j) in candidates {
        let ri = union_find(&mut parent, i);
        let rj = union_find(&mut parent, j);
        if ri == rj {
            continue;
        }
        parent[ri] = rj;
        edges.push((i, j));
    }
    NetTree { terminals, edges }
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

    fn wire_path(source: CellCoord, sinks: &[CellCoord]) -> Vec<CellCoord> {
        net_tree(source, sinks).wire_path()
    }

    #[test]
    fn net_wire_path_empty_sinks_returns_source_only() {
        let src = CellCoord::new(1, 2, 3);
        assert_eq!(wire_path(src, &[]), vec![src]);
    }

    #[test]
    fn net_wire_path_dedups_source_appearing_in_sinks() {
        let src = CellCoord::new(0, 0, 0);
        assert_eq!(wire_path(src, &[src]), vec![src]);
    }

    #[test]
    fn net_wire_path_single_sink_matches_l_shape() {
        let src = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(2, 0, 1);
        assert_eq!(wire_path(src, &[sink]), l_shape_path(src, sink));
    }

    /// The whole reason the tree is kept as terminals-plus-edges. With
    /// the direct source→sink edge the most expensive of the three, a
    /// minimum spanning tree drops it and the signal detours through
    /// the terminal between — so the dust from the source to that sink
    /// is 18 blocks along a path that shares only its endpoints with
    /// the 10-block straight line. A buffer repeater picked off the
    /// straight line stands on coords the net does not own; picked off
    /// the route it stands on the wire it refreshes.
    #[test]
    fn route_to_follows_the_tree_and_not_the_straight_line() {
        let source = CellCoord::new(0, 0, 0);
        let detour = CellCoord::new(5, 0, 4);
        let sink = CellCoord::new(10, 0, 0);
        let tree = net_tree(source, &[detour, sink]);

        let route = tree.route_to(sink).expect("sink is a terminal");
        assert_eq!(
            route.len() - 1,
            18,
            "9 blocks to the detour terminal and 9 more back out",
        );
        assert_eq!(manhattan(source, sink), 10, "the straight line is shorter");
        assert!(
            route.contains(&detour),
            "the route passes through the terminal the MST kept: {route:?}",
        );

        let straight = l_shape_path(source, sink);
        let owned: std::collections::HashSet<CellCoord> = tree.wire_path().into_iter().collect();
        let strays: Vec<CellCoord> = straight
            .iter()
            .copied()
            .filter(|c| !owned.contains(c))
            .collect();
        assert!(
            !strays.is_empty(),
            "the straight line has to leave the net's wire for this fixture to mean anything",
        );
    }

    /// Every step of a route moves one block: the shared endpoint
    /// between two tree edges appears once, not twice, so an index into
    /// the route counts blocks travelled.
    #[test]
    fn route_to_is_a_walk_with_no_repeated_step() {
        let source = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(10, 0, 0);
        let route = net_tree(source, &[CellCoord::new(5, 0, 4), sink])
            .route_to(sink)
            .expect("sink is a terminal");
        for pair in route.windows(2) {
            assert_eq!(
                manhattan(pair[0], pair[1]),
                1,
                "consecutive route coords are adjacent: {route:?}",
            );
        }
        assert_eq!(route.first().copied(), Some(source));
        assert_eq!(route.last().copied(), Some(sink));
    }

    /// A net with one sink has nothing to detour through, so its route
    /// is the L-shape both the tree and the straight line agree on.
    #[test]
    fn route_to_a_lone_sink_is_the_l_shape() {
        let source = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(4, 0, 2);
        assert_eq!(
            net_tree(source, &[sink]).route_to(sink),
            Some(l_shape_path(source, sink)),
        );
    }

    /// The source is its own terminal at distance zero — the case a
    /// driver whose sink coincides with its source takes, and the one
    /// that must not report a buffer's worth of dust.
    #[test]
    fn route_to_the_source_is_a_single_coord() {
        let source = CellCoord::new(3, 0, 1);
        let tree = net_tree(source, &[CellCoord::new(9, 0, 1)]);
        assert_eq!(tree.route_to(source), Some(vec![source]));
    }

    /// A coord that is not a terminal of this net gets no route rather
    /// than a plausible-looking one. The callers turn `None` into a
    /// panic naming the disagreement, which beats measuring a segment
    /// against a net that does not reach it.
    #[test]
    fn route_to_a_foreign_sink_is_none() {
        let tree = net_tree(CellCoord::new(0, 0, 0), &[CellCoord::new(4, 0, 0)]);
        assert_eq!(tree.route_to(CellCoord::new(4, 0, 1)), None);
    }

    /// `collect_nets` is the sink side every pass shares: a cell driver
    /// sinks at the cell body, an output driver at the actuator's pad.
    #[test]
    fn collect_nets_maps_cell_drivers_to_bodies_and_outputs_to_pads() {
        use cairn_lang_core::Edition;
        use cairn_lang_core::ast::DottedRef;

        use crate::edition_netlist_ir::EditionCell;
        use crate::netlist_ir::{CellPortDriver, NetlistOutput, PortName};
        use crate::placement_ir::{PlacedCellNode, PlacementIr, PlacementPhase};

        let region = reservation(8, 4);
        let mut ir = PlacementIr::new(Edition::Java);
        ir.cells.push(PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers: vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            coord: CellCoord::new(2, 0, 1),
            phase: PlacementPhase::Unrouted,
            span: Span::default(),
        });
        ir.outputs.push(NetlistOutput {
            name: DottedRef::new("sig".into(), vec!["out".into()]),
            driver: NetRef::Cell(0),
            span: Span::default(),
        });

        let nets = collect_nets(&ir, &region);
        assert_eq!(nets[&NetRef::Input(0)], vec![CellCoord::new(2, 0, 1)]);
        assert_eq!(nets[&NetRef::Cell(0)], vec![output_pad(0, &region)]);
    }

    /// Net order is fanout descending, ties by [`net_ref_key`]. Pinned
    /// because three passes walk it and a `HashMap`'s own order would
    /// make their outputs differ run to run.
    #[test]
    fn net_order_sorts_by_fanout_then_key() {
        let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
        nets.insert(NetRef::Cell(0), vec![CellCoord::new(1, 0, 0)]);
        nets.insert(
            NetRef::Input(1),
            vec![CellCoord::new(2, 0, 0), CellCoord::new(3, 0, 0)],
        );
        nets.insert(NetRef::Input(0), vec![CellCoord::new(4, 0, 0)]);
        assert_eq!(
            net_order(&nets),
            vec![NetRef::Input(1), NetRef::Input(0), NetRef::Cell(0)],
        );
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
            wire_path(a, &[b, c]),
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
