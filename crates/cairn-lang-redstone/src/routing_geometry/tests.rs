use std::collections::HashSet;

use cairn_lang_core::error::Span;

use super::*;
use crate::grid::output_pad;
use crate::netlist_ir::PortName;
use crate::placement_ir::RouteLayer;

fn reservation(width: u32, depth: u32) -> CircuitRegionReservation {
    CircuitRegionReservation {
        label: "floor".to_owned(),
        void: 1,
        width,
        depth,
        span: Span::default(),
    }
}

/// A reservation with an explicit service height.
fn region(width: u32, depth: u32, void: u32) -> CircuitRegionReservation {
    CircuitRegionReservation {
        void,
        ..reservation(width, depth)
    }
}

/// A router over `region` with a cell body on each of `blocks`, sources and sinks included.
fn router(region: &CircuitRegionReservation, blocks: &[CellCoord]) -> Router {
    let sites: Vec<BlockSite> = blocks
        .iter()
        .enumerate()
        .map(|(index, coord)| BlockSite {
            coord: *coord,
            kind: BlockKind::Cell,
            index,
        })
        .collect();
    Router::new(region, &sites)
}

fn nothing_in_the_way() -> HashSet<CellCoord> {
    HashSet::new()
}

/// The dust one net lays with nothing in its way.
fn dust_of(router: &Router, source: CellCoord, sinks: &[CellCoord]) -> Vec<CellCoord> {
    router.dust(&router.tree(source, sinks, &nothing_in_the_way()))
}

/// The dust and the coords [`beside`] it: the obstacle set as [`net_trees`] builds it.
fn keep_out(router: &Router, source: CellCoord, sinks: &[CellCoord]) -> HashSet<CellCoord> {
    let dust = dust_of(router, source, sinks);
    dust.iter()
        .copied()
        .chain(dust.iter().copied().flat_map(beside))
        .collect()
}

/// Called directly: no producer puts a repeated net anywhere but next to itself.
#[test]
fn each_driving_net_is_charged_once_however_the_ports_are_ordered() {
    let drivers = vec![
        CellPortDriver {
            port: PortName::Sel,
            net: NetRef::Input(0),
        },
        CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(1),
        },
        CellPortDriver {
            port: PortName::B,
            net: NetRef::Input(0),
        },
    ];
    let mut charged: Vec<NetRef> = Vec::new();
    let total = sum_over_driving_nets(&drivers, |net| {
        charged.push(net);
        match net {
            NetRef::Input(0) => 10,
            NetRef::Input(1) => 3,
            other => panic!("no other net drives this cell: {other:?}"),
        }
    });
    assert_eq!(
        charged,
        vec![NetRef::Input(0), NetRef::Input(1)],
        "one call per net, in first-appearance order",
    );
    assert_eq!(total, 13, "10 for the repeated net, once, plus 3");
}

/// Every pinned `wire_length` and buffer coord in the crate is measured along this L-shape.
#[test]
fn a_clear_run_is_the_l_shape_the_passes_were_built_around() {
    let region = region(4, 3, 2);
    let source = CellCoord::new(0, 0, 0);
    let sink = CellCoord::new(2, 1, 1);
    let tree = router(&region, &[source, sink]).tree(source, &[sink], &nothing_in_the_way());
    assert_eq!(
        tree.route_to(CellCoord::new(2, 1, 1)),
        Some(vec![
            CellCoord::new(0, 0, 0),
            CellCoord::new(1, 0, 0),
            CellCoord::new(2, 0, 0),
            CellCoord::new(2, 0, 1),
            CellCoord::with_layer(2, 1, 1, RouteLayer::Bridge),
        ]),
    );
}

#[test]
fn a_clear_run_backwards_still_walks_x_before_z() {
    let region = region(4, 3, 1);
    let source = CellCoord::new(3, 0, 2);
    let sink = CellCoord::new(1, 0, 0);
    let tree = router(&region, &[source, sink]).tree(source, &[sink], &nothing_in_the_way());
    assert_eq!(
        tree.route_to(CellCoord::new(1, 0, 0)),
        Some(vec![
            CellCoord::new(3, 0, 2),
            CellCoord::new(2, 0, 2),
            CellCoord::new(1, 0, 2),
            CellCoord::new(1, 0, 1),
            CellCoord::new(1, 0, 0),
        ]),
    );
}

#[test]
fn the_wire_goes_around_a_block_rather_than_through_it() {
    let region = region(5, 3, 1);
    let source = CellCoord::new(0, 0, 0);
    let sink = CellCoord::new(2, 0, 0);
    let blocked = router(&region, &[source, sink, CellCoord::new(1, 0, 0)]);

    assert_eq!(
        blocked
            .tree(source, &[sink], &nothing_in_the_way())
            .route_to(sink),
        Some(vec![
            CellCoord::new(0, 0, 0),
            CellCoord::new(0, 0, 1),
            CellCoord::new(1, 0, 1),
            CellCoord::new(2, 0, 1),
            CellCoord::new(2, 0, 0),
        ]),
    );
    assert_eq!(
        router(&region, &[source, sink])
            .tree(source, &[sink], &nothing_in_the_way())
            .route_to(sink)
            .map(|route| route.len()),
        Some(3),
        "with nothing in the way the same pair is two blocks apart",
    );
}

/// The climb lands on the bridge layer, the one the crossing pass lifts a repeater to.
#[test]
fn a_wire_with_no_room_beside_a_block_climbs_over_it() {
    let region = region(3, 1, 2);
    let source = CellCoord::new(0, 0, 0);
    let sink = CellCoord::new(2, 0, 0);
    let tree = router(&region, &[source, sink, CellCoord::new(1, 0, 0)]).tree(
        source,
        &[sink],
        &nothing_in_the_way(),
    );
    assert_eq!(
        tree.route_to(sink),
        Some(vec![
            CellCoord::new(0, 0, 0),
            CellCoord::with_layer(0, 1, 0, RouteLayer::Bridge),
            CellCoord::with_layer(1, 1, 0, RouteLayer::Bridge),
            CellCoord::with_layer(2, 1, 0, RouteLayer::Bridge),
            CellCoord::new(2, 0, 0),
        ]),
    );
}

/// A comparator hands on its own output, not the wire that fed it, so every sink is a leaf.
#[test]
fn a_row_of_sinks_is_fed_from_beside_it_and_never_through_itself() {
    let region = region(6, 3, 1);
    let source = CellCoord::new(0, 0, 2);
    let row: Vec<CellCoord> = (0..4).map(|x| CellCoord::new(x, 0, 0)).collect();
    let tree = router(&region, &row).tree(source, &row, &nothing_in_the_way());

    for sink in &row {
        let route = tree.route_to(*sink).expect("a sink of this net");
        let through: Vec<CellCoord> = route[1..route.len() - 1]
            .iter()
            .copied()
            .filter(|coord| row.contains(coord))
            .collect();
        assert!(
            through.is_empty(),
            "the route to {sink:?} passes through {through:?}",
        );
    }
    assert_eq!(
        tree.route_to(CellCoord::new(3, 0, 0)),
        Some(vec![
            CellCoord::new(0, 0, 2),
            CellCoord::new(0, 0, 1),
            CellCoord::new(1, 0, 1),
            CellCoord::new(2, 0, 1),
            CellCoord::new(3, 0, 1),
            CellCoord::new(3, 0, 0),
        ]),
        "the far cell is reached along the trunk the nearer ones hang off",
    );
}

/// Sharing the trunk is what lets one repeater serve both sinks.
#[test]
fn a_second_sink_hangs_off_the_trunk_the_first_one_laid() {
    let region = region(8, 2, 1);
    let source = CellCoord::new(0, 0, 0);
    let near = CellCoord::new(3, 0, 1);
    let far = CellCoord::new(6, 0, 1);
    let tree = router(&region, &[near, far]).tree(source, &[far, near], &nothing_in_the_way());

    let to_near = tree.route_to(near).expect("a sink of this net");
    let to_far = tree.route_to(far).expect("a sink of this net");
    assert_eq!(
        to_far[..to_near.len() - 1],
        to_near[..to_near.len() - 1],
        "the far route leaves the near one only at the cell itself",
    );
    assert_eq!(tree.wire_path().len(), 9, "one trunk, not two strands");
}

