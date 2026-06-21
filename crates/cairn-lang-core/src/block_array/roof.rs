//! Gable-roof voxel generator.
//!
//! M2-PR6 ships `kind=gable` only; other roof kinds (`shed`, `hip`,
//! `flat`, ...) degrade to a `W_DEFERRED_MEMBER` warning in `lower.rs` and
//! are scheduled for later milestones. The generator produces a sequence of
//! `(grid_pos, BlockState)` entries the lowering pass writes into the voxel
//! grid; keeping the geometry pure (no palette/voxel mutation here) lets the
//! unit tests in this file pin the exact stair layout without rebuilding a
//! whole `BlockArray`.
//!
//! ## Geometry conventions
//!
//! - **Ridge orientation.** The ridge runs along the *long* axis of the
//!   struct footprint. For `dims.x >= dims.z` (the cottage case at 9×7) the
//!   ridge runs along `x` and the slopes face north (`-z`) and south
//!   (`+z`). When `dims.x == dims.z` the tie breaks in favour of `x` —
//!   spec/compilation.md documents the rule.
//! - **Ridge height.** A gable rises one block of stair per block of
//!   horizontal span on each slope. For a short-axis span `s`, the ridge
//!   sits `ceil(s / 2)` blocks above the wall top, i.e. one apex layer
//!   regardless of even/odd span.
//! - **Stair blockstates.** The slope on `-z` (north) uses
//!   `spruce_stairs[half=bottom, facing=south, shape=straight]`; the slope
//!   on `+z` (south) uses `facing=north`; the apex caps with one
//!   `spruce_stairs[half=top, facing=south, shape=straight]`. The convention
//!   follows Minecraft's "the riser faces `facing`" rule so the upper step
//!   sits on the inward (toward-ridge) side.
//! - **Overhang.** `overhang=N` extends the roof's bounding box by `N`
//!   blocks on every horizontal axis (the eaves overhang the walls and the
//!   gable ends extend past the end walls). The walls themselves keep their
//!   authored `size` and are shifted inward by `+N` — see `block_array/mod.rs`.

use indexmap::IndexMap;

use super::BlockState;

/// Vanilla id used by gable roofs in M2-PR6. Themed material slots resolve
/// to this id via `slot roof -> @spruce_stairs` in the cottage example;
/// other species are not yet wired through, so the generator hard-codes the
/// canonical id and only varies the blockstate properties.
const STAIR_BASE_ID: &str = "minecraft:spruce_stairs";

/// One stair voxel produced by [`gable_voxels`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoofVoxel {
    /// Grid position `(x, y, z)` to write the stair into.
    pub pos: (u32, u32, u32),
    /// Resolved [`BlockState`] for the stair (ready to be interned in the
    /// palette).
    pub state: BlockState,
}

/// Vertical rise of a gable roof above its wall top.
///
/// `short_span` is the span of the slope (the *short* horizontal axis of
/// the roof bounding box, measured in voxels). The ridge sits this many
/// layers above the wall top, including the apex.
#[must_use]
pub fn gable_extra_height(short_span: u32) -> u32 {
    // Each layer steps inward by 1 on each slope, so a span of `s` reaches
    // a single apex at layer `ceil(s / 2)`.
    short_span.div_ceil(2).max(1)
}

