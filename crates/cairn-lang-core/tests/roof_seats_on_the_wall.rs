//! A roof's first course sits on the wall top, and a roof that is only
//! one course tall is still a first course.
//!
//! `spec/compilation.md` §4.5 lays a hip roof out as the inset rectangle
//! frame at every layer `L`, with `outer_*` corners so the diagonals
//! close, and caps whatever the frames leave. §4.3 does the same for a
//! gable with two slope rows per layer. A short span of 1 or 2 rises one
//! layer, which made layer 0 the apex layer as well — and the apex
//! branch ran instead of the frame, so a whole roof came out `half=top`.
//!
//! A `half=top` stair fills the upper half of its voxel (plus the lower
//! quarter on its facing side), so a course of them one voxel above the
//! wall leaves a half-block slit running the whole perimeter. For a hip it also drops the four `outer_*` corners
//! and every per-edge facing: sixteen cells that should have been four
//! corners and two slope rows came out as one repeated state.
//!
//! The even-span gable cap is the other half of the same subject. The
//! generator's own comment said the pair faced outward "so the cap
//! closes"; the table faced them at each other, which leaves the
//! `half=top` stair's open half on the *outer* face for the roof's full
//! length. Facing them outward moves that void under the ridge.
//!
//! The last group pins the footprints none of this touches. A change to
//! the degenerate case that also moved a three-layer roof would be a
//! regression these fixtures catch and the ones above cannot.

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, BlockState, lower_to_block_array};
use cairn_lang_core::{lower, parse, resolve};

/// A struct whose only members are `walls height=3` and one roof, so the
/// roof's first course always lands at `y = 4`.
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

fn state_at(ba: &BlockArray, x: u32, y: u32, z: u32) -> &BlockState {
    let i = ((y * ba.dims.z + z) * ba.dims.x + x) as usize;
    &ba.palette.entries[ba.voxels[i].0 as usize]
}

/// `(facing, half, shape)` of the stair at a cell, so a fixture reads as
/// the geometry rather than as three lookups.
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
    // The issue's third repro. `size=8x2` rises one layer, and every one
    // of its sixteen cells used to be `facing=south, half=top,
    // shape=straight` — one state for a course that is four corners and
    // two opposing slope rows.
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
    // Nothing anywhere is `half=top`: a cap closes a peak, and this roof
    // has no course under it to raise one.
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
    // The mirror of the case above. A frame two cells wide on x emits its
    // west and east *columns* and no north / south rows, so a fix that
    // only special-cased the row direction would leave this one capped.
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
    // The smallest footprint there is. The frame collapses to its
    // north-west corner and emits it once — the guards in the frame
    // emitter are what keep it from writing the same cell four times,
    // which the phase model would report as the member contesting itself.
    let out = roof("1x1", "hip");
    let ba = structure(&out);
    assert_eq!(stair_at(ba, 0, 4, 0), ("south", "bottom", "outer_left"));
    assert_eq!(stair_states(ba), [("south", "bottom", "outer_left")]);
}

#[test]
fn a_hip_over_a_two_by_two_is_four_corners() {
    // Square and even, so this used to take the `2x2` apex-cap branch and
    // come out as four `half=top` straights. Every cell of a 2x2 frame is
    // a corner.
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
    // Span 1: the two slopes converge on layer 0, which is neither an
    // apex nor a pair. Emitting both faces at the same index would make
    // the roof contest its own cell; emitting the apex face would cap a
    // course that sits on the wall.
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
    // `size=6x4` rises two layers: slopes at y=4 on z=0 and z=3, and the
    // cap pair at y=5 on z=1 and z=2 with the ridge between them. The
    // pair used to point at each other, leaving the open half of each
    // `half=top` stair on the outer face — an undercut running the length
    // of the roof, on both sides.
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
    // `size=4x6`: the ridge runs along z, so the short axis is x and the
    // cap pair straddles it at x=1 and x=2. The facing rule is the same
    // rule on the other axis, and a fix applied to one arm of the table
    // and not the other would pass the x-ridge fixture alone.
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
    // §4.3: "The apex caps with a single stair at `half=top` using the
    // low-slope facing." A converged cap is one cell wide, so both of its
    // faces are outer ones and a stair serves only one — the outward rule
    // above has nothing to choose here, and the undercut it removes for a
    // pair is unavoidable for a single. That is why this is a face of its
    // own rather than the pair's low half.
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

// ------------------------------------------- the footprints that do not move

#[test]
fn a_hip_taller_than_one_course_frames_below_and_caps_above() {
    // Two layers: the frame at y=4 and the `half=top` ridge band at y=5.
    // Seating layer 0 must not reach the layers above it — the apex
    // branch is what closes a roof that has a course under it.
    let out = roof("8x4", "hip");
    let ba = structure(&out);
    assert_eq!(stair_at(ba, 0, 4, 0), ("south", "bottom", "outer_left"));
    assert_eq!(stair_at(ba, 0, 4, 1), ("east", "bottom", "straight"));
    assert_eq!(stair_at(ba, 7, 4, 3), ("north", "bottom", "outer_left"));
    for x in 1..7 {
        for z in 1..3 {
            assert_eq!(stair_at(ba, x, 5, z), ("south", "top", "straight"));
        }
    }
}

#[test]
fn a_square_hip_still_caps_with_a_single_apex_cell() {
    // Three layers on a 5x5: frames at y=4 and y=5, one apex cell at y=6.
    let out = roof("5x5", "hip");
    let ba = structure(&out);
    assert_eq!(stair_at(ba, 0, 4, 0), ("south", "bottom", "outer_left"));
    assert_eq!(stair_at(ba, 1, 5, 1), ("south", "bottom", "outer_left"));
    assert_eq!(stair_at(ba, 2, 6, 2), ("south", "top", "straight"));
    assert_eq!(state_at(ba, 1, 6, 2).id, BlockState::AIR_ID);
}

#[test]
fn a_rectangular_hip_still_caps_with_a_ridge_row() {
    // 9x5: frames at y=4 and y=5, and a five-cell ridge along x at y=6.
    let out = roof("9x5", "hip");
    let ba = structure(&out);
    for x in 2..7 {
        assert_eq!(stair_at(ba, x, 6, 2), ("south", "top", "straight"));
    }
    assert_eq!(state_at(ba, 1, 6, 2).id, BlockState::AIR_ID);
    assert_eq!(state_at(ba, 7, 6, 2).id, BlockState::AIR_ID);
}
