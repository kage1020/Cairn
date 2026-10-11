use super::*;

/// Manhattan distance between two ground-plane cells — the minimal
/// possible number of steps, so `route_path` output length can be
/// asserted against `manhattan + 1` cells when no detour is needed.
fn manhattan(a: (i32, i32, i32), b: (i32, i32, i32)) -> usize {
    usize::try_from((a.0 - b.0).abs() + (a.2 - b.2).abs()).expect("non-negative")
}

/// Structural invariants every successful route must satisfy: the
/// endpoints are the requested ports, consecutive cells are
/// 4-neighbour adjacent at a constant Y, no cell repeats, and no
/// cell collides with `blocked`.
fn assert_route_shape(
    path: &[(i32, i32, i32)],
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    blocked: &HashSet<(i32, i32, i32)>,
) {
    assert_eq!(path.first(), Some(&from), "route must start at `from`");
    assert_eq!(path.last(), Some(&to), "route must end at `to`");
    for pair in path.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        assert_eq!(a.1, b.1, "route must stay at a constant Y: {a:?} -> {b:?}");
        assert_eq!(
            (a.0 - b.0).abs() + (a.2 - b.2).abs(),
            1,
            "route cells must be 4-neighbour adjacent: {a:?} -> {b:?}",
        );
    }
    let mut seen = HashSet::new();
    for cell in path {
        assert!(seen.insert(*cell), "route revisits cell {cell:?}");
        assert!(
            !blocked.contains(cell),
            "route crosses blocked cell {cell:?}"
        );
    }
}

/// Index-and-route shorthand so each test reads as `(from, to,
/// blocked)` without repeating the [`BlockedIndex`] construction.
fn route(
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    blocked: &HashSet<(i32, i32, i32)>,
) -> Result<Vec<(i32, i32, i32)>, RoutePathError> {
    route_path(from, to, &BlockedIndex::new(blocked))
}

/// Number of direction changes along a path — the second component
/// of the router's cost, re-derived so a test can pin it.
fn turn_count(path: &[(i32, i32, i32)]) -> usize {
    path.windows(3)
        .filter(|w| {
            let d0 = (w[1].0 - w[0].0, w[1].2 - w[0].2);
            let d1 = (w[2].0 - w[1].0, w[2].2 - w[1].2);
            d0 != d1
        })
        .count()
}

#[test]
fn route_path_unobstructed_is_shortest() {
    // With nothing in the way the route must not detour: the cell
    // count is exactly the Manhattan distance plus the start cell.
    let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    let (from, to) = ((0, 0, 0), (3, 0, 2));
    let path = route(from, to, &blocked).expect("open plane routes");
    assert_route_shape(&path, from, to, &blocked);
    assert_eq!(path.len(), manhattan(from, to) + 1);
}

#[test]
fn route_path_detours_around_a_wall() {
    // A solid wall of blocked cells across the straight line forces
    // the route around one end. Wall at x=2, z∈[-2, 2]; endpoints on
    // either side at z=0. Shortest detour: up/down to z=±3 and back
    // → 4 + manhattan extra steps. z=±3 lies *outside* the raw bbox
    // of blocked ∪ endpoints (z∈[-2, 2]) — the detour is only
    // reachable through the one-cell inflation margin, so this test
    // also pins the +1 inflation directly (an off-by-one there
    // leaves the router with no way around and fails the expect).
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    for z in -2..=2 {
        blocked.insert((2, 0, z));
    }
    let (from, to) = ((0, 0, 0), (4, 0, 0));
    let path = route(from, to, &blocked).expect("detour exists");
    assert_route_shape(&path, from, to, &blocked);
    // Manhattan is 4; rounding the wall costs 3 extra cells each way
    // (to z=3 or z=-3 and back) → 4 + 6 steps, 11 cells.
    assert_eq!(path.len(), manhattan(from, to) + 6 + 1);
    assert!(
        path.iter().any(|c| c.2.abs() == 3),
        "the only shortest detours run through the inflated margin row, got {path:?}",
    );
}

