use std::collections::BTreeSet;

use super::*;
use crate::intent::MemberRole;

// `tests/lower/conditional_arguments.rs` reads the table, so only this sees a kind with no row.
#[test]
fn every_roof_kind_the_dispatch_knows_has_an_arm() {
    let conditional = MemberRole::Roof.conditional_arguments();
    let [axis] = conditional else {
        panic!("`roof` dispatches on one selector, got {conditional:?}");
    };
    let from_dispatch: BTreeSet<&str> = RoofKind::ALL.iter().map(|kind| kind.name()).collect();
    let from_table: BTreeSet<&str> = axis
        .arms
        .iter()
        .map(|arm| {
            arm.value
                .ident()
                .expect("every `kind=` arm names an identifier; a roof with none defers")
        })
        .collect();
    assert_eq!(
        from_dispatch, from_table,
        "`RoofKind` and `MemberRole::Roof.conditional_arguments()` disagree about the kinds",
    );
    for name in from_table {
        assert!(
            RoofKind::from_ident(name).is_some(),
            "`kind={name}` has an arm and is not a kind `from_ident` knows",
        );
    }
}

#[test]
fn gable_extra_height_ceils_half_span() {
    assert_eq!(gable_extra_height(7), 4);
    assert_eq!(gable_extra_height(6), 3);
    assert_eq!(gable_extra_height(1), 1);
    assert_eq!(gable_extra_height(2), 1);
}

#[test]
fn cottage_dimensions_produce_99_voxels() {
    // An 11x9 box: four layers of two 11-stair rows, then one apex row.
    let voxels = gable_voxels(11, 9, 4);
    let expected: usize = 4 * 2 * 11 + 11;
    assert_eq!(voxels.len(), expected);
}

#[test]
fn cottage_layer_zero_uses_low_and_high_slope_faces() {
    let voxels = gable_voxels(11, 9, 4);
    let layer0_low: Vec<&GableVoxel> = voxels
        .iter()
        .filter(|v| v.pos.1 == 5 && v.pos.2 == 0)
        .collect();
    assert_eq!(layer0_low.len(), 11);
    for v in &layer0_low {
        assert_eq!(v.face, StairFace::LowSlope);
    }
    let state = gable_stair_state(Axis::X, StairFace::LowSlope);
    assert_eq!(state.id, "minecraft:spruce_stairs");
    assert_eq!(state.properties.get("facing").unwrap(), "south");
    assert_eq!(state.properties.get("half").unwrap(), "bottom");

    let layer0_high: Vec<&GableVoxel> = voxels
        .iter()
        .filter(|v| v.pos.1 == 5 && v.pos.2 == 8)
        .collect();
    assert_eq!(layer0_high.len(), 11);
    for v in &layer0_high {
        assert_eq!(v.face, StairFace::HighSlope);
    }
    assert_eq!(
        gable_stair_state(Axis::X, StairFace::HighSlope)
            .properties
            .get("facing")
            .unwrap(),
        "north",
    );
}

#[test]
fn odd_span_apex_caps_with_single_top_stair() {
    let voxels = gable_voxels(11, 9, 4);
    let apex_row: Vec<&GableVoxel> = voxels.iter().filter(|v| v.pos.1 == 9).collect();
    assert_eq!(apex_row.len(), 11);
    for v in &apex_row {
        assert_eq!(v.face, StairFace::Apex);
        assert_eq!(v.pos.2, 4);
    }
    let state = gable_stair_state(Axis::X, StairFace::Apex);
    assert_eq!(state.properties.get("half").unwrap(), "top");
    assert_eq!(state.properties.get("facing").unwrap(), "south");
}

