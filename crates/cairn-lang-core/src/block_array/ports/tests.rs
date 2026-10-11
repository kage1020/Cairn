use super::*;
use crate::block_array::openings::door_at_deferral;

fn pid(name: &str) -> PortId {
    PortId::new(name).expect("valid port id")
}

/// [`port_world_position`] with every door and window taken as cut, for
/// the cases about the port's own rules.
fn port_at(
    origin: (i32, i32, i32),
    dims: Dims,
    def: &DefIr,
    id: &PortId,
    walls: &WallColumn,
) -> Result<(i32, i32, i32), PortRejection> {
    let cut: HashSet<Span> = def
        .members
        .iter()
        .filter(|m| matches!(m.role, MemberRole::Door | MemberRole::Window))
        .map(|m| m.span.clone())
        .collect();
    port_world_position(origin, dims, def, id, walls, &cut)
}

/// The column the fixture's top-level `walls` declare, whether or not
/// their materials resolve. The port against what a body actually
/// paints is pinned in `tests/lower/port_reads_the_wall_that_paints.rs`.
fn declared_column(def: &DefIr) -> WallColumn {
    WallColumn::from_walls(
        def.members
            .iter()
            .filter(|m| matches!(m.role, MemberRole::Walls))
            .filter_map(|m| m.nonneg_u32("height").map(|h| (0, h))),
    )
}

#[test]
fn port_world_position_offsets_one_block_outside_front_door() {
    // size=3x3 interior, no overhang inflation (place dims match
    // interior). center_u = wall_length / 2 = 3 / 2 = 1; door wall
    // world at (10 + 1, 0, 20 + 3 - 1) = (11, 0, 22); +1 in +z
    // direction → (11, 0, 23).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=front at=center\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (11, 0, 23));
}

#[test]
fn port_world_position_shifts_outward_past_roof_overhang() {
    // size=3x3 with a `+1` overhang on every horizontal side → place
    // dims (5, _, 5). Front wall world: (origin.x + overhang + u,
    // origin.z + overhang + interior_h - 1) = (10 + 1 + 1,
    // 20 + 1 + 3 - 1) = (12, 23); +1 in the +z direction puts the
    // port one block beyond the eave → (12, 0, 24).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=front at=center\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 5, y: 1, z: 5 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (12, 0, 24));
}

#[test]
fn port_world_position_back_side_steps_into_negative_z() {
    // size=3x3, overhang=0, center u=1. Back wall world:
    // x = origin.x + (w-1-u) = 10 + (3-1-1) = 11, z = origin.z = 20.
    // Normal step is (0, -1) → port (11, 0, 19).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=back at=center\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (11, 0, 19));
}

#[test]
fn port_world_position_left_side_steps_into_negative_x() {
    // size=3x3, overhang=0, center u=1. Left wall world:
    // x = origin.x = 10, z = origin.z + u = 20 + 1 = 21.
    // Normal step is (-1, 0) → port (9, 0, 21).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=left at=center\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (9, 0, 21));
}

#[test]
fn port_world_position_right_side_steps_into_positive_x() {
    // size=3x3, overhang=0, center u=1. Right wall world:
    // x = origin.x + (w-1) = 12, z = origin.z + (h-1-u) = 20 + 1 = 21.
    // Normal step is (+1, 0) → port (13, 0, 21).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=right at=center\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (13, 0, 21));
}

#[test]
fn port_world_position_window_front_resolves_to_offset_center() {
    // size=3x3, no overhang, window offset=0 size=1x1 on front wall.
    // wall_length(Front, 3, 3) = 3; u = 0 + 1/2 = 0. Wall world via
    // wall_local_to_grid: (origin.x + 0, origin.z + 3 - 1) = (10, 22);
    // +1 in the +z normal step → port (10, 0, 23).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  window id=light side=front y=1 offset=0 size=1x1 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (10, 0, 23));
}