/// Stage 4 folds `wire_path` into a per-coord map, where a coord listed twice reads as a crossing.
#[test]
fn a_tree_lists_each_of_its_coords_once() {
    let region = region(5, 3, 2);
    let source = CellCoord::new(0, 0, 1);
    let near = CellCoord::new(2, 0, 1);
    let walled = CellCoord::new(4, 0, 0);
    let blocks = [
        source,
        near,
        walled,
        CellCoord::new(3, 0, 0),
        CellCoord::new(4, 0, 1),
        CellCoord::new(4, 1, 0),
    ];
    let tree = router(&region, &blocks).tree(source, &[near, walled], &nothing_in_the_way());
    assert_eq!(
        tree.unreachable(),
        [walled],
        "the fixture only covers the stranded path while one sink is stranded",
    );

    let path = tree.wire_path();
    let mut seen: Vec<CellCoord> = path.clone();
    seen.sort_unstable_by_key(|coord| coord_key(*coord));
    seen.dedup();
    assert_eq!(seen.len(), path.len(), "a coord is listed once: {path:?}");
}

/// The tree still answers for the sink, so both projections stay total.
#[test]
fn a_walled_in_sink_is_reported_rather_than_wired_through() {
    let region = region(3, 2, 1);
    let source = CellCoord::new(0, 0, 1);
    let sink = CellCoord::new(2, 0, 0);
    let walls = [
        sink,
        source,
        CellCoord::new(1, 0, 0),
        CellCoord::new(2, 0, 1),
    ];
    let tree = router(&region, &walls).tree(source, &[sink], &nothing_in_the_way());

    assert_eq!(tree.unreachable(), [sink]);
    assert_eq!(tree.route_to(sink), Some(vec![source, sink]));
    assert!(
        tree.wire_path().contains(&sink),
        "an unreachable sink is still a coord of the net",
    );
}

#[test]
fn the_same_sink_is_reachable_once_there_is_a_layer_to_climb_to() {
    let region = region(3, 2, 2);
    let source = CellCoord::new(0, 0, 1);
    let sink = CellCoord::new(2, 0, 0);
    let walls = [
        sink,
        source,
        CellCoord::new(1, 0, 0),
        CellCoord::new(2, 0, 1),
    ];
    let tree = router(&region, &walls).tree(source, &[sink], &nothing_in_the_way());

    assert_eq!(tree.unreachable(), []);
    assert_eq!(
        tree.route_to(sink),
        Some(vec![
            source,
            CellCoord::new(0, 0, 0),
            CellCoord::with_layer(0, 1, 0, RouteLayer::Bridge),
            CellCoord::with_layer(1, 1, 0, RouteLayer::Bridge),
            CellCoord::with_layer(2, 1, 0, RouteLayer::Bridge),
            sink,
        ]),
    );
}

/// A one-layer wall between `(0,0,0)` and `(0,0,sink_z)` whose only gap, at `x = width - 1`, makes
/// the route `2 * (width - 1) + sink_z` blocks.
fn detour(width: u32, sink_z: u32) -> (Router, CellCoord, CellCoord) {
    let source = CellCoord::new(0, 0, 0);
    let sink = CellCoord::new(0, 0, sink_z);
    let mut blocks = vec![source, sink];
    blocks.extend((0..width - 1).map(|x| CellCoord::new(x, 0, 2)));
    (router(&region(width, sink_z + 1, 1), &blocks), source, sink)
}

/// The width and `sink_z`, 3 or 4 by parity, at which [`detour`] routes exactly `length` blocks.
fn detour_for(length: u32) -> (u32, u32) {
    let sink_z = 3 + (length + 1) % 2;
    (detour_width(length, sink_z), sink_z)
}

/// The width at which [`detour`] routes exactly `length` blocks round to a sink at `sink_z`.
fn detour_width(length: u32, sink_z: u32) -> u32 {
    assert_eq!((length - sink_z) % 2, 0, "{length} is the other parity");
    (length - sink_z) / 2 + 1
}

#[test]
fn a_detour_is_laid_up_to_the_cap_and_no_further() {
    let cap = MAX_ATTENUATION_SEGMENT;
    for length in [cap - 1, cap] {
        let (width, sink_z) = detour_for(length);
        let (router, source, sink) = detour(width, sink_z);
        let tree = router.tree(source, &[sink], &nothing_in_the_way());
        assert!(tree.unreachable().is_empty(), "{length} is within the cap");
        let route = tree.route_to(sink).expect("the sink is on the net");
        assert_eq!(route.len() - 1, length as usize);
    }
    for length in [cap + 1, cap + 2] {
        let (width, sink_z) = detour_for(length);
        let (router, source, sink) = detour(width, sink_z);
        let tree = router.tree(source, &[sink], &nothing_in_the_way());
        assert_eq!(tree.unreachable(), [sink], "{length} is over the cap");
        assert_eq!(tree.stranded_by(sink), Some(Unreached::BeyondCap));
    }
}

const POCKET_WIDTH: u32 = 20;

/// A sink with one free face, opening onto a pocket closed on every other side.
fn pocket(depth: u32) -> (Router, CellCoord, CellCoord) {
    let source = CellCoord::new(1, 0, 1);
    let sink = CellCoord::new(10, 0, 10);
    let blocks = [
        source,
        sink,
        CellCoord::new(9, 0, 10),
        CellCoord::new(10, 0, 9),
        CellCoord::new(10, 0, 11),
        // `(11,0,10)` is the pocket; these close it.
        CellCoord::new(12, 0, 10),
        CellCoord::new(11, 0, 9),
        CellCoord::new(11, 0, 11),
    ];
    (
        router(&region(POCKET_WIDTH, depth, 1), &blocks),
        source,
        sink,
    )
}

/// The ceiling is the `g + h <= cap` diamond in a strip [`POCKET_WIDTH`] wide.
#[test]
fn giving_up_on_a_pocket_does_not_grow_with_the_reservation() {
    let cap = MAX_ATTENUATION_SEGMENT;
    let (near, source, sink) = pocket(cap);
    let (far, _, _) = pocket(cap * 16_384);
    let (near_found, near_settled) = near.search(&[source], &[sink], &nothing_in_the_way());
    let (far_found, far_settled) = far.search(&[source], &[sink], &nothing_in_the_way());
    assert_eq!(
        (near_found, far_found),
        (Err(GaveUp::Pruned), Err(GaveUp::Pruned)),
    );
    assert_eq!(
        near_settled, far_settled,
        "sixteen thousand times the reservation, the same search",
    );
    let ceiling = POCKET_WIDTH as usize * (cap as usize / 2 + 1);
    assert!(
        far_settled <= ceiling,
        "settled {far_settled}, over the cap's diamond of {ceiling}",
    );
}

/// The verdict must not follow whether anything was pruned, which would tie it to the reservation.
#[test]
fn a_pocket_is_walled_in_whatever_the_reservation() {
    let cap = MAX_ATTENUATION_SEGMENT;
    // Small enough for the frontier to empty before the cap, then at and far past the cap.
    for (depth, gave_up) in [
        (POCKET_WIDTH, GaveUp::Exhausted),
        (cap, GaveUp::Pruned),
        (cap * 16_384, GaveUp::Pruned),
    ] {
        let (router, source, sink) = pocket(depth);
        assert_eq!(
            router.search(&[source], &[sink], &nothing_in_the_way()).0,
            Err(gave_up),
            "depth {depth}",
        );
        let tree = router.tree(source, &[sink], &nothing_in_the_way());
        assert_eq!(tree.unreachable(), [sink], "depth {depth}");
        assert_eq!(
            tree.stranded_by(sink),
            Some(Unreached::WalledIn),
            "depth {depth}",
        );
        assert_eq!(
            router.unreached(sink, &[source], &nothing_in_the_way()),
            (Unreached::WalledIn, 2),
            "the sink and its one pocket coord, at depth {depth}",
        );
    }
}