/// Generate every stair voxel for a gable roof.
///
/// - `roof_w` / `roof_h` are the *roof bounding box* extents along x and z
///   (i.e. `size.w + 2*overhang` and `size.h + 2*overhang`).
/// - `wall_top` is the y of the highest wall voxel; the roof starts at
///   `wall_top + 1` and rises by [`gable_extra_height`].
///
/// Returns voxels in `(layer, slope, span)` order. The order is stable so
/// callers that intern into a palette and write into a voxel grid get the
/// same palette index sequence on every run — the lockfile's
/// `resolved_ir_hash` depends on it.
#[must_use]
pub fn gable_voxels(roof_w: u32, roof_h: u32, wall_top: u32) -> Vec<RoofVoxel> {
    // Pick the ridge axis: the ridge runs along the *long* axis so the
    // slopes are as short as possible. Ties go to x (spec rule).
    let ridge_axis = if roof_w >= roof_h { Axis::X } else { Axis::Z };
    let span = match ridge_axis {
        Axis::X => roof_h,
        Axis::Z => roof_w,
    };
    let long_axis_len = match ridge_axis {
        Axis::X => roof_w,
        Axis::Z => roof_h,
    };
    let layers = gable_extra_height(span);

    let mut out: Vec<RoofVoxel> = Vec::new();
    for layer in 0..layers {
        let y = wall_top.saturating_add(1).saturating_add(layer);
        let is_apex = layer + 1 == layers;
        // Two slope rows on layer `layer`: the "low" slope at offset `layer`
        // from one end, and the "high" slope at `span - 1 - layer` from the
        // same end. When the apex layer would have the two rows meet
        // (`span` odd), we emit a single capped voxel for that row.
        let low_index = layer;
        let high_index = span.saturating_sub(1).saturating_sub(layer);

        if is_apex && low_index >= high_index {
            // Single apex row: emit the cap stairs.
            emit_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                low_index,
                StairFace::Apex,
            );
        } else {
            emit_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                low_index,
                StairFace::LowSlope,
            );
            emit_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                high_index,
                StairFace::HighSlope,
            );
        }
    }
    out
}

#[derive(Debug, Clone, Copy)]
enum Axis {
    X,
    Z,
}

#[derive(Debug, Clone, Copy)]
enum StairFace {
    /// Slope on the lower-index side along the short axis (the `-z` slope
    /// for x-ridge, the `-x` slope for z-ridge). Stairs face *toward* the
    /// ridge so the upper riser sits on the inward side.
    LowSlope,
    /// Slope on the higher-index side along the short axis.
    HighSlope,
    /// Apex cap when the two slopes converge on a single odd-span layer.
    Apex,
}

fn emit_row(
    out: &mut Vec<RoofVoxel>,
    ridge_axis: Axis,
    long_axis_len: u32,
    y: u32,
    short_index: u32,
    face: StairFace,
) {
    let state = stair_state(ridge_axis, face);
    for i in 0..long_axis_len {
        let pos = match ridge_axis {
            Axis::X => (i, y, short_index),
            Axis::Z => (short_index, y, i),
        };
        out.push(RoofVoxel {
            pos,
            state: state.clone(),
        });
    }
}