#[test]
fn port_world_position_window_back_resolves_to_mirrored_center() {
    // size=3x3, window offset=1 size=1x1 on back wall. wall_length = 3;
    // u = 1 + 0 = 1. Back wall world: mirrored = 3 - 1 - 1 = 1,
    // (origin.x + 1, origin.z + 0) = (11, 20); -1 in the -z normal
    // step → port (11, 0, 19).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  window id=light side=back y=1 offset=1 size=1x1 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (11, 0, 19));
}

#[test]
fn port_world_position_window_left_resolves_to_offset_center() {
    // size=3x3, window offset=1 size=1x1 on left wall. wall_length
    // (Left, 3, 3) = 3; u = 1. Left wall world: (origin.x + 0,
    // origin.z + 1) = (10, 21); -1 in the -x normal step → port
    // (9, 0, 21).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  window id=light side=left y=1 offset=1 size=1x1 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (9, 0, 21));
}

#[test]
fn port_world_position_window_right_resolves_to_mirrored_center() {
    // size=3x3, window offset=1 size=1x1 on right wall. wall_length
    // (Right, 3, 3) = 3; u = 1. Right wall world: mirrored = 1,
    // x = origin.x + 3 - 1 = 12, z = origin.z + 1 = 21; +1 in the +x
    // normal step → port (13, 0, 21).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  window id=light side=right y=1 offset=1 size=1x1 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (13, 0, 21));
}

#[test]
fn port_world_position_window_shifts_outward_past_roof_overhang() {
    // size=3x3 interior, place_dims=(5,_,5) for overhang=1. Window
    // offset=0 size=1x1 on front. u = 0. Wall world via
    // wall_local_to_grid with overhang=1: (origin.x + 1, origin.z +
    // 1 + 3 - 1) = (11, 23); +1 in the +z normal step → port
    // (11, 0, 24).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  window id=light side=front y=1 offset=0 size=1x1 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 5, y: 1, z: 5 };
    let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (11, 0, 24));
}

#[test]
fn port_world_position_window_centres_on_2x2_offset() {
    // village.crn shape: size=9x7, place_dims (11,_,9) for
    // overhang=1, window id=front side=front y=2 offset=2 size=2x2.
    // wall_length(Front, 9, 7) = 9; u = 2 + 2/2 = 3. Wall world:
    // (origin.x + 1 + 3, origin.z + 1 + 7 - 1) = (origin.x + 4,
    // origin.z + 7); +1 in the +z normal step → port shifts to z+8.
    // With origin (0,0,0): port (4, 0, 8).
    let src = concat!(
        "def cottage size=9x7:\n",
        "  walls mat_slot=w height=4\n",
        "  window id=front side=front y=2 offset=2 size=2x2 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 11, y: 1, z: 9 };
    let pos =
        port_at((0, 0, 0), dims, def, &pid("front"), &declared_column(def)).expect("port resolves");
    assert_eq!(pos, (4, 0, 8));
}

#[test]
fn port_world_position_window_refuses_when_offset_size_overflows_wall() {
    // size=3x3 → wall_length(Front) = 3. offset=2 + size.w=2 = 4 > 3.
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=5\n",
        "  window id=light side=front y=1 offset=2 size=2x2 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    assert!(matches!(
        port_at((0, 0, 0), dims, def, &pid("light"), &declared_column(def)),
        Err(PortRejection::WindowPastWall {
            offset: 2,
            width: 2,
            wall_length: 3,
            ..
        }),
    ));
}

#[test]
fn port_world_position_window_sym_true_uses_primary_offset() {
    // `sym=true` mirrors the cut, but a port takes the primary `offset`
    // side so one `id=` maps to one coordinate. Same geometry as the
    // front-resolves test.
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  window id=light side=front y=1 offset=0 size=1x1 sym=true mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (10, 0, 23));
}

#[test]
fn port_world_position_window_pins_y_to_ground_row_regardless_of_authored_y() {
    // Walls `height=10` so the window fits vertically and only the pin
    // to `place_origin.1` is under test.
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=10\n",
        "  window id=light side=front y=4 offset=0 size=1x1 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    // Honouring the window's `y=` would give `(10, 11, 23)`.
    let pos = port_at((10, 7, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (10, 7, 23));
}

#[test]
fn port_world_position_refuses_for_roof_role() {
    let src = concat!(
        "def cottage size=3x3:\n",
        "  roof id=top kind=gable mat_slot=r\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    assert!(matches!(
        port_at((0, 0, 0), dims, def, &pid("top"), &declared_column(def)),
        Err(PortRejection::ReservedRole { ref role, .. }) if role == "roof",
    ));
}