#[test]
fn the_flood_sees_a_pocket_exactly_as_deep_as_the_cap() {
    let cap = MAX_ATTENUATION_SEGMENT;
    for (length, why) in [(cap, Unreached::WalledIn), (cap + 1, Unreached::BeyondCap)] {
        let sink = CellCoord::new(0, 0, 0);
        let end = CellCoord::new(0, 0, length + 1);
        let source = CellCoord::new(0, 0, length + 2);
        let router = router(&region(1, 4 * cap, 1), &[sink, end, source]);
        let tree = router.tree(source, &[sink], &nothing_in_the_way());
        assert_eq!(tree.stranded_by(sink), Some(why), "corridor of {length}");
        let (_, seen) = router.unreached(sink, &[source], &nothing_in_the_way());
        assert_eq!(
            seen,
            cap as usize + 1,
            "the sink and the cap's worth of corridor"
        );
    }
}

#[test]
fn classifying_a_sink_beyond_the_cap_does_not_grow_with_the_reservation() {
    let cap = MAX_ATTENUATION_SEGMENT;
    let source = CellCoord::new(1, 0, 1);
    let sink = CellCoord::new(1, 0, cap + 2);
    let blocks = [source, sink];
    let near = router(&region(POCKET_WIDTH, 4 * cap, 1), &blocks);
    let far = router(&region(POCKET_WIDTH, cap * 16_384, 1), &blocks);
    // No seed is within the cap of the sink, so the flood never touches the tree.
    let (near_why, near_seen) = near.unreached(sink, &[source], &nothing_in_the_way());
    let (far_why, far_seen) = far.unreached(sink, &[source], &nothing_in_the_way());
    assert_eq!(
        (near_why, far_why),
        (Unreached::BeyondCap, Unreached::BeyondCap)
    );
    assert_eq!(near_seen, far_seen);
    assert!(
        far_seen <= POCKET_WIDTH as usize * (2 * cap as usize + 1),
        "visited {far_seen}",
    );
}

#[test]
fn a_sink_with_no_free_face_is_walled_in_without_a_search() {
    let source = CellCoord::new(1, 0, 1);
    let sink = CellCoord::new(10, 0, 10);
    let blocks = [
        source,
        sink,
        CellCoord::new(9, 0, 10),
        CellCoord::new(11, 0, 10),
        CellCoord::new(10, 0, 9),
        CellCoord::new(10, 0, 11),
    ];
    let router = router(&region(POCKET_WIDTH, 3_000, 1), &blocks);
    assert_eq!(
        router.search(&[source], &[sink], &nothing_in_the_way()),
        (Err(GaveUp::Exhausted), 0),
    );
    let tree = router.tree(source, &[sink], &nothing_in_the_way());
    assert_eq!(tree.stranded_by(sink), Some(Unreached::WalledIn));
}

/// The search's one answer must not overwrite the cause of each sink it stranded.
#[test]
fn a_sealed_sink_stays_walled_in_beside_a_sink_that_is_searched_for() {
    let cap = MAX_ATTENUATION_SEGMENT;
    let sealed = CellCoord::new(5, 0, 5);
    let walls = [
        sealed,
        CellCoord::new(4, 0, 5),
        CellCoord::new(6, 0, 5),
        CellCoord::new(5, 0, 4),
        CellCoord::new(5, 0, 6),
    ];
    // The pocket, which the flood proves walled in, and a sink beyond the cap round a wall.
    for depth in [POCKET_WIDTH, cap + 44] {
        let (pocket, source, pocketed) = pocket(depth);
        let mut blocks: Vec<CellCoord> = pocket.blocked.iter().copied().collect();
        blocks.extend(walls);
        let router = router(&region(POCKET_WIDTH, depth, 1), &blocks);
        let tree = router.tree(source, &[pocketed, sealed], &nothing_in_the_way());
        assert_eq!(tree.unreachable(), [pocketed, sealed], "depth {depth}");
        assert_eq!(tree.stranded_by(sealed), Some(Unreached::WalledIn));
        assert_eq!(tree.stranded_by(pocketed), Some(Unreached::WalledIn));
    }

    let (width, sink_z) = detour_for(cap + 1);
    let (detour, source, far) = detour(width, sink_z);
    // Two rows more, with the sealed sink against their far edge so the detour's row stays open.
    let edge = sink_z + 2;
    let sealed = CellCoord::new(width / 2, 0, edge);
    let mut blocks: Vec<CellCoord> = detour.blocked.iter().copied().collect();
    blocks.extend([
        sealed,
        CellCoord::new(width / 2 - 1, 0, edge),
        CellCoord::new(width / 2 + 1, 0, edge),
        CellCoord::new(width / 2, 0, edge - 1),
    ]);
    let region = region(width, edge + 1, 1);
    let router = router(&region, &blocks);
    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    // The sink beyond the cap first, so the net's own order would anchor on it.
    nets.insert(NetRef::Input(0), vec![far, sealed]);
    let trees = net_trees(&nets, &router, |_| source);
    let tree = &trees[&NetRef::Input(0)];
    assert_eq!(tree.unreachable(), [far, sealed]);
    assert_eq!(tree.stranded_by(far), Some(Unreached::BeyondCap));
    assert_eq!(tree.stranded_by(sealed), Some(Unreached::WalledIn));
    let d = unroutable(
        &nets,
        &trees,
        &entry_for(&nets, "s"),
        "placed",
        &region,
        |_| source,
    )
    .expect("both sinks are stranded");
    assert_eq!(d.code, DiagnosticCode::RouteCongestion, "{}", d.primary);
    assert!(
        d.primary.contains(&format!(
            "cannot reach ({},0,{edge}) from the driver at (0,0,0)",
            width / 2,
        )),
        "the sealed sink anchors the message: {}",
        d.primary,
    );
    assert!(
        d.primary
            .ends_with("; 1 more of this scope's sinks cannot be reached either"),
        "{}",
        d.primary,
    );
}

#[test]
fn a_walled_in_net_anchors_the_refusal_over_one_beyond_the_cap() {
    let cap = MAX_ATTENUATION_SEGMENT;
    let (width, sink_z) = detour_for(cap + 1);
    let (detour, source, far) = detour(width, sink_z);
    // The second net beyond the wall too, on two rows more, so the detour's row stays open.
    let edge = sink_z + 2;
    let other_source = CellCoord::new(width - 1, 0, edge);
    let sealed = CellCoord::new(width / 2, 0, edge);
    let mut blocks: Vec<CellCoord> = detour.blocked.iter().copied().collect();
    blocks.extend([
        other_source,
        sealed,
        CellCoord::new(width / 2 - 1, 0, edge),
        CellCoord::new(width / 2 + 1, 0, edge),
        CellCoord::new(width / 2, 0, edge - 1),
    ]);
    let region = region(width, edge + 1, 1);
    let router = router(&region, &blocks);
    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    nets.insert(NetRef::Input(0), vec![far]);
    nets.insert(NetRef::Input(1), vec![sealed]);
    let source_of = |net: NetRef| match net {
        NetRef::Input(0) => source,
        _ => other_source,
    };
    let trees = net_trees(&nets, &router, source_of);
    assert_eq!(
        trees[&NetRef::Input(0)].stranded_by(far),
        Some(Unreached::BeyondCap),
    );
    assert_eq!(
        trees[&NetRef::Input(1)].stranded_by(sealed),
        Some(Unreached::WalledIn),
    );
    let d = unroutable(
        &nets,
        &trees,
        &entry_for(&nets, "s"),
        "placed",
        &region,
        source_of,
    )
    .expect("both sinks are stranded");
    assert_eq!(d.code, DiagnosticCode::RouteCongestion, "{}", d.primary);
    assert!(
        d.primary.starts_with(&format!(
            "placed netlist for struct `s` cannot reach ({},0,{edge}) from the driver at \
             ({},0,{edge})",
            width / 2,
            width - 1,
        )),
        "{}",
        d.primary,
    );
    assert!(
        d.primary
            .ends_with("; 1 more of this scope's sinks cannot be reached either"),
        "{}",
        d.primary,
    );
}

