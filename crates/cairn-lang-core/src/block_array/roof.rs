//! Gable-roof voxel generator.
//!
//! Currently only `kind=gable` is supported; other roof kinds (`shed`,
//! `hip`, `flat`, ...) degrade to a `W_DEFERRED_MEMBER` warning in
//! `lower.rs` and are scheduled for later work. The generator produces a
//! sequence of `(grid_pos, face_tag)` entries the lowering pass writes
//! into the voxel grid after interning one [`BlockState`] per face. Keeping
//! the geometry pure (no palette/voxel mutation here) lets the unit tests
//! in this file pin the exact stair layout without rebuilding a whole
//! `BlockArray`, and avoids the per-voxel `BlockState` clone the earlier
//! shape forced on the caller.
//!
//! ## Geometry conventions
//!
//! - **Ridge orientation.** The ridge runs along the *long* axis of the
//!   struct footprint. For `dims.x >= dims.z` (the cottage case at 9×7)
//!   the ridge runs along `x` and the slopes face north (`-z`) and south
//!   (`+z`). When `dims.x == dims.z` the tie breaks in favour of `x` —
//!   spec/compilation.md documents the rule.
//! - **Ridge height.** A gable rises one block of stair per block of
//!   horizontal span on each slope. For a short-axis span `s`, the ridge
//!   sits `ceil(s / 2)` blocks above the wall top.
//! - **Apex topology.** When the span is *odd*, the two slopes converge
//!   on a single mid row and one `half=top` stair caps the ridge.
//!   When the span is *even*, the two slopes meet at two adjacent rows;
//!   both rows are emitted as `half=top` stairs facing outward (one per
//!   slope) so the apex closes instead of leaving an open V.
//! - **Stair blockstates.** The slope on the low-index side (the `-z`
//!   slope for x-ridge, `-x` slope for z-ridge) uses `facing` pointed
//!   toward the ridge — `south` for x-ridge, `east` for z-ridge — so the
//!   upper-step side sits on the inward (toward-ridge) side. The opposite
//!   slope mirrors it (`north` / `west`). Apex stairs reuse those facings
//!   with `half=top`.
//! - **Overhang.** `overhang=N` extends the roof's bounding box by `N`
//!   blocks on every horizontal axis (the eaves overhang the walls and
//!   the gable ends extend past the end walls). The walls themselves keep
//!   their authored `size` and are shifted inward by `+N` — see
//!   `block_array/mod.rs`.

use indexmap::IndexMap;

use super::BlockState;

/// Vanilla id used by gable roofs. Themed material slots resolve to this
/// id via `slot roof -> @spruce_stairs` in the cottage example; other
/// species are not yet wired through, so the generator hard-codes the
/// canonical id and only varies the blockstate properties. `lower.rs`
/// emits a `W_DEFERRED_MEMBER` warning when a `mat_slot=` binding resolves
/// to a different id, so the user is not silently overridden.
pub const STAIR_BASE_ID: &str = "minecraft:spruce_stairs";

/// Which face of the gable a voxel belongs to. The lowering pass interns
/// one [`BlockState`] per variant via [`stair_state_for`] and reuses the
/// resulting [`super::PaletteIndex`] for every [`GableVoxel`] of that face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StairFace {
    /// Slope on the low-index side along the short axis (`-z` for an
    /// x-ridge, `-x` for a z-ridge). `half=bottom`, facing toward the
    /// ridge.
    LowSlope,
    /// Slope on the high-index side along the short axis (`+z` for an
    /// x-ridge, `+x` for a z-ridge). `half=bottom`, facing toward the
    /// ridge from the other side.
    HighSlope,
    /// Apex cap whose facing matches the low slope (used as the single
    /// cap on odd-span apex rows and as the low-side cap on even-span
    /// apex rows). `half=top`.
    ApexLow,
    /// Apex cap whose facing matches the high slope (only emitted on
    /// even-span apex rows so the closing pair forms an upside-down peak).
    /// `half=top`.
    ApexHigh,
}

/// Ridge orientation chosen for a roof bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Ridge runs along `x` (slopes face `±z`).
    X,
    /// Ridge runs along `z` (slopes face `±x`).
    Z,
}

/// One stair voxel produced by [`gable_voxels`].
///
/// Holds only the grid position and the face tag. The caller is expected
/// to intern one [`BlockState`] per face via [`stair_state_for`] up-front
/// and look up the palette index by face tag, so neither this struct nor
/// the voxel-grid write path clones a `BlockState` per cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GableVoxel {
    /// Grid position `(x, y, z)` to write the stair into.
    pub pos: (u32, u32, u32),
    /// Which face this voxel belongs to.
    pub face: StairFace,
}