#[test]
fn port_world_position_refuses_for_stair_role() {
    let src = concat!("def cottage size=3x3:\n", "  stair id=up at=corner\n");
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    assert!(matches!(
        port_at((0, 0, 0), dims, def, &pid("up"), &declared_column(def)),
        Err(PortRejection::ReservedRole { ref role, .. }) if role == "stair",
    ));
}

#[test]
fn port_world_position_window_accepts_boundary_offset_plus_size_equal_wall_length() {
    // Only this test catches the bound tightened from `>` to `>=`.
    // size=3x3 → wall_length(Front) = 3 = offset 1 + size.w 2.
    // u = 1 + 2/2 = 2. Wall world: (origin.x + 2, origin.z + 3 - 1) =
    // (12, 22); +1 +z → port (12, 0, 23).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=5\n",
        "  window id=light side=front y=1 offset=1 size=2x2 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (12, 0, 23));
}

#[test]
fn port_world_position_window_accepts_a_rectangle_inside_the_wall() {
    // Rows 2..=2 of 1..=3, strictly inside the wall.
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  window id=light side=front y=2 offset=0 size=1x1 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (10, 0, 23));
}

#[test]
fn port_world_position_window_refuses_when_the_top_edge_pierces_the_wall() {
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=2\n",
        "  window id=light side=front y=2 offset=0 size=1x2 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    // `walls height=2` paints rows 1..=2; the rectangle wants 2..=3.
    assert!(matches!(
        port_at((0, 0, 0), dims, def, &pid("light"), &declared_column(def)),
        Err(PortRejection::WindowOutsideCourse { y: 2, height: 2, ref walls, .. }) if !walls.is_empty(),
    ));
}

#[test]
fn port_world_position_window_refuses_when_def_has_no_walls() {
    let src = concat!(
        "def cottage size=3x3:\n",
        "  window id=light side=front y=0 offset=0 size=1x1 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    assert!(matches!(
        port_at((0, 0, 0), dims, def, &pid("light"), &declared_column(def)),
        Err(PortRejection::WindowNoWalls { .. }),
    ));
}

#[test]
fn port_world_position_window_back_shifts_outward_past_roof_overhang() {
    // The back wall maps differently from the front (mirrored `u`,
    // `z = overhang`), so the front's overhang test does not cover
    // it. size=3x3 interior, place_dims=(5,_,5) for
    // overhang=1, window offset=0 size=1x1 → u = 0; mirrored = 3 -
    // 1 - 0 = 2; wall world (origin.x + 1 + 2, origin.z + 1) =
    // (13, 21); -1 -z normal → port (13, 0, 20).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  window id=light side=back y=1 offset=0 size=1x1 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 5, y: 1, z: 5 };
    let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (13, 0, 20));
}

#[test]
fn port_world_position_window_right_shifts_outward_past_roof_overhang() {
    // Right wall mirrors `u` along `z` and pins `x = overhang +
    // interior_w - 1`. size=3x3 interior, place_dims=(5,_,5) for
    // overhang=1, window offset=0 size=1x1 → u = 0; mirrored = 3 -
    // 1 - 0 = 2; wall world (origin.x + 1 + 3 - 1, origin.z + 1 +
    // 2) = (13, 23); +1 +x normal → port (14, 0, 23).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  window id=light side=right y=1 offset=0 size=1x1 mat_slot=g\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 5, y: 1, z: 5 };
    let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (14, 0, 23));
}

#[test]
fn port_world_position_refuses_for_unknown_port_id() {
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=front at=center\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    assert!(matches!(
        port_at((0, 0, 0), dims, def, &pid("nope"), &declared_column(def)),
        Err(PortRejection::UnknownMember),
    ));
}