/// The search leaves a seed without asking if it is free, so the tree's own face is a way in.
#[test]
fn a_sink_open_only_to_the_tree_is_not_walled_in() {
    let source = CellCoord::new(1, 0, 1);
    let sink = CellCoord::new(2, 0, 1);
    let blocks = [
        source,
        sink,
        CellCoord::new(3, 0, 1),
        CellCoord::new(2, 0, 0),
        CellCoord::new(2, 0, 2),
    ];
    let router = router(&region(5, 3, 1), &blocks);
    let (found, _) = router.search(&[source], &[sink], &nothing_in_the_way());
    assert_eq!(found, Ok((0, vec![source, sink])));
}

/// The same code the delay pass raises for a laid detour over the cap.
#[test]
fn a_sink_beyond_the_cap_is_refused_as_the_attenuation_limit() {
    let cap = MAX_ATTENUATION_SEGMENT;
    // Both sinks beyond the wall, so neither reaches the other.
    let width = cap;
    let (detour, source, sink) = detour(width, 4);
    let beside = CellCoord::new(0, 0, 3);
    let mut blocks: Vec<CellCoord> = detour.blocked.iter().copied().collect();
    blocks.push(beside);
    let region = region(width, 5, 1);
    let router = router(&region, &blocks);
    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    nets.insert(NetRef::Input(0), vec![sink, beside]);
    let trees = net_trees(&nets, &router, |_| source);
    let d = unroutable(
        &nets,
        &trees,
        &entry_for(&nets, "s"),
        "placed",
        &region,
        |_| source,
    )
    .expect("the sinks are stranded");
    assert_eq!(d.code, DiagnosticCode::AttenuationLimit);
    assert_eq!(
        d.primary,
        format!(
            "placed netlist for struct `s` has no route from the driver at (0,0,0) to \
             (0,0,4) within the v1 attenuation limit of {cap} steps; 1 more of this \
             scope's sinks cannot be reached either"
        ),
    );
    let notes: Vec<&str> = d.notes.iter().map(|n| n.message.as_str()).collect();
    assert_eq!(
        notes,
        [
            "Fix: give the wire a shorter way round — raise `void` above 1 so it has a \
             layer to climb onto, or enlarge `region=` — or split the logic across several \
             scopes, each with its own `circuit` line"
        ],
    );
}

/// A sink can be out of reach for both reasons, and the author can move the nets in the way.
#[test]
fn a_sink_beyond_the_cap_keeps_the_nets_in_the_way() {
    let cap = MAX_ATTENUATION_SEGMENT;
    let (width, sink_z) = detour_for(cap + 1);
    let (detour, source, far) = detour(width, sink_z);
    // A second net on the face above the far sink, from a source past it to a sink beside it.
    let other_source = CellCoord::new(0, 0, sink_z + 2);
    let other_sink = CellCoord::new(1, 0, sink_z + 1);
    let mut blocks: Vec<CellCoord> = detour.blocked.iter().copied().collect();
    blocks.extend([other_source, other_sink, CellCoord::new(1, 0, sink_z + 2)]);
    let region = region(width, sink_z + 3, 1);
    let router = router(&region, &blocks);
    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    nets.insert(NetRef::Input(0), vec![other_sink]);
    nets.insert(NetRef::Input(1), vec![far]);
    let source_of = |net: NetRef| match net {
        NetRef::Input(0) => other_source,
        _ => source,
    };
    let trees = net_trees(&nets, &router, source_of);
    assert_eq!(
        trees[&NetRef::Input(0)].wire_path(),
        [other_source, CellCoord::new(0, 0, sink_z + 1), other_sink],
        "the fixture lays the other net on the far sink's face",
    );
    assert_eq!(
        trees[&NetRef::Input(1)].stranded_by(far),
        Some(Unreached::BeyondCap),
    );
    let d = unroutable(
        &nets,
        &trees,
        &entry_for(&nets, "s"),
        "placed",
        &region,
        source_of,
    )
    .expect("the far sink is stranded");
    assert_eq!(d.code, DiagnosticCode::AttenuationLimit);
    assert!(
        d.primary.ends_with(&format!(
            "within the v1 attenuation limit of {cap} steps; the faces it could arrive \
             through are taken by sig.a"
        )),
        "{}",
        d.primary,
    );
}

#[test]
fn a_sink_beside_the_source_is_two_coords() {
    let region = region(4, 2, 1);
    let source = CellCoord::new(0, 0, 0);
    let sink = CellCoord::new(1, 0, 0);
    assert_eq!(
        router(&region, &[source, sink])
            .tree(source, &[sink], &nothing_in_the_way())
            .route_to(sink),
        Some(vec![source, sink]),
    );
}

#[test]
fn a_net_with_no_sinks_still_occupies_its_source() {
    let region = region(4, 2, 1);
    let source = CellCoord::new(1, 0, 1);
    let tree = router(&region, &[]).tree(source, &[], &nothing_in_the_way());
    assert_eq!(tree.wire_path(), vec![source]);
    assert_eq!(tree.route_to(source), Some(vec![source]));
}

#[test]
fn a_sink_that_is_the_source_is_a_single_coord() {
    let region = region(4, 2, 1);
    let source = CellCoord::new(3, 0, 1);
    let other = CellCoord::new(0, 0, 1);
    let tree =
        router(&region, &[source, other]).tree(source, &[source, other], &nothing_in_the_way());
    assert_eq!(tree.route_to(source), Some(vec![source]));
}

/// Callers turn `None` into a panic naming the disagreement.
#[test]
fn route_to_a_foreign_sink_is_none() {
    let region = region(6, 3, 1);
    let source = CellCoord::new(0, 0, 0);
    let sink = CellCoord::new(4, 0, 0);
    let tree = router(&region, &[source, sink]).tree(source, &[sink], &nothing_in_the_way());
    assert_eq!(tree.route_to(CellCoord::new(4, 0, 2)), None);
}

/// Every pinned coord downstream depends on which detour the router takes.
#[test]
fn equally_short_detours_are_decided_by_the_axis_order() {
    let region = region(3, 2, 2);
    let source = CellCoord::new(0, 0, 0);
    let sink = CellCoord::new(2, 0, 0);
    let tree = router(&region, &[source, sink, CellCoord::new(1, 0, 0)]).tree(
        source,
        &[sink],
        &nothing_in_the_way(),
    );
    let route = tree.route_to(sink).expect("a sink of this net");
    assert_eq!(route.len(), 5, "both ways round cost the same two blocks");
    assert_eq!(
        route[1],
        CellCoord::new(0, 0, 1),
        "z comes before y, so the wire goes round rather than over: {route:?}",
    );
}

/// A tie would leave the wire alone but move the routes through it, along which every
/// `wire_length` and buffer coord is measured; no other fixture tells the two apart.
#[test]
fn a_coord_reached_twice_for_one_price_keeps_the_first_parent() {
    let region = region(6, 4, 1);
    let source = CellCoord::new(3, 0, 2);
    let sinks = [
        CellCoord::new(4, 0, 1),
        CellCoord::new(1, 0, 0),
        CellCoord::new(3, 0, 1),
        CellCoord::new(4, 0, 2),
    ];
    let mut blocks = vec![source];
    blocks.extend(sinks);
    let tree = router(&region, &blocks).tree(source, &sinks, &nothing_in_the_way());
    assert_eq!(
        tree.route_to(CellCoord::new(4, 0, 1)),
        Some(vec![
            CellCoord::new(3, 0, 2),
            CellCoord::new(2, 0, 2),
            CellCoord::new(1, 0, 2),
            CellCoord::new(1, 0, 1),
            CellCoord::new(2, 0, 1),
            CellCoord::new(2, 0, 0),
            CellCoord::new(3, 0, 0),
            CellCoord::new(4, 0, 0),
            CellCoord::new(4, 0, 1),
        ]),
    );
}

#[test]
fn the_order_the_sinks_are_asked_for_does_not_change_the_wire() {
    let region = region(8, 4, 2);
    let source = CellCoord::new(0, 0, 3);
    let sinks = [
        CellCoord::new(5, 0, 0),
        CellCoord::new(1, 0, 0),
        CellCoord::new(3, 0, 2),
    ];
    let router = router(&region, &sinks);
    let forward = router.tree(source, &sinks, &nothing_in_the_way());
    let mut backward_sinks = sinks;
    backward_sinks.reverse();
    assert_eq!(
        forward,
        router.tree(source, &backward_sinks, &nothing_in_the_way())
    );
}