#[test]
fn even_span_apex_caps_both_rows_with_half_top() {
    let voxels = gable_voxels(8, 4, 4);
    let apex: Vec<&GableVoxel> = voxels.iter().filter(|v| v.pos.1 == 6).collect();
    assert_eq!(apex.len(), 16);
    let z_values: Vec<u32> = apex.iter().map(|v| v.pos.2).collect();
    assert!(z_values.contains(&1), "low apex row at z=1 missing");
    assert!(z_values.contains(&2), "high apex row at z=2 missing");
    for v in apex {
        assert!(matches!(v.face, StairFace::ApexLow | StairFace::ApexHigh));
        let state = gable_stair_state(Axis::X, v.face);
        assert_eq!(
            state.properties.get("half").unwrap(),
            "top",
            "even-span apex must use half=top",
        );
    }
}

#[test]
fn z_axis_ridge_picks_east_west_facing() {
    let voxels = gable_voxels(7, 11, 4);
    let low = voxels.iter().find(|v| v.pos == (0, 5, 0)).expect("(0,5,0)");
    assert_eq!(low.face, StairFace::LowSlope);
    assert_eq!(
        gable_stair_state(Axis::Z, StairFace::LowSlope)
            .properties
            .get("facing")
            .unwrap(),
        "east",
    );
    let high = voxels.iter().find(|v| v.pos == (6, 5, 0)).expect("(6,5,0)");
    assert_eq!(high.face, StairFace::HighSlope);
    assert_eq!(
        gable_stair_state(Axis::Z, StairFace::HighSlope)
            .properties
            .get("facing")
            .unwrap(),
        "west",
    );
}

#[test]
fn square_footprint_breaks_tie_to_x_axis_ridge() {
    let voxels = gable_voxels(9, 9, 4);
    let y5_positions: Vec<(u32, u32, u32)> = voxels
        .iter()
        .filter(|v| v.pos.1 == 5)
        .map(|v| v.pos)
        .collect();
    for (_, _, z) in &y5_positions {
        assert!(*z == 0 || *z == 8, "expected z=0|8, got z={z}");
    }
}

#[test]
fn parse_roof_kind_recognises_four_idents() {
    assert_eq!(RoofKind::from_ident("gable"), Some(RoofKind::Gable));
    assert_eq!(RoofKind::from_ident("shed"), Some(RoofKind::Shed));
    assert_eq!(RoofKind::from_ident("hip"), Some(RoofKind::Hip));
    assert_eq!(RoofKind::from_ident("flat"), Some(RoofKind::Flat));
    assert_eq!(RoofKind::from_ident("pyramid"), None);
    assert_eq!(RoofKind::from_ident(""), None);
}

#[test]
fn roof_kind_base_block_id_picks_stairs_or_planks() {
    assert_eq!(RoofKind::Gable.base_block_id(), STAIR_BASE_ID);
    assert_eq!(RoofKind::Shed.base_block_id(), STAIR_BASE_ID);
    assert_eq!(RoofKind::Hip.base_block_id(), STAIR_BASE_ID);
    assert_eq!(RoofKind::Flat.base_block_id(), FLAT_BASE_ID);
}

#[test]
fn shed_extra_height_equals_slope_span() {
    assert_eq!(shed_extra_height(7), 7);
    assert_eq!(shed_extra_height(1), 1);
    assert_eq!(shed_extra_height(0), 1);
}

#[test]
fn shed_voxels_climb_one_layer_per_step_toward_high_side() {
    let voxels = shed_voxels(5, 4, 4, WallSide::Front);
    assert_eq!(voxels.len(), 4 * 5);
    let l0: Vec<&ShedVoxel> = voxels.iter().filter(|v| v.pos.1 == 5).collect();
    assert_eq!(l0.len(), 5);
    for v in &l0 {
        assert_eq!(v.pos.2, 0, "layer 0 sits at the low edge (z=0)");
        assert_eq!(v.face, ShedFace::Slope);
    }
    let apex: Vec<&ShedVoxel> = voxels.iter().filter(|v| v.pos.1 == 8).collect();
    assert_eq!(apex.len(), 5);
    for v in &apex {
        assert_eq!(v.pos.2, 3, "apex sits at the high edge (z=slope_span-1)");
        assert_eq!(v.face, ShedFace::Apex);
    }
    let slope_state = shed_stair_state(WallSide::Front, ShedFace::Slope);
    assert_eq!(slope_state.properties.get("facing").unwrap(), "south");
    assert_eq!(slope_state.properties.get("half").unwrap(), "bottom");
    let apex_state = shed_stair_state(WallSide::Front, ShedFace::Apex);
    assert_eq!(apex_state.properties.get("half").unwrap(), "top");
}