#[test]
fn port_world_position_door_at_left_resolves_to_origin_corner_on_front() {
    // size=3x3, no overhang. `at=left` pins u = 0 (the wall-local axis
    // origin). Front wall world: (origin.x + 0, _, origin.z + 3 - 1)
    // = (10, _, 22); +1 in the +z normal step → port (10, 0, 23).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=front at=left\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (10, 0, 23));
}

#[test]
fn port_world_position_door_at_right_resolves_to_far_corner_on_front() {
    // size=3x3, no overhang. `at=right` pins u = wall_length - 1 = 2.
    // Front wall world: (origin.x + 2, _, origin.z + 3 - 1) = (12, _,
    // 22); +1 in the +z normal step → port (12, 0, 23).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=front at=right\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (12, 0, 23));
}

#[test]
fn port_world_position_door_back_at_left_uses_mirrored_axis() {
    // Back wall mirrors u along x (`x = w - 1 - u`), so `at=left`
    // (u = 0) lands at the far x corner: (origin.x + (3 - 1 - 0),
    // origin.z + 0) = (12, _, 20); -1 in the -z normal step → port
    // (12, 0, 19). Without the mirror it would land at (10, 0, 19).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=back at=left\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (12, 0, 19));
}

#[test]
fn port_world_position_door_left_at_right_uses_far_z_corner() {
    // Left wall maps u to z without mirroring, so `at=right`
    // (u = interior_h - 1 = 2) lands at z = origin.z + 2 = 22.
    // x = origin.x; -1 in the -x normal step → port (9, 0, 22).
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=left at=right\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (9, 0, 22));
}

#[test]
fn port_world_position_door_right_at_left_mirrors_along_z() {
    // Right wall mirrors u along z (`z = h - 1 - u`), so `at=left`
    // (u = 0) lands at the far z corner: z = origin.z + 2 = 22.
    // x = origin.x + interior_w - 1 = 12; +1 in the +x normal step →
    // port (13, 0, 22). Without the mirror it would land at z = 20.
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=right at=left\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (13, 0, 22));
}

#[test]
fn port_world_position_door_at_left_shifts_outward_past_roof_overhang() {
    // size=3x3 interior, place_dims=(5,_,5) for overhang=1.
    // `at=left` pins u = 0. Front wall world: (origin.x + overhang +
    // 0, _, origin.z + overhang + 3 - 1) = (11, _, 23); +1 in the +z
    // normal step → port (11, 0, 24). Without the overhang shift it
    // would land inside the eave at z = 23.
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=front at=left\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 5, y: 1, z: 5 };
    let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
        .expect("port resolves");
    assert_eq!(pos, (11, 0, 24));
}

#[test]
fn port_world_position_door_refuses_when_the_body_paints_no_wall() {
    let src = concat!(
        "def cottage size=3x3:\n",
        "  door id=entry side=front at=center\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    assert!(matches!(
        port_at((0, 0, 0), dims, def, &pid("entry"), &WallColumn::default()),
        Err(PortRejection::DoorNoWalls { .. }),
    ));
}

#[test]
fn port_world_position_door_refuses_when_the_walls_start_above_the_doorway() {
    // The column holds rows 7..=10 (`level y=6 walls height=4`), and
    // the doorway opens at row 1, in open air.
    let src = concat!(
        "def cottage size=3x3:\n",
        "  door id=entry side=front at=center\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    let upper_storey = WallColumn::from_walls([(6, 4)]);
    assert!(matches!(
        port_at((0, 0, 0), dims, def, &pid("entry"), &upper_storey),
        Err(PortRejection::DoorOutsideCourse { ref walls, .. }) if !walls.is_empty(),
    ));
    // With a ground course under it the port anchors, so the refusal
    // is about the row and not the level.
    let both_storeys = WallColumn::from_walls([(0, 3), (6, 4)]);
    assert!(port_at((0, 0, 0), dims, def, &pid("entry"), &both_storeys).is_ok());
    // A one-row course `1..=1` is enough: the port asks where the
    // doorway opens, not where it ends. A row constant off by one
    // would refuse it.
    let one_row = WallColumn::from_walls([(0, 1)]);
    assert!(port_at((0, 0, 0), dims, def, &pid("entry"), &one_row).is_ok());
}

#[test]
fn port_world_position_door_refuses_for_unknown_at_value() {
    let src = concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=entry side=front at=middle\n",
    );
    let module = crate::parse(src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 1, z: 3 };
    assert!(matches!(
        port_at((0, 0, 0), dims, def, &pid("entry"), &declared_column(def)),
        Err(PortRejection::DoorAt { written: Written::Ident(ref s), .. }) if s == "middle",
    ));
}