#[test]
fn route_path_prefers_fewest_turns_among_shortest_routes() {
    // Every shortest detour around the wall is 11 cells, but they
    // differ in turn count: a staircase zigzag has up to 8 turns,
    // the U along the margin row has 2. The cost's second component
    // must pick 2 — dropping `turns` from the cost (len-only
    // Dijkstra) would let heap timing pick a zigzag and the laid
    // gravel would look hand-broken.
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    for z in -2..=2 {
        blocked.insert((2, 0, z));
    }
    let path = route((0, 0, 0), (4, 0, 0), &blocked).expect("detour exists");
    assert_eq!(
        turn_count(&path),
        2,
        "shortest fewest-turns detour is a single U, got {path:?}",
    );
}

#[test]
fn route_path_breaks_symmetric_ties_toward_positive_x() {
    // A wall across the z axis leaves two mirror-image shortest
    // detours: around the east end (+x) or the west end (-x), equal
    // in both length and turns. The fixed STEP_DIRS order expands
    // `PosX` first, so the east side must win — this is the same
    // tie-break that routes village.crn's home1↔home3 walkway
    // around home1's east face, pinned here in isolation so a
    // STEP_DIRS reorder fails a unit test and not just the village
    // integration pins.
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    for x in -2..=2 {
        blocked.insert((x, 0, 3));
    }
    let (from, to) = ((0, 0, 0), (0, 0, 6));
    let path = route(from, to, &blocked).expect("detour exists");
    assert_route_shape(&path, from, to, &blocked);
    assert!(
        path.contains(&(3, 0, 3)),
        "the +x-first expansion order must round the east end of the wall, got {path:?}",
    );
}

#[test]
fn route_path_is_deterministic() {
    // Two runs over the same input must produce the identical cell
    // sequence — the lockfile pins walkway origin/dims, so a
    // hash-order-dependent tie-break would break reproducible builds.
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    for z in -2..=2 {
        blocked.insert((2, 0, z));
    }
    let a = route((0, 0, 0), (4, 0, 0), &blocked).expect("routes");
    let b = route((0, 0, 0), (4, 0, 0), &blocked).expect("routes");
    assert_eq!(a, b);
}

#[test]
fn route_path_is_independent_of_blocked_insertion_order() {
    // The determinism contract must not lean on hash-map iteration
    // order: two sets with the same cells but different insertion
    // orders (and therefore different `RandomState` seeds and
    // bucket layouts) must route identically. This is the guard
    // that keeps a future refactor from sneaking an iteration-order
    // dependency into the search.
    let cells: Vec<(i32, i32, i32)> = (-2..=2).map(|z| (2, 0, z)).collect();
    let forward: HashSet<(i32, i32, i32)> = cells.iter().copied().collect();
    let reverse: HashSet<(i32, i32, i32)> = cells.iter().rev().copied().collect();
    let a = route((0, 0, 0), (4, 0, 0), &forward).expect("routes");
    let b = route((0, 0, 0), (4, 0, 0), &reverse).expect("routes");
    assert_eq!(a, b);
}

#[test]
fn route_path_reports_which_endpoint_is_buried() {
    // A port buried under another placement's floor cannot anchor
    // a route; the error must say which side so the caller's
    // W_WALKWAY_BLOCKED note points at the right port.
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    blocked.insert((0, 0, 0));
    blocked.insert((9, 0, 9));
    assert_eq!(
        route((0, 0, 0), (5, 0, 5), &blocked),
        Err(RoutePathError::EndpointBlocked {
            from_blocked: true,
            to_blocked: false,
        }),
    );
    assert_eq!(
        route((5, 0, 5), (9, 0, 9), &blocked),
        Err(RoutePathError::EndpointBlocked {
            from_blocked: false,
            to_blocked: true,
        }),
    );
    assert_eq!(
        route((0, 0, 0), (9, 0, 9), &blocked),
        Err(RoutePathError::EndpointBlocked {
            from_blocked: true,
            to_blocked: true,
        }),
    );
}

