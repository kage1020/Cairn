use super::*;

fn dims_for(interior_w: u32, interior_h: u32, overhang: u32, y: u32) -> Dims {
    Dims {
        x: interior_w + 2 * overhang,
        y,
        z: interior_h + 2 * overhang,
    }
}

#[test]
fn from_ident_parses_four_cardinals() {
    assert_eq!(WallSide::from_ident("front"), Some(WallSide::Front));
    assert_eq!(WallSide::from_ident("back"), Some(WallSide::Back));
    assert_eq!(WallSide::from_ident("left"), Some(WallSide::Left));
    assert_eq!(WallSide::from_ident("right"), Some(WallSide::Right));
    assert_eq!(WallSide::from_ident("up"), None);
}

#[test]
fn front_wall_maps_u_to_x_and_z_to_far_edge() {
    // size=9x7, overhang=1: the front wall sits at z = 1 + 7 - 1 = 7.
    let dims = dims_for(9, 7, 1, 9);
    assert_eq!(
        wall_local_to_grid(WallSide::Front, 0, 1, 1, 9, 7, dims),
        Some((1, 1, 7)),
    );
    assert_eq!(
        wall_local_to_grid(WallSide::Front, 8, 4, 1, 9, 7, dims),
        Some((9, 4, 7)),
    );
}

#[test]
fn back_wall_mirrors_u_along_x() {
    let dims = dims_for(9, 7, 1, 9);
    assert_eq!(
        wall_local_to_grid(WallSide::Back, 0, 1, 1, 9, 7, dims),
        Some((9, 1, 1)),
    );
    assert_eq!(
        wall_local_to_grid(WallSide::Back, 8, 1, 1, 9, 7, dims),
        Some((1, 1, 1)),
    );
}

#[test]
fn left_and_right_walls_run_along_z() {
    let dims = dims_for(9, 7, 1, 9);
    assert_eq!(
        wall_local_to_grid(WallSide::Left, 0, 2, 1, 9, 7, dims),
        Some((1, 2, 1)),
    );
    assert_eq!(
        wall_local_to_grid(WallSide::Left, 6, 2, 1, 9, 7, dims),
        Some((1, 2, 7)),
    );
    assert_eq!(
        wall_local_to_grid(WallSide::Right, 0, 2, 1, 9, 7, dims),
        Some((9, 2, 7)),
    );
    assert_eq!(
        wall_local_to_grid(WallSide::Right, 6, 2, 1, 9, 7, dims),
        Some((9, 2, 1)),
    );
}

#[test]
fn out_of_range_returns_none() {
    let dims = dims_for(9, 7, 1, 9);
    assert_eq!(
        wall_local_to_grid(WallSide::Front, 9, 1, 1, 9, 7, dims),
        None,
    );
    assert_eq!(
        wall_local_to_grid(WallSide::Front, 0, 9, 1, 9, 7, dims),
        None,
    );
}

#[test]
fn the_gate_is_exactly_the_wall_on_every_side() {
    let dims = dims_for(9, 7, 1, 9);
    for (side, len, last_cell) in [
        (WallSide::Front, 9, (9, 8, 7)),
        (WallSide::Back, 9, (1, 8, 1)),
        (WallSide::Left, 7, (1, 8, 7)),
        (WallSide::Right, 7, (9, 8, 1)),
    ] {
        assert_eq!(wall_length(side, 9, 7), len, "{side:?}");
        assert_eq!(
            wall_local_to_grid(side, len - 1, dims.y - 1, 1, 9, 7, dims),
            Some(last_cell),
            "{side:?}: the last cell of the wall maps",
        );
        assert_eq!(
            wall_local_to_grid(side, len, 0, 1, 9, 7, dims),
            None,
            "{side:?}: `u = len` is past the wall",
        );
        assert_eq!(
            wall_local_to_grid(side, 0, dims.y, 1, 9, 7, dims),
            None,
            "{side:?}: `v = dims.y` is past the wall",
        );
    }
}

#[test]
fn wall_length_picks_axis_by_side() {
    assert_eq!(wall_length(WallSide::Front, 9, 7), 9);
    assert_eq!(wall_length(WallSide::Back, 9, 7), 9);
    assert_eq!(wall_length(WallSide::Left, 9, 7), 7);
    assert_eq!(wall_length(WallSide::Right, 9, 7), 7);
}