/// Lower `body` as the members of `def cottage size=3x3` and ask for
/// the port `id` against the column its own `walls` declare.
fn port_in(body: &str, id: &str) -> Result<(i32, i32, i32), PortRejection> {
    let src = format!("def cottage size=3x3:\n{body}");
    let module = crate::parse(&src).expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 5, z: 3 };
    port_at((0, 0, 0), dims, def, &pid(id), &declared_column(def))
}

#[test]
fn port_world_position_asks_the_role_before_the_side() {
    assert!(matches!(
        port_in("  stair id=up kind=stairs mat_slot=s\n", "up"),
        Err(PortRejection::ReservedRole { ref role, .. }) if role == "stair",
    ));
}

#[test]
fn port_world_position_refuses_a_missing_side_as_absent() {
    assert!(matches!(
        port_in("  walls mat_slot=w height=3\n  door id=e at=center\n", "e"),
        Err(PortRejection::Side {
            written: Written::Absent,
            ..
        }),
    ));
}

#[test]
fn port_world_position_refuses_a_non_cardinal_side_with_what_was_written() {
    assert!(matches!(
        port_in("  walls mat_slot=w height=3\n  door id=e side=frnt at=center\n", "e"),
        Err(PortRejection::Side { written: Written::Ident(ref s), .. }) if s == "frnt",
    ));
    assert!(matches!(
        port_in(
            "  walls mat_slot=w height=3\n  door id=e side=3 at=center\n",
            "e"
        ),
        Err(PortRejection::Side {
            written: Written::OtherShape,
            ..
        }),
    ));
}

#[test]
fn port_world_position_refuses_a_missing_door_at_as_absent() {
    assert!(matches!(
        port_in("  walls mat_slot=w height=3\n  door id=e side=front\n", "e"),
        Err(PortRejection::DoorAt {
            written: Written::Absent,
            ..
        }),
    ));
}

#[test]
fn port_world_position_asks_the_door_at_before_the_masonry() {
    assert!(matches!(
        port_in("  door id=e side=front at=middle\n", "e"),
        Err(PortRejection::DoorAt { .. }),
    ));
}

#[test]
fn port_world_position_refuses_a_window_argument_by_name() {
    for (args, fault) in [
        ("side=front offset=0 size=1x1", WindowArgFault::YAbsent),
        (
            "side=front offset=0 y=abc size=1x1",
            WindowArgFault::YMalformed,
        ),
        ("side=front offset=0 y=1", WindowArgFault::SizeAbsent),
        (
            "side=front offset=0 y=1 size=3",
            WindowArgFault::SizeMalformed,
        ),
        (
            "side=front offset=x y=1 size=1x1",
            WindowArgFault::OffsetMalformed,
        ),
    ] {
        let body = format!("  walls mat_slot=w height=3\n  window id=l {args} mat_slot=g\n");
        assert!(
            matches!(
                port_in(&body, "l"),
                Err(PortRejection::WindowArgument { fault: f, .. }) if f == fault,
            ),
            "`{args}` should be refused as {fault:?}",
        );
    }
}

#[test]
fn port_world_position_window_without_offset_anchors_at_the_origin_like_the_cut() {
    // `cut_window` reads an absent `offset=` as `0`.
    assert_eq!(
        port_in(
            "  walls mat_slot=w height=3\n  window id=l side=front y=1 size=1x1 mat_slot=g\n",
            "l"
        ),
        port_in(
            "  walls mat_slot=w height=3\n  window id=l side=front offset=0 y=1 size=1x1 mat_slot=g\n",
            "l"
        ),
    );
    assert!(
        port_in(
            "  walls mat_slot=w height=3\n  window id=l side=front y=1 size=1x1 mat_slot=g\n",
            "l"
        )
        .is_ok()
    );
}