/// Two strands one step apart are one strand carrying two signals; `void=1` leaves no way over.
#[test]
fn a_net_does_not_run_in_the_lane_beside_another_nets_dust() {
    let region = region(5, 4, 1);
    let first_source = CellCoord::new(0, 0, 0);
    let first_sink = CellCoord::new(4, 0, 0);
    let second_source = CellCoord::new(0, 0, 1);
    let second_sink = CellCoord::new(4, 0, 1);
    let router = router(
        &region,
        &[first_source, first_sink, second_source, second_sink],
    );

    let lane = CellCoord::new(2, 0, 1);
    let first = dust_of(&router, first_source, &[first_sink]);
    assert!(
        !first.contains(&lane),
        "the fixture needs the second net's own row clear of the first's \
         dust, so that only the one-step rule keeps it off: {first:?}",
    );

    let tree = router.tree(
        second_source,
        &[second_sink],
        &keep_out(&router, first_source, &[first_sink]),
    );
    let second = router.dust(&tree);
    assert!(
        tree.unreachable().is_empty(),
        "the second net still gets there: {:?}",
        tree.unreachable(),
    );
    assert!(
        !second.contains(&lane),
        "but not down the lane beside the first: {second:?}",
    );
    assert!(
        second
            .iter()
            .all(|coord| first.iter().all(|other| manhattan(*coord, *other) > 1)),
        "and no coord of one is a step from a coord of the other: {second:?}",
    );
}

/// So [`net_trees`] widens the obstacle set once a net is laid, not as each coord goes in.
#[test]
fn a_net_is_not_kept_out_by_its_own_dust() {
    // One row and one layer, so the keep-out set has nowhere to send the net.
    let region = region(5, 1, 1);
    let source = CellCoord::new(0, 0, 0);
    let sink = CellCoord::new(4, 0, 0);
    let router = router(&region, &[source, sink]);

    let dust = dust_of(&router, source, &[sink]);
    assert!(
        dust.windows(2).any(|pair| manhattan(pair[0], pair[1]) == 1),
        "a strand runs beside itself at every step: {dust:?}",
    );
    let mine = keep_out(&router, source, &[sink]);
    assert_eq!(
        router.tree(source, &[sink], &mine).unreachable(),
        [sink],
        "so a net routed against its own keep-out set has nowhere to go",
    );

    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    nets.insert(NetRef::Input(0), vec![sink]);
    let trees = net_trees(&nets, &router, |_| source);
    assert!(
        trees[&NetRef::Input(0)].unreachable().is_empty(),
        "and the pass that widens the set for the nets after this one \
         does not turn it on the net it has just laid",
    );
}

/// A face is unusable when another net's dust is on it or [`beside`] it, so the refusal names
/// nets two steps from the sink.
#[test]
fn a_net_that_takes_a_face_without_standing_on_it_is_named() {
    let region = region(5, 3, 1);
    let sink = CellCoord::new(2, 0, 0);
    let driver = CellCoord::new(0, 0, 0);
    let walls = [CellCoord::new(1, 0, 0), CellCoord::new(3, 0, 0)];
    let other_source = CellCoord::new(0, 0, 2);
    let other_sink = CellCoord::new(4, 0, 2);
    let router = router(
        &region,
        &[sink, driver, walls[0], walls[1], other_source, other_sink],
    );

    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    nets.insert(NetRef::Input(0), vec![other_sink]);
    nets.insert(NetRef::Cell(0), vec![sink]);
    let source_of_net = |net: NetRef| match net {
        NetRef::Input(_) => other_source,
        NetRef::Cell(_) => driver,
    };
    let trees = net_trees(&nets, &router, source_of_net);

    let far_row = router.dust(&trees[&NetRef::Input(0)]);
    assert!(
        far_row.contains(&CellCoord::new(2, 0, 2)),
        "the fixture needs the first net one step from the sink's last \
         free face: {far_row:?}",
    );
    assert!(
        far_row.iter().all(|coord| manhattan(*coord, sink) > 1),
        "and on none of its faces, so only the widening can name it: \
         {far_row:?}",
    );
    assert_eq!(
        trees[&NetRef::Cell(0)].unreachable(),
        [sink],
        "and the driver stranded, so there is a refusal to word",
    );

    assert_eq!(
        crowding_nets(&nets, &trees, &source_of_net, NetRef::Cell(0), sink),
        vec![NetRef::Input(0)],
    );
}

/// Separating them is the physical tile layer's obligation (`spec/redstone` "Place-and-route"),
/// which is why [`IN_PLANE`] has four steps, not six. The pairs are counted by hand as well as by
/// [`tile_layer_pairs`], since a list checked against itself agrees with any rule.
#[test]
fn two_strands_one_layer_apart_are_named_rather_than_kept_apart() {
    let region = region(3, 3, 2);
    let source = CellCoord::new(1, 0, 0);
    let sink = CellCoord::new(1, 0, 2);
    let wall_source = CellCoord::new(0, 0, 1);
    let wall_sink = CellCoord::new(2, 0, 1);
    let router = router(&region, &[source, sink, wall_source, wall_sink]);

    // Through `net_trees`, so the wire is the routing pass's: equal fanout puts `Input(0)` first.
    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    nets.insert(NetRef::Input(0), vec![wall_sink]);
    nets.insert(NetRef::Cell(0), vec![sink]);
    let trees = net_trees(&nets, &router, |net| match net {
        NetRef::Input(_) => wall_source,
        NetRef::Cell(_) => source,
    });
    let wall = router.dust(&trees[&NetRef::Input(0)]);
    let climbed = router.dust(&trees[&NetRef::Cell(0)]);

    let mut stacked = 0usize;
    let mut staircase = 0usize;
    for over in &climbed {
        for under in &wall {
            if over.y.abs_diff(under.y) != 1 {
                continue;
            }
            match over.x.abs_diff(under.x) + over.z.abs_diff(under.z) {
                0 => stacked += 1,
                1 => staircase += 1,
                _ => {}
            }
        }
    }
    assert_eq!(
        (stacked, staircase),
        (1, 2),
        "the escape lands over the strand it cleared and beside it \
         twice: {climbed:?} over {wall:?}",
    );

    let pairs = tile_layer_pairs(&nets, &trees, &router);
    assert_eq!(
        (
            pairs.iter().filter(|pair| pair.stacked()).count(),
            pairs.iter().filter(|pair| !pair.stacked()).count(),
        ),
        (stacked, staircase),
        "and the list handed to the tile layer is those same pairs: {pairs:?}",
    );
    assert!(
        pairs
            .iter()
            .all(|pair| pair.over.0.y.abs_diff(pair.under.0.y) == 1
                && pair.over.0.y > pair.under.0.y),
        "each recorded once, upper coord first: {pairs:?}",
    );
    assert!(
        pairs
            .iter()
            .all(|pair| pair.over.1 == NetRef::Cell(0) && pair.under.1 == NetRef::Input(0)),
        "and the net that climbed is the upper of every pair, which is what \
         a message naming them the other way round would get wrong: {pairs:?}",
    );

    let entry = entry_for(&nets, "climb");
    let finding = tile_layer_clearance(&nets, &trees, &router, &entry, &region)
        .expect("three pairs are a finding");
    assert_eq!(finding.code, DiagnosticCode::RouteCrossLayerClearance);
    assert!(
        finding
            .primary
            .contains("leaves 3 pairs of dust within one step of each other across layers (1 stacked, 2 staircase)"),
        "the primary counts both shapes: {}",
        finding.primary,
    );
    let notes: Vec<&str> = finding.notes.iter().map(|n| n.message.as_str()).collect();
    assert_eq!(
        notes[..3],
        [
            "(1,1,1) on cell #0 stands directly over (1,0,1) on sig.a",
            "(1,1,0) on cell #0 stands a layer over, and one step across from, (1,0,1) on sig.a",
            "(1,1,2) on cell #0 stands a layer over, and one step across from, (1,0,1) on sig.a",
        ],
        "every pair named, upper coord and climbing net first: {notes:?}",
    );
    assert!(
        !notes.iter().any(|note| note.starts_with("and ")),
        "and nothing tallied, because nothing was left out — a finding that \
         named `and 0 more` would be counting the cap rather than the \
         pairs: {notes:?}",
    );
}