#[test]
fn shed_voxels_left_high_runs_along_x() {
    let voxels = shed_voxels(4, 5, 3, WallSide::Left);
    let l0: Vec<&ShedVoxel> = voxels.iter().filter(|v| v.pos.1 == 4).collect();
    assert_eq!(l0.len(), 5);
    for v in &l0 {
        assert_eq!(v.pos.0, 3, "low side at x=3 for slope_to=left");
    }
    let state = shed_stair_state(WallSide::Left, ShedFace::Slope);
    assert_eq!(state.properties.get("facing").unwrap(), "west");
}

#[test]
fn hip_extra_height_matches_gable_short_span_formula() {
    assert_eq!(hip_extra_height(11, 9), 5); // ceil(9/2)
    assert_eq!(hip_extra_height(11, 11), 6);
    assert_eq!(hip_extra_height(5, 5), 3);
}

#[test]
fn hip_square_5x5_caps_with_single_apex_cell() {
    let voxels = hip_voxels(5, 5, 4);
    let apex_y = 4 + hip_extra_height(5, 5);
    let apex: Vec<&HipVoxel> = voxels.iter().filter(|v| v.pos.1 == apex_y).collect();
    assert_eq!(apex.len(), 1, "odd-span square hip apex is one cell");
    assert_eq!(apex[0].pos, (2, apex_y, 2));
    assert_eq!(apex[0].face, HipFace::ApexCap);
}

#[test]
fn hip_square_4x4_caps_with_two_by_two_apex() {
    let voxels = hip_voxels(4, 4, 3);
    let apex_y = 3 + hip_extra_height(4, 4);
    let apex: Vec<&HipVoxel> = voxels.iter().filter(|v| v.pos.1 == apex_y).collect();
    assert_eq!(apex.len(), 4, "even-span square hip apex is 2x2");
    for v in apex {
        assert_eq!(v.face, HipFace::ApexCap);
    }
}

#[test]
fn hip_rectangular_even_short_span_emits_two_row_ridge_band_single_facing() {
    // The whole band takes one facing (`spec/compilation` "Hip roof voxel rules"), unlike a
    // gable's even-span pair.
    let voxels = hip_voxels(8, 4, 4);
    let apex_y = 4 + hip_extra_height(8, 4);
    assert_eq!(apex_y, 6);
    let apex: Vec<&HipVoxel> = voxels.iter().filter(|v| v.pos.1 == apex_y).collect();
    assert_eq!(apex.len(), 6 * 2);
    let z_vals: std::collections::BTreeSet<u32> = apex.iter().map(|v| v.pos.2).collect();
    assert_eq!(z_vals, std::collections::BTreeSet::from([1, 2]));
    for v in &apex {
        assert_eq!(v.face, HipFace::ApexRidge);
    }
    let state = hip_stair_state(Axis::X, HipFace::ApexRidge);
    assert_eq!(state.properties.get("facing").unwrap(), "south");
    assert_eq!(state.properties.get("half").unwrap(), "top");
}