#[test]
fn port_world_position_refuses_a_sizeless_def() {
    let module = crate::parse("def cottage:\n  door id=e side=front at=center\n").expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 5, z: 3 };
    assert_eq!(
        port_at(
            (0, 0, 0),
            dims,
            def,
            &pid("e"),
            &WallColumn::from_walls([(0, 3)])
        ),
        Err(PortRejection::Sizeless),
    );
}

#[test]
fn port_world_position_refuses_a_coordinate_past_i32_as_out_of_range() {
    let module = crate::parse(
        "def cottage size=3x3:\n  walls mat_slot=w height=3\n  door id=e side=right at=center\n",
    )
    .expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 5, z: 3 };
    // The right wall's `origin.x + 2` overflows inside `wall_world_xz`,
    // before the step out of the wall, which the next test reaches.
    assert_eq!(
        port_at(
            (i32::MAX - 1, 0, 0),
            dims,
            def,
            &pid("e"),
            &declared_column(def)
        ),
        Err(PortRejection::OutOfRange),
    );
}

#[test]
fn port_world_position_refuses_a_coordinate_past_i32_at_the_last_step_out_of_the_wall() {
    // A back door at `z = i32::MIN`: the wall is addressable and only
    // the step outward (`-z`) overflows, the one case in this file that
    // reaches the final `checked_add`.
    let module = crate::parse(
        "def cottage size=3x3:\n  walls mat_slot=w height=3\n  door id=e side=back at=center\n",
    )
    .expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 5, z: 3 };
    assert_eq!(
        port_at(
            (0, 0, i32::MIN),
            dims,
            def,
            &pid("e"),
            &declared_column(def)
        ),
        Err(PortRejection::OutOfRange),
    );
    // One row further in, the door is placed.
    assert_eq!(
        port_at(
            (0, 0, i32::MIN + 1),
            dims,
            def,
            &pid("e"),
            &declared_column(def)
        ),
        Ok((1, 0, i32::MIN)),
    );
}

#[test]
fn port_world_position_asks_the_window_horizontal_fit_before_the_masonry() {
    // `offset=4 size=3x2` runs past the 3-long wall and `y=9` is above
    // the 3-high course.
    assert!(matches!(
        port_in(
            "  walls mat_slot=w height=3\n  window id=l side=front offset=4 y=9 size=3x2 mat_slot=g\n",
            "l"
        ),
        Err(PortRejection::WindowPastWall { .. }),
    ));
}

#[test]
fn port_world_position_refuses_an_opening_the_openings_pass_did_not_cut() {
    // Every rule the port reads holds; only the empty `cut` set
    // refuses it.
    let module = crate::parse(concat!(
        "def cottage size=3x3:\n",
        "  walls mat_slot=w height=3\n",
        "  door id=e side=front at=center\n",
        "  window id=l side=back offset=0 y=1 size=1x1 mat_slot=g\n",
    ))
    .expect("parse");
    let ir = crate::lower(&module);
    let def = ir.defs.first().expect("def lowered");
    let dims = Dims { x: 3, y: 5, z: 3 };
    let walls = declared_column(def);
    let none: HashSet<Span> = HashSet::new();
    for (id, role) in [("e", "door"), ("l", "window")] {
        assert!(
            matches!(
                port_world_position((0, 0, 0), dims, def, &pid(id), &walls, &none),
                Err(PortRejection::NotCut { role: r, .. }) if r == role,
            ),
            "`{id}` was not cut",
        );
        assert!(port_at((0, 0, 0), dims, def, &pid(id), &walls).is_ok());
    }
}