/// Vertical rise of a gable roof above its wall top.
///
/// `short_span` is the span of the slope (the *short* horizontal axis of
/// the roof bounding box, measured in voxels). The ridge sits this many
/// layers above the wall top, including the apex layer.
#[must_use]
pub fn gable_extra_height(short_span: u32) -> u32 {
    // Each layer steps inward by 1 on each slope, so a span of `s` reaches
    // an apex at layer `ceil(s / 2)`.
    short_span.div_ceil(2).max(1)
}

/// Choose the ridge axis for a given roof bounding box.
///
/// The ridge runs along the longer axis so the slopes stay as short as
/// possible. Ties break to `x` (spec/compilation.md §4.3).
#[must_use]
pub fn gable_ridge_axis(roof_w: u32, roof_h: u32) -> Axis {
    if roof_w >= roof_h { Axis::X } else { Axis::Z }
}

/// Build the [`BlockState`] for one stair variant of a gable roof.
///
/// Pure: the same `(ridge_axis, face)` always produces the same state, so
/// callers can intern a small lookup table once and avoid the per-voxel
/// clone the earlier shape forced.
#[must_use]
pub fn stair_state_for(ridge_axis: Axis, face: StairFace) -> BlockState {
    // Apex caps reuse the slope facings: an `ApexLow` cap keeps the
    // low-slope direction (`south` for x-ridge), an `ApexHigh` keeps the
    // high-slope direction (`north`). Pairing apex with the slope it
    // covers keeps the cap visually continuous with the slope below it,
    // and `half=top` (set below) flips it upside-down so the triangle
    // closes.
    let facing = match (ridge_axis, face) {
        // x-ridge: short axis is z. Low slope is on -z; its riser faces
        // toward +z (south) — the upper-step side ends up on the inward
        // side (toward the ridge).
        (Axis::X, StairFace::LowSlope | StairFace::ApexLow) => "south",
        (Axis::X, StairFace::HighSlope | StairFace::ApexHigh) => "north",
        // z-ridge: short axis is x. Mirror the same rule onto the x axis.
        (Axis::Z, StairFace::LowSlope | StairFace::ApexLow) => "east",
        (Axis::Z, StairFace::HighSlope | StairFace::ApexHigh) => "west",
    };
    let half = match face {
        StairFace::ApexLow | StairFace::ApexHigh => "top",
        StairFace::LowSlope | StairFace::HighSlope => "bottom",
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

/// Generate every stair voxel position for a gable roof.
///
/// - `roof_w` / `roof_h` are the *roof bounding box* extents along x and z
///   (i.e. `size.w + 2*overhang` and `size.h + 2*overhang`).
/// - `wall_top` is the y of the highest wall voxel; the roof starts at
///   `wall_top + 1` and rises by [`gable_extra_height`].
///
/// Returns voxels in `(layer, slope, span)` order. The order is stable so
/// callers that intern face states into a palette and write into a voxel
/// grid get the same palette index sequence on every run — the lockfile's
/// `resolved_ir_hash` depends on it.
#[must_use]
pub fn gable_voxels(roof_w: u32, roof_h: u32, wall_top: u32) -> Vec<GableVoxel> {
    let ridge_axis = gable_ridge_axis(roof_w, roof_h);
    let span = match ridge_axis {
        Axis::X => roof_h,
        Axis::Z => roof_w,
    };
    let long_axis_len = match ridge_axis {
        Axis::X => roof_w,
        Axis::Z => roof_h,
    };
    let layers = gable_extra_height(span);

    let mut out: Vec<GableVoxel> = Vec::new();
    for layer in 0..layers {
        let y = wall_top.saturating_add(1).saturating_add(layer);
        let is_apex = layer + 1 == layers;
        let low_index = layer;
        let high_index = span.saturating_sub(1).saturating_sub(layer);

        if is_apex && low_index == high_index {
            // Odd-span apex: the two slopes converge on a single row.
            // Cap it with one half=top stair using the low-slope facing.
            emit_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                low_index,
                StairFace::ApexLow,
            );
        } else if is_apex {
            // Even-span apex: the slopes meet at two adjacent rows. Emit
            // both as half=top stairs facing outward so the cap closes
            // (the previous shape left an open V because the apex branch
            // was skipped whenever low_index < high_index on the top
            // layer).
            emit_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                low_index,
                StairFace::ApexLow,
            );
            emit_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                high_index,
                StairFace::ApexHigh,
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

fn emit_row(
    out: &mut Vec<GableVoxel>,
    ridge_axis: Axis,
    long_axis_len: u32,
    y: u32,
    short_index: u32,
    face: StairFace,
) {
    for i in 0..long_axis_len {
        let pos = match ridge_axis {
            Axis::X => (i, y, short_index),
            Axis::Z => (short_index, y, i),
        };
        out.push(GableVoxel { pos, face });
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
    fn cottage_dimensions_produce_99_voxels() {
        // size=9x7 overhang=1 → roof bbox = 11x9. Ridge along x (long axis),
        // short span = 9 → gable_extra_height(9) = 5 layers, apex on layer 4
        // (odd span → single apex row).
        // Layers 0..=3 each emit two rows of 11 stairs; layer 4 emits one
        // row of 11 stairs. Total = 4 * 2 * 11 + 11 = 99.
        let voxels = gable_voxels(11, 9, 4);
        let expected: usize = 4 * 2 * 11 + 11;
        assert_eq!(voxels.len(), expected);
    }

    #[test]
    fn cottage_layer_zero_uses_low_and_high_slope_faces() {
        let voxels = gable_voxels(11, 9, 4);
        // Layer 0 at z=0 → LowSlope face → facing=south.
        let layer0_low: Vec<&GableVoxel> = voxels
            .iter()
            .filter(|v| v.pos.1 == 5 && v.pos.2 == 0)
            .collect();
        assert_eq!(layer0_low.len(), 11);
        for v in &layer0_low {
            assert_eq!(v.face, StairFace::LowSlope);
        }
        let state = stair_state_for(Axis::X, StairFace::LowSlope);
        assert_eq!(state.id, "minecraft:spruce_stairs");
        assert_eq!(state.properties.get("facing").unwrap(), "south");
        assert_eq!(state.properties.get("half").unwrap(), "bottom");

        // Layer 0 at z=8 → HighSlope face → facing=north.
        let layer0_high: Vec<&GableVoxel> = voxels
            .iter()
            .filter(|v| v.pos.1 == 5 && v.pos.2 == 8)
            .collect();
        assert_eq!(layer0_high.len(), 11);
        for v in &layer0_high {
            assert_eq!(v.face, StairFace::HighSlope);
        }
        assert_eq!(
            stair_state_for(Axis::X, StairFace::HighSlope)
                .properties
                .get("facing")
                .unwrap(),
            "north",
        );
    }

    #[test]
    fn odd_span_apex_caps_with_single_top_stair() {
        let voxels = gable_voxels(11, 9, 4);
        // Apex y = 4 + gable_extra_height(9) = 9. Span 9 is odd so the
        // apex is a single row at z=4.
        let apex_row: Vec<&GableVoxel> = voxels.iter().filter(|v| v.pos.1 == 9).collect();
        assert_eq!(apex_row.len(), 11);
        for v in &apex_row {
            assert_eq!(v.face, StairFace::ApexLow);
            assert_eq!(v.pos.2, 4);
        }
        let state = stair_state_for(Axis::X, StairFace::ApexLow);
        assert_eq!(state.properties.get("half").unwrap(), "top");
        assert_eq!(state.properties.get("facing").unwrap(), "south");
    }

    #[test]
    fn even_span_apex_caps_both_rows_with_half_top() {
        // span=4 (e.g. interior z=4 with no overhang) → layers=2, apex on
        // layer 1. low=1, high=2 — must produce two half=top rows so the
        // ridge actually closes.
        let voxels = gable_voxels(8, 4, 4);
        // Layer 1 sits at y=6.
        let apex: Vec<&GableVoxel> = voxels.iter().filter(|v| v.pos.1 == 6).collect();
        // Two rows of 8 stairs each.
        assert_eq!(apex.len(), 16);
        let z_values: Vec<u32> = apex.iter().map(|v| v.pos.2).collect();
        assert!(z_values.contains(&1), "low apex row at z=1 missing");
        assert!(z_values.contains(&2), "high apex row at z=2 missing");
        for v in apex {
            assert!(matches!(v.face, StairFace::ApexLow | StairFace::ApexHigh));
            let state = stair_state_for(Axis::X, v.face);
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
            stair_state_for(Axis::Z, StairFace::LowSlope)
                .properties
                .get("facing")
                .unwrap(),
            "east",
        );
        let high = voxels.iter().find(|v| v.pos == (6, 5, 0)).expect("(6,5,0)");
        assert_eq!(high.face, StairFace::HighSlope);
        assert_eq!(
            stair_state_for(Axis::Z, StairFace::HighSlope)
                .properties
                .get("facing")
                .unwrap(),
            "west",
        );
    }

    #[test]
    fn square_footprint_breaks_tie_to_x_axis_ridge() {
        let voxels = gable_voxels(9, 9, 4);
        // Tie should go to x-axis ridge → slope rows differ by z, not by x.
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