#[test]
fn hip_rectangular_emits_ridge_row_on_long_axis() {
    let voxels = hip_voxels(9, 5, 4);
    let apex_y = 4 + 3;
    let apex: Vec<&HipVoxel> = voxels.iter().filter(|v| v.pos.1 == apex_y).collect();
    assert_eq!(apex.len(), 5);
    let z_vals: std::collections::BTreeSet<u32> = apex.iter().map(|v| v.pos.2).collect();
    assert_eq!(z_vals, std::collections::BTreeSet::from([2]));
    for v in apex {
        assert_eq!(v.face, HipFace::ApexRidge);
    }
}

#[test]
fn hip_layer_zero_has_four_corners_and_four_edge_rows() {
    let voxels = hip_voxels(9, 5, 4);
    let l0: Vec<&HipVoxel> = voxels.iter().filter(|v| v.pos.1 == 5).collect();
    assert_eq!(l0.len(), 2 * 9 + 2 * 5 - 4);
    assert!(
        l0.iter()
            .any(|v| v.pos == (0, 5, 0) && v.face == HipFace::CornerNorthWest)
    );
    assert!(
        l0.iter()
            .any(|v| v.pos == (8, 5, 0) && v.face == HipFace::CornerNorthEast)
    );
    assert!(
        l0.iter()
            .any(|v| v.pos == (0, 5, 4) && v.face == HipFace::CornerSouthWest)
    );
    assert!(
        l0.iter()
            .any(|v| v.pos == (8, 5, 4) && v.face == HipFace::CornerSouthEast)
    );
}

#[test]
fn hip_corner_state_uses_outer_left_or_right() {
    let nw = hip_stair_state(Axis::X, HipFace::CornerNorthWest);
    assert_eq!(nw.properties.get("facing").unwrap(), "south");
    assert_eq!(nw.properties.get("shape").unwrap(), "outer_left");
    let ne = hip_stair_state(Axis::X, HipFace::CornerNorthEast);
    assert_eq!(ne.properties.get("shape").unwrap(), "outer_right");
    let sw = hip_stair_state(Axis::X, HipFace::CornerSouthWest);
    assert_eq!(sw.properties.get("facing").unwrap(), "north");
    assert_eq!(sw.properties.get("shape").unwrap(), "outer_right");
}

#[test]
fn flat_extra_height_is_one() {
    assert_eq!(flat_extra_height(), 1);
}

#[test]
fn flat_voxels_cover_full_inflated_bbox_at_single_layer() {
    let voxels = flat_voxels(11, 9, 4);
    assert_eq!(voxels.len(), 11 * 9);
    for (x, y, z) in &voxels {
        assert_eq!(*y, 5, "flat layer sits at wall_top + 1");
        assert!(*x < 11 && *z < 9);
    }
}

#[test]
fn the_stair_family_is_keyed_off_the_identifier_path() {
    for id in [
        "minecraft:oak_stairs",
        "minecraft:dark_oak_stairs",
        "oak_stairs",
        "create:brass_stairs",
    ] {
        assert!(is_stair(id), "`{id}` is a stair");
    }
    for id in [
        "minecraft:cobblestone",
        "minecraft:stairs",
        "mystairs",
        "minecraft:oak_stairs_slab",
        "weird_stairs:cobblestone",
        "create:cogwheel",
        "",
    ] {
        assert!(!is_stair(id), "`{id}` is not a stair");
    }
}

#[test]
fn only_the_kinds_that_attach_states_require_a_stair() {
    for kind in [RoofKind::Gable, RoofKind::Shed, RoofKind::Hip] {
        assert!(kind.paints_stairs(), "{kind:?}");
        assert!(
            is_stair(kind.base_block_id()),
            "{kind:?}: its own fallback must pass the check it imposes",
        );
    }
    assert!(
        !RoofKind::Flat.paints_stairs(),
        "a flat deck attaches no states, so every block is as valid as another",
    );
}

#[test]
fn flat_block_state_is_spruce_planks() {
    let bs = flat_block_state();
    assert_eq!(bs.id, FLAT_BASE_ID);
    assert!(bs.properties.is_empty());
}