/// One rejection per variant and, where a variant carries how an
/// argument was [`Written`], per way of writing it.
fn every_rejection() -> Vec<PortRejection> {
    // The seeds' fields are placeholders: `samples` reads only the
    // variant.
    let seeds = [
        PortRejection::UnknownMember,
        PortRejection::ReservedRole {
            role: String::new(),
            member: 0..0,
        },
        PortRejection::Side {
            written: Written::Absent,
            member: 0..0,
        },
        PortRejection::Sizeless,
        PortRejection::DoorAt {
            written: Written::Absent,
            member: 0..0,
        },
        PortRejection::DoorNoWalls { member: 0..0 },
        PortRejection::DoorOutsideCourse {
            walls: WallColumn::default(),
            member: 0..0,
        },
        PortRejection::WindowArgument {
            fault: WindowArgFault::YAbsent,
            member: 0..0,
        },
        PortRejection::WindowPastWall {
            offset: 0,
            width: 0,
            wall_length: 0,
            member: 0..0,
        },
        PortRejection::WindowNoWalls { member: 0..0 },
        PortRejection::WindowOutsideCourse {
            y: 0,
            height: 0,
            walls: WallColumn::default(),
            member: 0..0,
        },
        PortRejection::NotCut {
            role: "door",
            member: 0..0,
        },
        PortRejection::OutOfRange,
    ];
    seeds.iter().flat_map(samples).collect()
}

/// Every rendering [`every_rejection`] checks for the variant of `r`.
/// An exhaustive `match`, so a new variant is a compile error here
/// until it has a row.
fn samples(r: &PortRejection) -> Vec<PortRejection> {
    let span: Span = 3..9;
    match r {
        PortRejection::UnknownMember => vec![PortRejection::UnknownMember],
        PortRejection::ReservedRole { .. } => ["stair", "roof", "floor"]
            .into_iter()
            .map(|role| PortRejection::ReservedRole {
                role: role.to_owned(),
                member: span.clone(),
            })
            .collect(),
        PortRejection::Side { .. } => written_samples("frnt")
            .into_iter()
            .map(|written| PortRejection::Side {
                written,
                member: span.clone(),
            })
            .collect(),
        PortRejection::Sizeless => vec![PortRejection::Sizeless],
        PortRejection::DoorAt { .. } => written_samples("middle")
            .into_iter()
            .map(|written| PortRejection::DoorAt {
                written,
                member: span.clone(),
            })
            .collect(),
        PortRejection::DoorNoWalls { .. } => vec![PortRejection::DoorNoWalls {
            member: span.clone(),
        }],
        PortRejection::DoorOutsideCourse { .. } => {
            vec![PortRejection::DoorOutsideCourse {
                walls: WallColumn::from_walls([(6, 4)]),
                member: span.clone(),
            }]
        }
        PortRejection::WindowArgument { .. } => [
            WindowArgFault::OffsetMalformed,
            WindowArgFault::YAbsent,
            WindowArgFault::YMalformed,
            WindowArgFault::SizeAbsent,
            WindowArgFault::SizeMalformed,
        ]
        .into_iter()
        .map(|fault| PortRejection::WindowArgument {
            fault,
            member: span.clone(),
        })
        .collect(),
        PortRejection::WindowPastWall { .. } => vec![PortRejection::WindowPastWall {
            offset: 2,
            width: 2,
            wall_length: 3,
            member: span.clone(),
        }],
        PortRejection::WindowNoWalls { .. } => vec![PortRejection::WindowNoWalls {
            member: span.clone(),
        }],
        PortRejection::WindowOutsideCourse { .. } => [(0, 1), (u32::MAX, 2)]
            .into_iter()
            .map(|(y, height)| PortRejection::WindowOutsideCourse {
                y,
                height,
                walls: WallColumn::from_walls([(0, 3)]),
                member: span.clone(),
            })
            .collect(),
        PortRejection::NotCut { .. } => ["door", "window"]
            .into_iter()
            .map(|role| PortRejection::NotCut {
                role,
                member: span.clone(),
            })
            .collect(),
        PortRejection::OutOfRange => vec![PortRejection::OutOfRange],
    }
}

fn written_samples(ident: &str) -> [Written; 3] {
    [
        Written::Absent,
        Written::Ident(ident.to_owned()),
        Written::OtherShape,
    ]
}