#[test]
fn route_path_reports_enclosed_target_as_unreachable() {
    // A full ring of blocked cells around `to` leaves no route at
    // all — the search must terminate with `TargetUnreachable`
    // rather than spin.
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    for d in -1..=1 {
        blocked.insert((5 + d, 0, 4));
        blocked.insert((5 + d, 0, 6));
        blocked.insert((4, 0, 5 + d));
        blocked.insert((6, 0, 5 + d));
    }
    assert_eq!(
        route((0, 0, 0), (5, 0, 5), &blocked),
        Err(RoutePathError::TargetUnreachable),
    );
}

#[test]
fn route_path_same_endpoints_yields_single_cell() {
    // Kept graceful rather than asserted away: `route_path` is a
    // public API, and a single-cell "route" is the honest answer
    // for coincident ports even though `lower_connects` never asks
    // (a collision-free 1-cell L never reaches the router).
    let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    assert_eq!(route((5, 0, 5), (5, 0, 5), &blocked), Ok(vec![(5, 0, 5)]));
}

#[test]
fn route_path_ignores_blocked_cells_on_other_y_planes() {
    // `blocked` is a world-space 3D set; cells at a different Y must
    // neither obstruct the route nor inflate the search bounds.
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    for z in -2..=2 {
        blocked.insert((2, 7, z));
    }
    let (from, to) = ((0, 0, 0), (4, 0, 0));
    let path = route(from, to, &blocked).expect("open at y=0");
    assert_eq!(path.len(), manhattan(from, to) + 1);
}

#[test]
fn route_path_mixed_y_planes_only_walk_plane_obstructs() {
    // Obstacles on the walk plane and a *longer* copy of the same
    // wall on another plane, in one set: the route must detour
    // around the y=0 wall exactly as if the y=7 cells were absent.
    // A regression that inverts (or drops) the `y` filter would see
    // the taller y=7 wall, block the z=±3 margin crossing, and
    // return a longer path.
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    for z in -2..=2 {
        blocked.insert((2, 0, z));
    }
    for z in -4..=4 {
        blocked.insert((2, 7, z));
    }
    let (from, to) = ((0, 0, 0), (4, 0, 0));
    let path = route(from, to, &blocked).expect("detour exists at y=0");
    assert_route_shape(&path, from, to, &blocked);
    assert_eq!(path.len(), manhattan(from, to) + 6 + 1);
}

#[test]
fn route_path_gives_up_past_the_area_cap() {
    // Endpoints so far apart that the bounding rectangle exceeds the
    // search cap must fail fast instead of allocating the world.
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    blocked.insert((1, 0, 0));
    assert!(matches!(
        route((0, 0, 0), (10_000_000, 0, 10_000_000), &blocked),
        Err(RoutePathError::AreaCapExceeded { cap, .. }) if cap == ROUTE_AREA_CAP,
    ));
}

#[test]
fn route_path_area_cap_boundary_is_exclusive() {
    // Pin the `>` in the cap check from both sides. The endpoints
    // are adjacent, so the allowed case resolves in a handful of
    // heap pops even though the rectangle is at the cap — the cap
    // bounds the worst case, not every search. With `from=(0,0,0)`,
    // `to=(1,0,0)` and one far blocked cell at `(a, 0, 1997)`, the
    // inflated rectangle spans `(a+3) × 2000`: `a=1997` lands
    // exactly on the 4-million cap (allowed), `a=1998` is one
    // column past it (refused with both numbers reported).
    let at_cap: HashSet<(i32, i32, i32)> = std::iter::once((1997, 0, 1997)).collect();
    let path = route((0, 0, 0), (1, 0, 0), &at_cap).expect("area == cap is allowed");
    assert_eq!(path, vec![(0, 0, 0), (1, 0, 0)]);

    let past_cap: HashSet<(i32, i32, i32)> = std::iter::once((1998, 0, 1997)).collect();
    assert_eq!(
        route((0, 0, 0), (1, 0, 0), &past_cap),
        Err(RoutePathError::AreaCapExceeded {
            area: 4_002_000,
            cap: ROUTE_AREA_CAP,
            rect: SearchRect {
                min_x: -1,
                max_x: 1999,
                min_z: -1,
                max_z: 1998,
            },
        }),
    );
}