/// A scope entry a finding can name nets out of; [`net_label`] reads only its input row.
fn entry_for(nets: &HashMap<NetRef, Vec<CellCoord>>, name: &str) -> ScopedPlacementIrEntry {
    use cairn_lang_core::Edition;
    use cairn_lang_core::ast::DottedRef;

    use crate::logic_ir::ScopeKind;
    use crate::netlist_ir::NetlistInput;

    let mut ir = PlacementIr::new(Edition::Java);
    // One input per net, more than any of them drives; `net_label` reads the row by index.
    for letter in "abcdefghijklmnopqrstuvwxyz".chars().take(nets.len()) {
        ir.inputs.push(NetlistInput {
            name: DottedRef::new("sig".into(), vec![letter.to_string()]),
            span: Span::default(),
        });
    }
    ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: name.to_owned(),
        ir,
    }
}

/// Every climb puts a net's own coord under another, so counting those would bury the real pairs.
#[test]
fn a_strand_that_climbs_over_its_own_dust_is_not_a_pair() {
    // One row deep, so the way past the block is up and back down.
    let region = region(5, 1, 2);
    let source = CellCoord::new(0, 0, 0);
    let sink = CellCoord::new(4, 0, 0);
    let wall = CellCoord::new(2, 0, 0);
    let router = router(&region, &[source, sink, wall]);

    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    nets.insert(NetRef::Input(0), vec![sink]);
    let trees = net_trees(&nets, &router, |_| source);
    let dust = router.dust(&trees[&NetRef::Input(0)]);
    assert!(
        dust.iter().any(|coord| coord.y > 0),
        "the fixture needs the net to climb: {dust:?}",
    );
    assert!(
        dust.iter()
            .any(|over| dust.iter().any(|under| over.y.abs_diff(under.y) == 1
                && over.x.abs_diff(under.x) + over.z.abs_diff(under.z) <= 1)),
        "and to pass within one step of itself across layers, or there is \
         nothing here for the rule to exclude: {dust:?}",
    );

    assert_eq!(
        tile_layer_pairs(&nets, &trees, &router),
        vec![],
        "one net's own climb is the wire: {dust:?}",
    );
}

/// The shortcut for an unobstructed run needs the obstacle set the search gets.
#[test]
fn a_straight_run_does_not_cross_the_dust_already_laid() {
    // `void=1`, so the way round is on the plane or nowhere.
    let region = region(5, 3, 1);
    let source = CellCoord::new(2, 0, 0);
    let sink = CellCoord::new(2, 0, 2);
    let other_source = CellCoord::new(0, 0, 1);
    let other_sink = CellCoord::new(3, 0, 1);
    let router = router(&region, &[source, sink, other_source, other_sink]);

    assert_eq!(
        router
            .tree(source, &[sink], &nothing_in_the_way())
            .wire_path(),
        vec![source, CellCoord::new(2, 0, 1), sink],
        "with nothing in the way the run is the straight line",
    );

    assert!(
        dust_of(&router, other_source, &[other_sink]).contains(&CellCoord::new(2, 0, 1)),
        "the fixture needs the first net across the second's line",
    );
    let across = keep_out(&router, other_source, &[other_sink]);
    let tree = router.tree(source, &[sink], &across);
    assert!(
        tree.unreachable().is_empty(),
        "the second net still gets there: {:?}",
        tree.unreachable(),
    );
    assert!(
        router
            .dust(&tree)
            .iter()
            .all(|coord| !across.contains(coord)),
        "by going round rather than over, and keeping a coord clear \
         between the two: {:?}",
        router.dust(&tree),
    );
}

/// An escape in `spec/redstone` "Place-and-route". The assertion spells the layer rather than
/// reading it off the height as the constructor does.
#[test]
fn a_net_with_no_way_round_climbs_over_the_dust_in_its_way() {
    let region = region(3, 3, 2);
    let source = CellCoord::new(1, 0, 0);
    let sink = CellCoord::new(1, 0, 2);
    let wall_source = CellCoord::new(0, 0, 1);
    let wall_sink = CellCoord::new(2, 0, 1);
    let router = router(&region, &[source, sink, wall_source, wall_sink]);

    assert_eq!(
        dust_of(&router, wall_source, &[wall_sink]),
        vec![CellCoord::new(1, 0, 1)],
        "the fixture needs the row between the two sealed",
    );
    let wall = keep_out(&router, wall_source, &[wall_sink]);
    let tree = router.tree(source, &[sink], &wall);
    assert_eq!(
        tree.route_to(sink).expect("a sink of this net"),
        vec![
            source,
            CellCoord::with_layer(1, 1, 0, RouteLayer::Bridge),
            CellCoord::with_layer(1, 1, 1, RouteLayer::Bridge),
            CellCoord::with_layer(1, 1, 2, RouteLayer::Bridge),
            sink,
        ],
        "the escape is a climb, a run along the layer above, and a drop",
    );
}

/// A cell body is a terminal of every net driving it, and the obstacle set is dust, not terminals.
#[test]
fn a_second_driver_of_one_cell_still_arrives_at_it() {
    let region = region(4, 3, 2);
    let cell = CellCoord::new(2, 0, 0);
    let first = CellCoord::new(0, 0, 0);
    let second = CellCoord::new(0, 0, 2);
    let router = router(&region, &[cell, first, second]);

    let dust = dust_of(&router, first, &[cell]);
    assert!(
        !dust.contains(&cell),
        "the cell body is a terminal, not dust: {dust:?}",
    );
    let tree = router.tree(second, &[cell], &keep_out(&router, first, &[cell]));
    assert!(
        tree.unreachable().is_empty(),
        "the second driver has to reach the cell it drives: {:?}",
        tree.unreachable(),
    );
}

#[test]
fn a_net_with_nowhere_left_to_go_is_stranded_rather_than_shorted() {
    let region = region(3, 3, 1);
    let source = CellCoord::new(1, 0, 0);
    let sink = CellCoord::new(1, 0, 2);
    let wall_source = CellCoord::new(0, 0, 1);
    let wall_sink = CellCoord::new(2, 0, 1);
    let router = router(&region, &[source, sink, wall_source, wall_sink]);

    let wall = keep_out(&router, wall_source, &[wall_sink]);
    let tree = router.tree(source, &[sink], &wall);
    assert_eq!(
        tree.unreachable(),
        [sink],
        "with no layer above the plane there is nowhere to escape to",
    );
    assert!(
        router.dust(&tree).iter().all(|coord| !wall.contains(coord)),
        "and the answer is not to lay it over the wire in the way: {:?}",
        tree.wire_path(),
    );
}

/// Pinned because the three passes that call [`net_trees`] each rebuild the trees and must agree.
#[test]
fn the_wider_fanout_is_laid_first_and_keeps_its_run() {
    let region = region(5, 3, 2);
    let wide_source = CellCoord::new(0, 0, 1);
    let narrow_source = CellCoord::new(4, 0, 1);
    let near = CellCoord::new(2, 0, 0);
    let far = CellCoord::new(2, 0, 2);
    let blocks = [wide_source, narrow_source, near, far];
    let router = router(&region, &blocks);

    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    nets.insert(NetRef::Input(0), vec![near, far]);
    nets.insert(NetRef::Cell(0), vec![near]);
    let trees = net_trees(&nets, &router, |net| match net {
        NetRef::Input(0) => wide_source,
        _ => narrow_source,
    });

    let wide: HashSet<CellCoord> = router.dust(&trees[&NetRef::Input(0)]).into_iter().collect();
    let narrow: HashSet<CellCoord> = router.dust(&trees[&NetRef::Cell(0)]).into_iter().collect();
    assert!(
        wide.contains(&CellCoord::new(1, 0, 1)) && wide.contains(&CellCoord::new(2, 0, 1)),
        "the two-sink net is laid first and takes the middle row: {wide:?}",
    );
    assert!(
        wide.is_disjoint(&narrow),
        "and the one-sink net goes round it: {wide:?} vs {narrow:?}",
    );
}