#[test]
fn every_rejection_note_renders_in_full() {
    const OWN: &str = "; the member says so on its own line too";
    const NO_WALLS: &str = "its `def` has no `walls` that paints — a positive `height=` and a `mat_slot=` that resolves";
    let expected: Vec<(String, bool)> = vec![
        ("`a.p` names no member of its `def` body".to_owned(), false),
        ("`a.p` is a `stair`, and only a `door` or a `window` can anchor a port (`stair` ports are reserved) — declare the port on a door or window instead".to_owned(), true),
        ("`a.p` is a `roof`, and only a `door` or a `window` can anchor a port (`roof` ports are reserved) — declare the port on a door or window instead".to_owned(), true),
        ("`a.p` is a `floor`, and only a `door` or a `window` can anchor a port — declare the port on a door or window instead".to_owned(), true),
        (format!("`a.p` has no `side=` (expected one of front, back, left, right){OWN}"), true),
        (format!("`a.p` has `side=frnt`, which is not one of front, back, left, right{OWN}"), true),
        (format!("`a.p` has a `side=` that is not one of front, back, left, right{OWN}"), true),
        ("`a.p` belongs to a `def` with no `size=`, so its walls have no length".to_owned(), false),
        (format!("`a.p` is a door that has no `at=`{OWN}"), true),
        (format!("`a.p` is a door that has `at=middle`, which is not one of center, left, right{OWN}"), true),
        (format!("`a.p` is a door that has an `at=` that is not one of center, left, right (numeric offsets are reserved){OWN}"), true),
        (format!("`a.p` is a door with no wall to open: {NO_WALLS}{OWN}"), true),
        (format!("`a.p` is a door that opens at y=1, the row above the floor slab, which is not inside any wall course (the walls occupy y=7..=10){OWN}"), true),
        (format!("`a.p` is a window that has an `offset=` that is not a non-negative integer that fits in u32{OWN}"), true),
        (format!("`a.p` is a window that has no `y=`{OWN}"), true),
        (format!("`a.p` is a window that has a `y=` that is not a non-negative integer that fits in u32{OWN}"), true),
        (format!("`a.p` is a window that has no `size=WxH`{OWN}"), true),
        (format!("`a.p` is a window that has a `size=` that is not a `WxH` of two positive integers{OWN}"), true),
        (format!("`a.p` is a window that runs past the end of its wall (`offset + size.w` = 2 + 2, wall length 3){OWN}"), true),
        (format!("`a.p` is a window with no wall to cut into: {NO_WALLS}{OWN}"), true),
        (format!("`a.p` is a window whose rows y=0..=0 are not all inside one wall course (the walls occupy y=1..=3){OWN}"), true),
        (format!("`a.p` is a window whose 2 rows from y=4294967295 run past the highest row a build can address (the walls occupy y=1..=3){OWN}"), true),
        ("`a.p` is a door the openings pass did not cut, so a strip would end against the wall — the finding on its own line says why".to_owned(), true),
        ("`a.p` is a window the openings pass did not cut, so a strip would end against the wall — the finding on its own line, or on the material its `mat_slot=` names, says why".to_owned(), true),
        ("`a.p` would sit outside the coordinate range a build can address — bring its `place` closer to the origin".to_owned(), false),
    ];
    let got: Vec<(String, bool)> = every_rejection()
        .iter()
        .map(|r| {
            let note = r.note("a.p");
            (note.message, note.span == Some(3..9))
        })
        .collect();
    assert_eq!(got.len(), expected.len(), "{got:#?}");
    for (got, expected) in got.iter().zip(&expected) {
        assert_eq!(got, expected);
    }
}

#[test]
fn the_member_line_and_the_note_word_a_door_at_fault_alike() {
    for written in written_samples("middle") {
        let deferral = door_at_deferral(&written);
        let note = PortRejection::DoorAt {
            written: written.clone(),
            member: 0..0,
        }
        .note("a.p")
        .message;
        let clause = door_at_clause(&written);
        assert!(
            deferral.starts_with(&format!("door {clause}")),
            "{deferral}"
        );
        assert!(note.contains(&clause), "{note}");
    }
}