#[test]
fn route_path_pins_the_exact_tie_broken_detour() {
    // The full cell sequence for the wall fixture, pinned once so
    // any change to the cost function, the STEP_DIRS order, or the
    // equal-cost keep-first rule shows up as a concrete
    // before/after diff instead of a distant integration failure.
    // (The bounding-box pins in `village_lower.rs` survive a
    // tie-break flip because both sides share a bbox; this pin does
    // not.)
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    for z in -2..=2 {
        blocked.insert((2, 0, z));
    }
    let path = route((0, 0, 0), (4, 0, 0), &blocked).expect("detour exists");
    assert_eq!(
        path,
        vec![
            (0, 0, 0),
            (0, 0, 1),
            (0, 0, 2),
            (0, 0, 3),
            (1, 0, 3),
            (2, 0, 3),
            (3, 0, 3),
            (4, 0, 3),
            (4, 0, 2),
            (4, 0, 1),
            (4, 0, 0),
        ],
    );
}

#[test]
fn route_path_detours_along_each_edge_of_the_coordinate_space() {
    // `search_rect` refuses a rectangle whose one-cell margin
    // leaves `i32`, so the margin itself may lie on `i32::MIN` or
    // `i32::MAX`, and the router expands the cells there. Each of
    // those cells has a neighbour past the edge, which the router
    // has to skip rather than compute.
    //
    // The fixture is laid out in local coordinates: `a` is the
    // distance from the edge and `b` runs along it. The endpoints
    // are at `a = 1`, and a wall at `b = 2` covers `a ∈ 1..=3`.
    // The only short way round is through `a = 0`, the edge itself
    // (7 cells); the far way round, through `a = 4`, takes 11.
    type Local = fn(i32, i32) -> (i32, i32, i32);
    let edges: [(&str, Local); 4] = [
        ("+x", |a, b| (i32::MAX - a, 0, b)),
        ("-x", |a, b| (i32::MIN + a, 0, b)),
        ("+z", |a, b| (b, 0, i32::MAX - a)),
        ("-z", |a, b| (b, 0, i32::MIN + a)),
    ];
    for (edge, at) in edges {
        let blocked: HashSet<(i32, i32, i32)> = (1..=3).map(|a| at(a, 2)).collect();
        let (from, to) = (at(1, 0), at(1, 4));
        let path = route(from, to, &blocked)
            .unwrap_or_else(|e| panic!("{edge}: a detour along the edge exists, got {e:?}"));
        assert_route_shape(&path, from, to, &blocked);
        assert_eq!(
            path,
            [(1, 0), (0, 0), (0, 1), (0, 2), (0, 3), (0, 4), (1, 4)]
                .map(|(a, b)| at(a, b))
                .to_vec(),
            "{edge}: the route must run along the edge of the coordinate space",
        );
    }
}

#[test]
fn l_path_x_then_z_dedupes_corner() {
    let path = l_path((0, 0, 0), (3, 0, 2));
    // Expected order: (0,0,0) (1,0,0) (2,0,0) (3,0,0) — x leg
    //                 (3,0,1) (3,0,2)                  — z leg
    assert_eq!(
        path,
        vec![
            (0, 0, 0),
            (1, 0, 0),
            (2, 0, 0),
            (3, 0, 0),
            (3, 0, 1),
            (3, 0, 2),
        ],
    );
}

#[test]
fn l_path_negative_axes_step_backwards() {
    let path = l_path((2, 0, 1), (0, 0, -2));
    assert_eq!(
        path,
        vec![
            (2, 0, 1),
            (1, 0, 1),
            (0, 0, 1),
            (0, 0, 0),
            (0, 0, -1),
            (0, 0, -2),
        ],
    );
}

#[test]
fn l_path_same_endpoints_yields_single_cell() {
    let path = l_path((5, 0, 5), (5, 0, 5));
    assert_eq!(path, vec![(5, 0, 5)]);
}