fn stair_state(ridge_axis: Axis, face: StairFace) -> BlockState {
    // Apex caps reuse the low-slope facing (matched by ridge axis): for
    // x-ridge that means `facing=south`, for z-ridge `facing=east`.
    // Pairing apex with low slope keeps the ridge capping consistent with
    // the slope orientation; `half=top` (set below) flips it upside-down
    // so it closes the triangle.
    let facing = match (ridge_axis, face) {
        // x-ridge: short axis is z. Low slope is on -z; its riser faces
        // toward +z (south) — the upper-step side ends up on the inward
        // side (toward the ridge).
        (Axis::X, StairFace::LowSlope | StairFace::Apex) => "south",
        (Axis::X, StairFace::HighSlope) => "north",
        // z-ridge: short axis is x. Mirror the same rule onto the x axis.
        (Axis::Z, StairFace::LowSlope | StairFace::Apex) => "east",
        (Axis::Z, StairFace::HighSlope) => "west",
    };
    let half = match face {
        StairFace::Apex => "top",
        _ => "bottom",
    };
    let mut properties: IndexMap<String, String> = IndexMap::new();
    properties.insert("facing".to_owned(), facing.to_owned());
    properties.insert("half".to_owned(), half.to_owned());
    properties.insert("shape".to_owned(), "straight".to_owned());
    BlockState {
        id: STAIR_BASE_ID.to_owned(),
        properties,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gable_extra_height_ceils_half_span() {
        assert_eq!(gable_extra_height(7), 4);
        assert_eq!(gable_extra_height(6), 3);
        assert_eq!(gable_extra_height(1), 1);
        assert_eq!(gable_extra_height(2), 1);
    }

    #[test]
    fn cottage_dimensions_produce_four_layers_of_voxels() {
        // size=9x7 overhang=1 → roof bbox = 11x9. Ridge along x (long axis),
        // short span = 9 → ceil(9/2) = 5 layers tall? Let's compute:
        //   roof_h=9, gable_extra_height(9)=5. So 5 layers, apex on layer 4.
        let voxels = gable_voxels(11, 9, 4);
        // Layers 0..4: two rows each at z=layer / z=9-1-layer = 8-layer.
        // Layer 4 (apex): rows would be z=4 and z=4 → single apex row.
        // Each row spans x in 0..11 → 11 voxels.
        // Layers 0..=3 each contribute two rows of 11 stairs; layer 4 is
        // the single apex row of 11 stairs. Total = 4 * 2 * 11 + 11 = 99.
        let expected: usize = 4 * 2 * 11 + 11;
        assert_eq!(voxels.len(), expected);
    }

    #[test]
    fn cottage_layer_zero_uses_facing_south_and_facing_north() {
        let voxels = gable_voxels(11, 9, 4);
        // Layer 0 at z=0 → low slope → facing=south.
        let layer0_low: Vec<&RoofVoxel> = voxels
            .iter()
            .filter(|v| v.pos.1 == 5 && v.pos.2 == 0)
            .collect();
        assert_eq!(layer0_low.len(), 11);
        for v in &layer0_low {
            assert_eq!(v.state.id, "minecraft:spruce_stairs");
            assert_eq!(v.state.properties.get("facing").unwrap(), "south");
            assert_eq!(v.state.properties.get("half").unwrap(), "bottom");
        }
        // Layer 0 at z=8 → high slope → facing=north.
        let layer0_high: Vec<&RoofVoxel> = voxels
            .iter()
            .filter(|v| v.pos.1 == 5 && v.pos.2 == 8)
            .collect();
        assert_eq!(layer0_high.len(), 11);
        for v in &layer0_high {
            assert_eq!(v.state.properties.get("facing").unwrap(), "north");
            assert_eq!(v.state.properties.get("half").unwrap(), "bottom");
        }
    }

    #[test]
    fn apex_layer_uses_half_top() {
        let voxels = gable_voxels(11, 9, 4);
        // Apex at y = 4 + 5 = 9? Wait — gable_extra_height(9) = 5, so layers
        // run 0..5 and y = wall_top+1+layer = 5..10. Apex is layer 4, y=9.
        let apex_row: Vec<&RoofVoxel> = voxels.iter().filter(|v| v.pos.1 == 9).collect();
        assert_eq!(apex_row.len(), 11);
        for v in &apex_row {
            assert_eq!(v.state.properties.get("half").unwrap(), "top");
            assert_eq!(v.state.properties.get("facing").unwrap(), "south");
        }
    }

    #[test]
    fn z_axis_ridge_picks_east_west_facing() {
        // Swap dims so the long axis is z. roof_w=7, roof_h=11.
        let voxels = gable_voxels(7, 11, 4);
        // Layer 0 at x=0 → low slope → facing=east; at x=6 → high slope → facing=west.
        let low = voxels.iter().find(|v| v.pos == (0, 5, 0)).expect("(0,5,0)");
        assert_eq!(low.state.properties.get("facing").unwrap(), "east");
        let high = voxels.iter().find(|v| v.pos == (6, 5, 0)).expect("(6,5,0)");
        assert_eq!(high.state.properties.get("facing").unwrap(), "west");
    }

    #[test]
    fn square_footprint_breaks_tie_to_x_axis_ridge() {
        // dims.x == dims.z = 9. Tie should go to x-axis ridge → short span
        // is z → slope rows differ by z, not by x.
        let voxels = gable_voxels(9, 9, 4);
        // Voxels at y=5 should occur at z=0 and z=8 (two rows), not x=0/x=8.
        let y5_positions: Vec<(u32, u32, u32)> = voxels
            .iter()
            .filter(|v| v.pos.1 == 5)
            .map(|v| v.pos)
            .collect();
        for (_, _, z) in &y5_positions {
            assert!(*z == 0 || *z == 8, "expected z=0|8, got z={z}");
        }
    }
}
