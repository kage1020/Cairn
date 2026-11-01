//! A roof's first course sits on the wall top even when it is the only course, and an even-span
//! gable's or hip's cap pair faces outward: `spec/compilation` "Hip roof voxel rules" and "Gable
//! roof voxel rules".

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, BlockState, lower_to_block_array};
use cairn_lang_core::{lower, parse, resolve};

use crate::common::state_at;

/// A struct of `walls height=3` and one roof, whose first course lands at `y = 4`.
fn roof(size: &str, kind: &str) -> BlockArrayIr {
    let src = format!(
        "theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot roof -> @spruce_stairs\n\n\
         struct s size={size}\n  \
         walls mat_slot=wall height=3\n  \
         roof  kind={kind} mat_slot=roof overhang=0\n",
    );
    let module = parse(&src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let out = lower_to_block_array(&ir, &resolution, None);
    assert!(
        out.diagnostics.is_empty(),
        "these roofs lower cleanly: {:?}",
        out.diagnostics,
    );
    out
}

fn structure(ir: &BlockArrayIr) -> &BlockArray {
    ir.structures.values().next().expect("one structure")
}

/// `(facing, half, shape)` of the stair at a cell.
fn stair_at(ba: &BlockArray, x: u32, y: u32, z: u32) -> (&str, &str, &str) {
    let state = state_at(ba, x, y, z);
    assert_eq!(state.id, "minecraft:spruce_stairs", "at ({x}, {y}, {z})");
    let get = |k: &str| state.properties.get(k).map_or("", String::as_str);
    (get("facing"), get("half"), get("shape"))
}

/// The distinct stair states the whole grid carries, as sorted triples.
fn stair_states(ba: &BlockArray) -> Vec<(&str, &str, &str)> {
    let mut out: Vec<_> = ba
        .palette
        .entries
        .iter()
        .filter(|s| s.id == "minecraft:spruce_stairs")
        .map(|s| {
            let get = |k: &str| s.properties.get(k).map_or("", String::as_str);
            (get("facing"), get("half"), get("shape"))
        })
        .collect();
    out.sort_unstable();
    out
}

// ------------------------------------------ one-layer hip: the frame

#[test]
fn a_hip_one_course_tall_frames_its_only_layer() {
    // `size=8x2` rises one layer: four corners and two opposing slope rows.
    let out = roof("8x2", "hip");
    let ba = structure(&out);
    assert_eq!(stair_at(ba, 0, 4, 0), ("south", "bottom", "outer_left"));
    assert_eq!(stair_at(ba, 7, 4, 0), ("south", "bottom", "outer_right"));
    assert_eq!(stair_at(ba, 0, 4, 1), ("north", "bottom", "outer_right"));
    assert_eq!(stair_at(ba, 7, 4, 1), ("north", "bottom", "outer_left"));
    for x in 1..7 {
        assert_eq!(stair_at(ba, x, 4, 0), ("south", "bottom", "straight"));
        assert_eq!(stair_at(ba, x, 4, 1), ("north", "bottom", "straight"));
    }
    assert!(
        stair_states(ba)
            .iter()
            .all(|(_, half, _)| *half == "bottom"),
        "{:?}",
        stair_states(ba),
    );
}

#[test]
fn a_hip_one_course_tall_on_the_other_axis_frames_its_columns() {
    // Two cells wide on x, so the frame is a west and an east column.
    let out = roof("2x8", "hip");
    let ba = structure(&out);
    assert_eq!(stair_at(ba, 0, 4, 0), ("south", "bottom", "outer_left"));
    assert_eq!(stair_at(ba, 1, 4, 0), ("south", "bottom", "outer_right"));
    assert_eq!(stair_at(ba, 0, 4, 7), ("north", "bottom", "outer_right"));
    assert_eq!(stair_at(ba, 1, 4, 7), ("north", "bottom", "outer_left"));
    for z in 1..7 {
        assert_eq!(stair_at(ba, 0, 4, z), ("east", "bottom", "straight"));
        assert_eq!(stair_at(ba, 1, 4, z), ("west", "bottom", "straight"));
    }
}

#[test]
fn a_hip_over_a_single_voxel_is_one_corner() {
    let out = roof("1x1", "hip");
    let ba = structure(&out);
    assert_eq!(stair_at(ba, 0, 4, 0), ("south", "bottom", "outer_left"));
    assert_eq!(stair_states(ba), [("south", "bottom", "outer_left")]);
}

#[test]
fn a_hip_over_a_two_by_two_is_four_corners() {
    let out = roof("2x2", "hip");
    let ba = structure(&out);
    assert_eq!(stair_at(ba, 0, 4, 0), ("south", "bottom", "outer_left"));
    assert_eq!(stair_at(ba, 1, 4, 0), ("south", "bottom", "outer_right"));
    assert_eq!(stair_at(ba, 0, 4, 1), ("north", "bottom", "outer_right"));
    assert_eq!(stair_at(ba, 1, 4, 1), ("north", "bottom", "outer_left"));
    assert_eq!(stair_states(ba).len(), 4);
}

// --------------------------------------- one-layer gable: two slopes

#[test]
fn a_gable_one_course_tall_seats_both_slopes_on_the_wall() {
    let out = roof("6x2", "gable");
    let ba = structure(&out);
    for x in 0..6 {
        assert_eq!(stair_at(ba, x, 4, 0), ("south", "bottom", "straight"));
        assert_eq!(stair_at(ba, x, 4, 1), ("north", "bottom", "straight"));
    }
    assert_eq!(
        stair_states(ba),
        [
            ("north", "bottom", "straight"),
            ("south", "bottom", "straight"),
        ],
    );
}

#[test]
fn a_gable_over_a_one_deep_footprint_emits_its_row_once() {
    // Span 1: both slopes converge on layer 0, which is neither an apex nor a pair.
    let out = roof("6x1", "gable");
    let ba = structure(&out);
    for x in 0..6 {
        assert_eq!(stair_at(ba, x, 4, 0), ("south", "bottom", "straight"));
    }
    assert_eq!(stair_states(ba), [("south", "bottom", "straight")]);
}

// ------------------------------- the even-span cap faces away from the ridge

#[test]
fn an_even_span_gable_caps_the_ridge_with_a_pair_facing_outward() {
    // Slopes at y=4 on z=0 and z=3, and the cap pair at y=5 on z=1 and z=2.
    let out = roof("6x4", "gable");
    let ba = structure(&out);
    for x in 0..6 {
        assert_eq!(stair_at(ba, x, 4, 0), ("south", "bottom", "straight"));
        assert_eq!(stair_at(ba, x, 4, 3), ("north", "bottom", "straight"));
        assert_eq!(stair_at(ba, x, 5, 1), ("north", "top", "straight"));
        assert_eq!(stair_at(ba, x, 5, 2), ("south", "top", "straight"));
    }
}

#[test]
fn an_even_span_gable_on_a_z_ridge_mirrors_the_outward_pair() {
    // The ridge runs along z, so the cap pair straddles x=1 and x=2.
    let out = roof("4x6", "gable");
    let ba = structure(&out);
    for z in 0..6 {
        assert_eq!(stair_at(ba, 0, 4, z), ("east", "bottom", "straight"));
        assert_eq!(stair_at(ba, 3, 4, z), ("west", "bottom", "straight"));
        assert_eq!(stair_at(ba, 1, 5, z), ("west", "top", "straight"));
        assert_eq!(stair_at(ba, 2, 5, z), ("east", "top", "straight"));
    }
}

#[test]
fn an_odd_span_gable_keeps_the_low_slope_facing_on_its_single_cap() {
    // `spec/compilation` "Gable roof voxel rules": a single apex cap uses the low-slope facing.
    let out = roof("7x5", "gable");
    let ba = structure(&out);
    for x in 0..7 {
        assert_eq!(stair_at(ba, x, 6, 2), ("south", "top", "straight"));
    }
    assert_eq!(
        stair_states(ba),
        [
            ("north", "bottom", "straight"),
            ("south", "bottom", "straight"),
            ("south", "top", "straight"),
        ],
        "an odd span emits three faces and never the apex pair",
    );
}

#[test]
fn a_hip_taller_than_one_course_frames_below_and_caps_with_an_outward_pair() {
    // The frame at y=4 and the `half=top` ridge band at y=5.
    let out = roof("8x4", "hip");
    let ba = structure(&out);
    assert_eq!(stair_at(ba, 0, 4, 0), ("south", "bottom", "outer_left"));
    assert_eq!(stair_at(ba, 0, 4, 1), ("east", "bottom", "straight"));
    assert_eq!(stair_at(ba, 7, 4, 3), ("north", "bottom", "outer_left"));
    for x in 1..7 {
        assert_eq!(stair_at(ba, x, 5, 1), ("north", "top", "straight"));
        assert_eq!(stair_at(ba, x, 5, 2), ("south", "top", "straight"));
    }
}

#[test]
fn an_even_span_hip_on_a_z_ridge_mirrors_the_outward_pair() {
    let out = roof("4x8", "hip");
    let ba = structure(&out);
    for z in 1..7 {
        assert_eq!(stair_at(ba, 1, 5, z), ("west", "top", "straight"));
        assert_eq!(stair_at(ba, 2, 5, z), ("east", "top", "straight"));
    }
}

#[test]
fn a_square_even_span_hip_caps_with_the_x_ridge_pair() {
    let out = roof("4x4", "hip");
    let ba = structure(&out);
    for x in 1..3 {
        assert_eq!(stair_at(ba, x, 5, 1), ("north", "top", "straight"));
        assert_eq!(stair_at(ba, x, 5, 2), ("south", "top", "straight"));
    }
}

// ----------------------------------------- an odd-span hip caps with one row

#[test]
fn an_odd_square_hip_caps_with_a_single_apex_cell() {
    // Three layers on a 5x5: frames at y=4 and y=5, one apex cell at y=6.
    let out = roof("5x5", "hip");
    let ba = structure(&out);
    assert_eq!(stair_at(ba, 0, 4, 0), ("south", "bottom", "outer_left"));
    assert_eq!(stair_at(ba, 1, 5, 1), ("south", "bottom", "outer_left"));
    assert_eq!(stair_at(ba, 2, 6, 2), ("south", "top", "straight"));
    assert_eq!(state_at(ba, 1, 6, 2).id, BlockState::AIR_ID);
}

#[test]
fn an_odd_span_rectangular_hip_caps_with_a_ridge_row() {
    // 9x5: frames at y=4 and y=5, and a five-cell ridge along x at y=6.
    let out = roof("9x5", "hip");
    let ba = structure(&out);
    for x in 2..7 {
        assert_eq!(stair_at(ba, x, 6, 2), ("south", "top", "straight"));
    }
    assert_eq!(state_at(ba, 1, 6, 2).id, BlockState::AIR_ID);
    assert_eq!(state_at(ba, 7, 6, 2).id, BlockState::AIR_ID);
}