/// A path with no x leg is all z leg, and its first cell is still the
/// one the x leg pushed: the stepping order alone keeps it single.
#[test]
fn l_path_along_z_alone_lays_each_cell_once() {
    let path = l_path((4, 0, 0), (4, 0, 3));
    assert_eq!(path, vec![(4, 0, 0), (4, 0, 1), (4, 0, 2), (4, 0, 3)]);
}

/// A north–south strip must not cost time quadratic in its length. A
/// lookup into the laid cells on every z step would make it quadratic:
/// a million-cell strip would then take hours in a debug build, where
/// the walk without one takes milliseconds. The deadline sits orders of
/// magnitude from both, so a slow runner cannot trip it and the
/// quadratic walk cannot meet it; the walk runs on its own thread so a
/// regression fails here instead of hanging the suite.
#[test]
fn l_path_lays_a_long_z_strip_in_less_than_quadratic_time() {
    use std::sync::mpsc::RecvTimeoutError;
    const LEN: i32 = 1_000_000;
    let (tx, rx) = std::sync::mpsc::channel();
    let walk = std::thread::spawn(move || {
        let _ = tx.send(l_path((0, 0, 0), (0, 0, LEN - 1)));
    });
    let path = match rx.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(path) => path,
        Err(RecvTimeoutError::Timeout) => {
            panic!("a million-cell z strip was not laid within a minute")
        }
        Err(RecvTimeoutError::Disconnected) => match walk.join() {
            Err(payload) => std::panic::resume_unwind(payload),
            Ok(()) => panic!("the walk thread ended without sending a path"),
        },
    };
    assert_eq!(path.len(), usize::try_from(LEN).expect("fits"));
    assert_eq!(path.first(), Some(&(0, 0, 0)));
    assert_eq!(path.last(), Some(&(0, 0, LEN - 1)));
}

fn sample_key() -> WalkwayScopeKey {
    use crate::ids::{PlaceId, PortId, SiteName, WalkwayEndpoint};
    let site = SiteName::new("s").expect("site");
    let a = WalkwayEndpoint {
        place: PlaceId::new("a").expect("place"),
        port: PortId::new("entry").expect("port"),
    };
    let b = WalkwayEndpoint {
        place: PlaceId::new("b").expect("place"),
        port: PortId::new("entry").expect("port"),
    };
    WalkwayScopeKey::from_parts(&site, &a, &b).expect("from_parts")
}

#[test]
fn build_walkway_array_fills_unblocked_cells() {
    let path = vec![(0, 0, 0), (1, 0, 0), (1, 0, 1)];
    let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    let layout = build_walkway_array(
        &path,
        BlockState::bare("minecraft:gravel"),
        &blocked,
        &sample_key(),
    );
    assert_eq!(layout.blocked_count, 0);
    assert_eq!(layout.origin, (0, 0, 0));
    assert_eq!(layout.array.dims, Dims { x: 2, y: 1, z: 2 });
    // Three of the four cells should hold gravel; (0,0,1) was never
    // in the path, so it stays air.
    let palette_id_at = |x: u32, z: u32| -> &str {
        let i = layout.array.dims.index(x, 0, z).expect("in-range");
        let pi = layout.array.voxels[i];
        layout.array.palette.entries[usize::from(pi.0)].id.as_str()
    };
    assert_eq!(palette_id_at(0, 0), "minecraft:gravel");
    assert_eq!(palette_id_at(1, 0), "minecraft:gravel");
    assert_eq!(palette_id_at(1, 1), "minecraft:gravel");
    assert_eq!(palette_id_at(0, 1), "minecraft:air");
}

#[test]
fn build_walkway_array_skips_blocked_cells() {
    let path = vec![(0, 0, 0), (1, 0, 0), (2, 0, 0)];
    let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
    blocked.insert((1, 0, 0));
    let layout = build_walkway_array(
        &path,
        BlockState::bare("minecraft:gravel"),
        &blocked,
        &sample_key(),
    );
    assert_eq!(layout.blocked_count, 1);
    // Middle cell stays air despite being on the path.
    let mid = layout.array.dims.index(1, 0, 0).unwrap();
    assert_eq!(layout.array.voxels[mid], PaletteIndex::AIR);
}