/// A path this router laid runs through no block and ends at one, so its blocks are its terminals.
#[test]
fn the_dust_of_a_tree_is_everything_but_its_terminals() {
    let region = region(4, 2, 1);
    let source = CellCoord::new(0, 0, 0);
    let sink = CellCoord::new(3, 0, 0);
    let router = router(&region, &[source, sink]);
    let tree = router.tree(source, &[sink], &nothing_in_the_way());
    assert_eq!(
        router.dust(&tree),
        vec![CellCoord::new(1, 0, 0), CellCoord::new(2, 0, 0)],
        "the two ends are blocks; what is between them is dust",
    );
}

#[test]
fn collect_nets_maps_cell_drivers_to_bodies_and_outputs_to_pads() {
    let region = reservation(8, 4);
    let ir = one_cell_ir(&region);

    let nets = collect_nets(&ir);
    assert_eq!(nets[&NetRef::Input(0)], vec![CellCoord::new(2, 0, 1)]);
    assert_eq!(
        nets[&NetRef::Cell(0)],
        vec![output_pad(0, PadColumn::for_cell_count(1), &region)]
    );
}

/// The list's length is the fanout [`net_order`] sorts on, so a duplicate would reorder the nets.
#[test]
fn two_ports_of_one_cell_on_one_net_are_one_sink() {
    let region = reservation(8, 4);
    let mut ir = one_cell_ir(&region);
    ir.cells[0].drivers.push(CellPortDriver {
        port: PortName::B,
        net: NetRef::Input(0),
    });

    let nets = collect_nets(&ir);
    assert_eq!(
        nets[&NetRef::Input(0)],
        vec![CellCoord::new(2, 0, 1)],
        "one cell body, however many of its ports read the net",
    );
    assert_eq!(
        net_order(&nets),
        vec![NetRef::Input(0), NetRef::Cell(0)],
        "and one apiece, so the tie is broken by the key rather than by a \
         fanout one of them does not have",
    );
}

/// Only a sink outside the reservation, which a hand-built IR can carry, can have the net's own
/// dust beside it.
#[test]
fn a_stranded_net_is_not_named_as_crowding_its_own_sink() {
    let region = region(4, 3, 1);
    let own_source = CellCoord::new(0, 0, 0);
    let own_sink = CellCoord::new(3, 0, 1);
    let past_the_edge = CellCoord::new(4, 0, 0);
    let other_source = CellCoord::new(0, 0, 2);
    let other_sink = CellCoord::new(2, 0, 2);
    let router = router(
        &region,
        &[
            own_source,
            own_sink,
            past_the_edge,
            other_source,
            other_sink,
        ],
    );

    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    nets.insert(NetRef::Input(0), vec![other_sink]);
    nets.insert(NetRef::Cell(0), vec![own_sink, past_the_edge]);
    let source_of_net = |net: NetRef| match net {
        NetRef::Input(_) => other_source,
        NetRef::Cell(_) => own_source,
    };
    let trees = net_trees(&nets, &router, source_of_net);

    assert_eq!(
        trees[&NetRef::Cell(0)].unreachable(),
        [past_the_edge],
        "the fixture needs the sink past the edge stranded",
    );
    assert!(
        router
            .dust(&trees[&NetRef::Cell(0)])
            .contains(&CellCoord::new(3, 0, 0)),
        "and its own dust beside that sink",
    );

    assert_eq!(
        crowding_nets(
            &nets,
            &trees,
            &source_of_net,
            NetRef::Cell(0),
            past_the_edge,
        ),
        Vec::<NetRef>::new(),
        "a net does not stand in its own way",
    );
}

/// One list, so the three passes cannot disagree about what is standing where.
#[test]
fn block_sites_lists_cells_then_input_pads_then_output_pads() {
    let region = reservation(8, 4);
    let ir = one_cell_ir(&region);

    let sites = block_sites(&ir, &region);
    assert_eq!(
        sites
            .iter()
            .map(|site| (site.kind, site.index, site.coord))
            .collect::<Vec<_>>(),
        vec![
            (BlockKind::Cell, 0, CellCoord::new(2, 0, 1)),
            (BlockKind::InputPad, 0, CellCoord::new(0, 0, 0)),
            (BlockKind::OutputPad, 0, CellCoord::new(7, 0, 0)),
        ],
    );
}

/// One cell reading input 0, driving one actuator pad.
fn one_cell_ir(region: &CircuitRegionReservation) -> PlacementIr {
    use cairn_lang_core::Edition;
    use cairn_lang_core::ast::DottedRef;

    use crate::edition_netlist_ir::EditionCell;
    use crate::netlist_ir::NetlistInput;
    use crate::placement_ir::{PlacedCellNode, PlacedOutputNode, PlacementPhase};

    let mut ir = PlacementIr::new(Edition::Java);
    ir.inputs.push(NetlistInput {
        name: DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
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
    ir.outputs.push(PlacedOutputNode::new(
        DottedRef::new("sig".into(), vec!["out".into()]),
        NetRef::Cell(0),
        output_pad(0, PadColumn::for_cell_count(1), region),
        Span::default(),
    ));
    ir
}

/// Pinned because three passes walk it, and a `HashMap`'s order would differ run to run.
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

mod route_invariants {
    //! Properties every consumer of [`NetTree`] leans on, over layouts the placement pass never
    //! produces.
    use proptest::prelude::*;

    use super::{
        BlockKind, BlockSite, CellCoord, CircuitRegionReservation, HashMap, HashSet, NetRef,
        Router, Span, beside, manhattan, net_trees, nothing_in_the_way,
    };

    const WIDTH: u32 = 9;
    const DEPTH: u32 = 4;

    /// A source, four sinks and six blocks at most in a 9x2x4 box, small enough to wall things in.
    fn layout() -> impl Strategy<Value = (u32, CellCoord, Vec<CellCoord>, Vec<CellCoord>)> {
        let coord = (0u32..WIDTH, 0u32..DEPTH).prop_map(|(x, z)| CellCoord::new(x, 0, z));
        (
            1u32..=2,
            coord.clone(),
            prop::collection::vec(coord.clone(), 1..=4),
            prop::collection::vec(coord, 0..=6),
        )
    }

    fn routed(
        void: u32,
        source: CellCoord,
        sinks: &[CellCoord],
        loose: &[CellCoord],
    ) -> (Router, HashSet<CellCoord>) {
        let region = CircuitRegionReservation {
            label: "floor".to_owned(),
            void,
            width: WIDTH,
            depth: DEPTH,
            span: Span::default(),
        };
        let mut blocks: Vec<CellCoord> = vec![source];
        blocks.extend(sinks.iter().copied());
        blocks.extend(loose.iter().copied());
        let sites: Vec<BlockSite> = blocks
            .iter()
            .enumerate()
            .map(|(index, coord)| BlockSite {
                coord: *coord,
                kind: BlockKind::Cell,
                index,
            })
            .collect();
        (
            Router::new(&region, &sites),
            blocks.iter().copied().collect(),
        )
    }

    /// The fewest steps from any seed to any target over free space, by breadth-first sweep.
    fn breadth_first(router: &Router, seeds: &[CellCoord], targets: &[CellCoord]) -> Option<u32> {
        let mut seen: HashSet<CellCoord> = seeds.iter().copied().collect();
        let mut frontier: Vec<CellCoord> = seeds.to_vec();
        let mut distance = 0u32;
        while !frontier.is_empty() {
            let mut next: Vec<CellCoord> = Vec::new();
            for coord in frontier {
                for delta in super::STEPS {
                    let Some(step) = router.step(coord, delta) else {
                        continue;
                    };
                    if targets.contains(&step) {
                        return Some(distance + 1);
                    }
                    if !router.free(step, &nothing_in_the_way()) || !seen.insert(step) {
                        continue;
                    }
                    next.push(step);
                }
            }
            distance += 1;
            frontier = next;
        }
        None
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

        #[test]
        fn a_route_is_a_walk_along_the_wire_it_belongs_to(
            (void, source, sinks, loose) in layout()
        ) {
            let (router, _) = routed(void, source, &sinks, &loose);
            let tree = router.tree(source, &sinks, &nothing_in_the_way());
            let owned: HashSet<CellCoord> = tree.wire_path().into_iter().collect();
            for sink in &sinks {
                let route = tree.route_to(*sink).expect("a sink of this net");
                prop_assert_eq!(route.first().copied(), Some(source));
                prop_assert_eq!(route.last().copied(), Some(*sink));
                for coord in &route {
                    prop_assert!(
                        owned.contains(coord),
                        "route to {:?} visits {:?}, which the net does not own",
                        sink,
                        coord,
                    );
                }
                if tree.unreachable().contains(sink) {
                    continue;
                }
                for pair in route.windows(2) {
                    prop_assert_eq!(manhattan(pair[0], pair[1]), 1);
                }
            }
        }

        /// The breadth-first walk is the oracle, driven through [`Router::reach`] so the real
        /// tier choice is what gets measured.
        #[test]
        fn every_attachment_is_as_short_as_a_breadth_first_walk(
            (void, source, sinks, loose) in layout()
        ) {
            let (router, _) = routed(void, source, &sinks, &loose);
            let mut seeds = vec![source];
            let mut targets: Vec<CellCoord> = Vec::new();
            for sink in &sinks {
                if *sink != source && !targets.contains(sink) {
                    targets.push(*sink);
                }
            }
            prop_assume!(!targets.is_empty());
            while !targets.is_empty() {
                let reference = breadth_first(&router, &seeds, &targets);
                let Ok((index, path)) = router.reach(&seeds, &targets, &nothing_in_the_way()) else {
                    prop_assert_eq!(
                        reference,
                        None,
                        "the router gave up where a plain walk gets through",
                    );
                    break;
                };
                let walked = u32::try_from(path.len() - 1).expect("a path fits in u32");
                prop_assert_eq!(Some(walked), reference, "path {:?}", path);
                let searched = router
                    .search(&seeds, &targets, &nothing_in_the_way())
                    .0
                    .expect("a path exists, so the search has to find one");
                prop_assert_eq!(
                    searched.1.len(),
                    path.len(),
                    "search {:?} against the tier that answered {:?}",
                    searched.1,
                    path,
                );
                targets.remove(index);
                seeds.extend(path[1..path.len() - 1].iter().copied());
            }
        }

        /// Up and down are left to the physical tile layer (`spec/redstone` "Place-and-route").
        /// Three nets, so the third must avoid the union of two; too few blocks is a reject.
        #[test]
        fn no_two_nets_of_a_scope_run_within_one_step_in_one_plane(
            (void, source, sinks, loose) in layout()
        ) {
            prop_assume!(loose.len() >= 3);
            let (router, _) = routed(void, source, &sinks, &loose);
            let split = 2 + (loose.len() - 2) / 2;
            let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
            nets.insert(NetRef::Input(0), sinks.clone());
            nets.insert(NetRef::Cell(0), loose[2..split].to_vec());
            nets.insert(NetRef::Cell(1), loose[split..].to_vec());
            let trees = net_trees(&nets, &router, |net| match net {
                NetRef::Input(_) => source,
                NetRef::Cell(0) => loose[0],
                NetRef::Cell(_) => loose[1],
            });
            let mut seen: HashMap<CellCoord, NetRef> = HashMap::new();
            for (net, tree) in &trees {
                for coord in router.dust(tree) {
                    for taken in std::iter::once(coord).chain(beside(coord)) {
                        if let Some(other) = seen.get(&taken)
                            && other != net
                        {
                            return Err(TestCaseError::fail(format!(
                                "{net:?} lays dust on {coord:?}, which is \
                                 {taken:?} or beside it, and {other:?} \
                                 already has it",
                            )));
                        }
                    }
                }
                for coord in router.dust(tree) {
                    seen.insert(coord, *net);
                }
            }
        }

        #[test]
        fn no_route_passes_through_a_block(
            (void, source, sinks, loose) in layout()
        ) {
            let (router, blocks) = routed(void, source, &sinks, &loose);
            let tree = router.tree(source, &sinks, &nothing_in_the_way());
            for sink in &sinks {
                if tree.unreachable().contains(sink) {
                    continue;
                }
                let route = tree.route_to(*sink).expect("a sink of this net");
                let interior = route.get(1..route.len().saturating_sub(1)).unwrap_or_default();
                for coord in interior {
                    prop_assert!(
                        !blocks.contains(coord),
                        "the route to {:?} runs through {:?}",
                        sink,
                        coord,
                    );
                }
            }
        }
    }
}

#[test]
fn a_block_on_a_taken_coord_is_found_and_a_clear_layout_is_not() {
    let clear = [
        site(BlockKind::Cell, 0, CellCoord::new(1, 0, 1)),
        site(BlockKind::InputPad, 0, CellCoord::new(0, 0, 0)),
        site(BlockKind::OutputPad, 0, CellCoord::new(3, 0, 0)),
    ];
    assert!(
        collapsed_block(&clear).is_none(),
        "three coords, three blocks",
    );

    let collapsed = [
        site(BlockKind::Cell, 0, CellCoord::new(1, 0, 1)),
        site(BlockKind::InputPad, 0, CellCoord::new(0, 0, 0)),
        site(BlockKind::OutputPad, 0, CellCoord::new(1, 0, 1)),
    ];
    let found = collapsed_block(&collapsed).expect("the pad on the cell");
    assert_eq!(
        (found.kind, found.index),
        (BlockKind::OutputPad, 0),
        "the block that landed second is the one that could not fit",
    );
}

/// No `size=` repairs it, so there is no diagnostic for an author to act on.
#[test]
#[should_panic(expected = "two cells stand on (1,0,1)")]
fn two_cells_on_one_coord_is_a_caller_bug_and_says_so() {
    let two_cells = [
        site(BlockKind::Cell, 0, CellCoord::new(1, 0, 1)),
        site(BlockKind::Cell, 1, CellCoord::new(1, 0, 1)),
    ];
    let _ = collapsed_block(&two_cells);
}

/// Reporting the pad would ask for a `size=` that cannot fix the IR.
#[test]
#[should_panic(expected = "two cells stand on (1,0,1)")]
fn a_cell_fault_outranks_a_pad_fault_later_in_the_list() {
    let both = [
        site(BlockKind::Cell, 0, CellCoord::new(1, 0, 1)),
        site(BlockKind::Cell, 1, CellCoord::new(1, 0, 1)),
        site(BlockKind::InputPad, 0, CellCoord::new(1, 0, 1)),
    ];
    let _ = collapsed_block(&both);
}

#[test]
fn a_pad_on_a_single_cell_is_still_the_pad_refusal() {
    let cell_then_pad = [
        site(BlockKind::Cell, 0, CellCoord::new(1, 0, 1)),
        site(BlockKind::InputPad, 0, CellCoord::new(1, 0, 1)),
    ];
    assert_eq!(
        collapsed_block(&cell_then_pad).map(|s| s.kind),
        Some(BlockKind::InputPad),
    );
}

/// A struct literal can write `(x, 0, z, Bridge)` where [`CellCoord::new`] derives `Plane`, and
/// [`Router::new`] keys both through [`keyed`] as one voxel.
#[test]
fn a_coord_is_read_the_way_the_router_keys_it() {
    let mislabelled = CellCoord {
        x: 2,
        y: 0,
        z: 1,
        layer: RouteLayer::Bridge,
    };
    assert_ne!(
        mislabelled,
        CellCoord::new(2, 0, 1),
        "the premise: raw equality calls these two coords",
    );

    let pair = [
        site(BlockKind::Cell, 0, CellCoord::new(2, 0, 1)),
        site(BlockKind::OutputPad, 0, mislabelled),
    ];
    assert_eq!(
        collapsed_block(&pair).map(|s| s.kind),
        Some(BlockKind::OutputPad),
        "one voxel, one obstacle to the router, so one collision here",
    );

    let apart = [
        site(BlockKind::Cell, 0, CellCoord::new(2, 0, 1)),
        site(BlockKind::OutputPad, 0, CellCoord::new(2, 1, 1)),
    ];
    assert!(
        collapsed_block(&apart).is_none(),
        "a block genuinely a layer up is not standing on the one below it",
    );
}

/// A block at `coord`, as a struct literal since these rows need coords no IR produces.
fn site(kind: BlockKind, index: usize, coord: CellCoord) -> BlockSite {
    BlockSite { coord, kind, index }
}
