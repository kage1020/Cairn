//! Roof voxel generators.
//!
//! Cairn supports four roof kinds — `gable`, `shed`, `hip`, and `flat`.
//! Each is voxelised by a pure geometry function in this module: the
//! function returns a sequence of `(grid_pos, face_tag)` entries the
//! lowering pass writes into the voxel grid after interning one
//! [`BlockState`] per face tag. Keeping the geometry pure (no
//! palette/voxel mutation here) lets the unit tests pin the exact stair
//! layout without rebuilding a whole `BlockArray`, and avoids a
//! per-voxel `BlockState` clone in the caller.
//!
//! The four kinds share their wall-top and overhang convention, but each
//! one's exact ridge / corner rule lives in `spec/compilation.md` §4.3–4.6
//! and is mirrored by the per-kind generator below.
//!
//! ## Common geometry conventions
//!
//! - **Bounding box.** The roof sits in a `roof_w x roof_h` box where
//!   `roof_w = size.w + 2*overhang` and `roof_h = size.h + 2*overhang`.
//!   Floors, walls, doors, and windows stay on their authored
//!   coordinates and shift inward by `overhang` on x/z; the roof spans
//!   the full inflated box so eaves extend past the wall ring.
//! - **Wall top.** The lowest roof voxel sits at `y = wall_top + 1`.
//!   `wall_top` is the max `height=` across walls members; without
//!   walls, the roof starts at `y = 1`.
//! - **Stair facing.** Sloped roof voxels are `minecraft:spruce_stairs`
//!   with `facing` pointed *toward the ridge* (the riser's upper-step
//!   side ends up on the inward side), `half=bottom`. Apex caps reuse
//!   the slope facings with `half=top` so the upside-down stair closes
//!   the peak. Per-theme roof species follow with the registry pack.
//! - **Material id.** `gable`, `shed`, and `hip` hardcode
//!   `minecraft:spruce_stairs` (see [`STAIR_BASE_ID`]); `flat` hardcodes
//!   [`FLAT_BASE_ID`]. A `mat_slot=` binding that resolves to anything
//!   else fires a `W_DEFERRED_MEMBER` warning in `lower.rs` so the user's
//!   intent is not silently replaced.

use indexmap::IndexMap;

use super::BlockState;
use super::openings::WallSide;

/// Vanilla stair id used by `gable`, `shed`, and `hip` roofs. Themed
/// material slots resolve to this id via `slot roof -> @spruce_stairs` in
/// the cottage example; per-species roofs land with the registry pack.
/// `lower.rs` emits a `W_DEFERRED_MEMBER` warning when a `mat_slot=`
/// binding resolves to a different id, so the user is not silently
/// overridden.
pub const STAIR_BASE_ID: &str = "minecraft:spruce_stairs";

/// Vanilla planks id used by `flat` roofs. Same warn-on-mismatch contract
/// as [`STAIR_BASE_ID`].
pub const FLAT_BASE_ID: &str = "minecraft:spruce_planks";

/// One of the four supported roof kinds.
///
/// Held as an `enum` rather than a string so the lowering dispatcher and
/// the dimension computation cannot drift on the spelling of the kind
/// name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoofKind {
    /// Two opposing stair slopes meeting at a ridge along the long axis.
    Gable,
    /// Single stair slope rising toward a chosen wall side.
    Shed,
    /// Four-sided stair pyramid (square footprint) or pyramid-with-ridge
    /// (rectangular footprint).
    Hip,
    /// Single layer of solid planks above the wall top.
    Flat,
}

impl RoofKind {
    /// Parse a `kind=` identifier value into a [`RoofKind`].
    ///
    /// Returns `None` for any other identifier; the caller turns that into
    /// a `W_DEFERRED_MEMBER` warning that distinguishes "unknown kind"
    /// from "missing `kind=`" at the diagnostic level.
    #[must_use]
    pub fn from_ident(name: &str) -> Option<Self> {
        match name {
            "gable" => Some(Self::Gable),
            "shed" => Some(Self::Shed),
            "hip" => Some(Self::Hip),
            "flat" => Some(Self::Flat),
            _ => None,
        }
    }

    /// Lowercase kind name, matching the source `kind=` identifier.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Gable => "gable",
            Self::Shed => "shed",
            Self::Hip => "hip",
            Self::Flat => "flat",
        }
    }

    /// Canonical block id this kind voxelises into. Used by `lower.rs`
    /// to check a `mat_slot=` binding against the kind's hardcoded
    /// material.
    #[must_use]
    pub fn base_block_id(self) -> &'static str {
        match self {
            Self::Gable | Self::Shed | Self::Hip => STAIR_BASE_ID,
            Self::Flat => FLAT_BASE_ID,
        }
    }
}

/// Cardinal direction stair facings can take. Aliased to north/south/east
/// /west string values when an actual blockstate is emitted.
///
/// Used by [`shed_voxels`] and [`hip_voxels`] so the geometry layer can
/// name the four sides without re-deriving the `WallSide → Cardinal`
/// mapping at each call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cardinal {
    /// `-z` direction.
    North,
    /// `+z` direction.
    South,
    /// `-x` direction.
    West,
    /// `+x` direction.
    East,
}

impl Cardinal {
    /// String form used inside a `BlockState.properties["facing"]`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }
}

/// Stair `shape` property values used by hip-roof corner cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StairShape {
    /// Default straight stair — used for every slope row that is not a
    /// hip corner.
    Straight,
    /// Outer corner whose missing step is on the left of the facing
    /// direction.
    OuterLeft,
    /// Outer corner whose missing step is on the right of the facing
    /// direction.
    OuterRight,
}

impl StairShape {
    /// String form used inside a `BlockState.properties["shape"]`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Straight => "straight",
            Self::OuterLeft => "outer_left",
            Self::OuterRight => "outer_right",
        }
    }
}

// ---------------------------------------------------------------- gable

/// Which face of the gable a voxel belongs to. The lowering pass interns
/// one [`BlockState`] per variant via [`gable_stair_state`] and reuses the
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
pub fn gable_stair_state(ridge_axis: Axis, face: StairFace) -> BlockState {
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
        (Axis::X, StairFace::LowSlope | StairFace::ApexLow) => Cardinal::South,
        (Axis::X, StairFace::HighSlope | StairFace::ApexHigh) => Cardinal::North,
        // z-ridge: short axis is x. Mirror the same rule onto the x axis.
        (Axis::Z, StairFace::LowSlope | StairFace::ApexLow) => Cardinal::East,
        (Axis::Z, StairFace::HighSlope | StairFace::ApexHigh) => Cardinal::West,
    };
    let half = match face {
        StairFace::ApexLow | StairFace::ApexHigh => "top",
        StairFace::LowSlope | StairFace::HighSlope => "bottom",
    };
    stair_state(STAIR_BASE_ID, facing, half, StairShape::Straight)
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
            emit_gable_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                low_index,
                StairFace::ApexLow,
            );
        } else if is_apex {
            // Even-span apex: the slopes meet at two adjacent rows. Emit
            // both as half=top stairs facing outward so the cap closes.
            emit_gable_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                low_index,
                StairFace::ApexLow,
            );
            emit_gable_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                high_index,
                StairFace::ApexHigh,
            );
        } else {
            emit_gable_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                low_index,
                StairFace::LowSlope,
            );
            emit_gable_row(
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

fn emit_gable_row(
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

// ----------------------------------------------------------------- shed

/// Which face of a shed roof a voxel belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShedFace {
    /// `half=bottom` slope row. Every layer from `0..extra_height-1` uses
    /// this face.
    Slope,
    /// `half=top` apex cap on the highest layer.
    Apex,
}

/// One stair voxel produced by [`shed_voxels`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShedVoxel {
    /// Grid position `(x, y, z)` to write the stair into.
    pub pos: (u32, u32, u32),
    /// Which face this voxel belongs to.
    pub face: ShedFace,
}

/// Vertical rise of a shed roof above its wall top.
///
/// A shed roof climbs one layer per voxel of slope along the chosen axis,
/// so `extra_height = short_span` (where `short_span` is the roof
/// bounding-box extent along the slope axis). The apex layer is the top
/// row, capped with `half=top`.
#[must_use]
pub fn shed_extra_height(slope_span: u32) -> u32 {
    slope_span.max(1)
}

/// Cardinal direction the *high* edge of a shed roof sits on.
///
/// The slope descends away from this side; stairs face this direction so
/// the riser steps up toward the high edge.
#[must_use]
pub fn shed_high_side(slope_to: WallSide) -> Cardinal {
    match slope_to {
        WallSide::Front => Cardinal::South,
        WallSide::Back => Cardinal::North,
        WallSide::Left => Cardinal::West,
        WallSide::Right => Cardinal::East,
    }
}

/// Span of a shed roof along its slope axis.
///
/// The slope rises by 1 voxel per step along this span. Front/Back point
/// the high edge in the `±z` direction, so the slope axis is `z` and the
/// span is `roof_h`; Left/Right point in the `±x` direction, so the
/// slope axis is `x` and the span is `roof_w`. Centralising the
/// `slope_to → axis` choice here keeps the dim-computation pass in
/// `block_array::lower` and the voxel generator below from drifting on
/// the rule.
#[must_use]
pub fn shed_slope_span(roof_w: u32, roof_h: u32, slope_to: WallSide) -> u32 {
    match slope_to {
        WallSide::Front | WallSide::Back => roof_h,
        WallSide::Left | WallSide::Right => roof_w,
    }
}

/// Build the [`BlockState`] for one face of a shed roof.
#[must_use]
pub fn shed_stair_state(slope_to: WallSide, face: ShedFace) -> BlockState {
    let facing = shed_high_side(slope_to);
    let half = match face {
        ShedFace::Slope => "bottom",
        ShedFace::Apex => "top",
    };
    stair_state(STAIR_BASE_ID, facing, half, StairShape::Straight)
}

/// Generate every stair voxel position for a shed roof.
///
/// - `roof_w` / `roof_h` are the roof bounding box extents along x and z.
/// - `wall_top` is the y of the highest wall voxel.
/// - `slope_to` is the wall the high edge sits on. The slope descends
///   from this wall toward the opposite wall.
///
/// Voxels are emitted in `(layer, perpendicular)` order; the order is
/// stable so the palette layout and the lockfile hash do not drift.
#[must_use]
pub fn shed_voxels(roof_w: u32, roof_h: u32, wall_top: u32, slope_to: WallSide) -> Vec<ShedVoxel> {
    let mut out: Vec<ShedVoxel> = Vec::new();
    // The slope axis is the one the high edge sits on. `shed_slope_span`
    // is the single source of truth for the axis choice — the dim pass
    // in `block_array::lower` calls the same helper, so the dim and the
    // generator cannot drift on which axis the slope runs along.
    let slope_span = shed_slope_span(roof_w, roof_h, slope_to);
    let (perp_span, axis_is_z) = match slope_to {
        WallSide::Front | WallSide::Back => (roof_w, true),
        WallSide::Left | WallSide::Right => (roof_h, false),
    };
    let layers = shed_extra_height(slope_span);
    let high_on_max = matches!(slope_to, WallSide::Front | WallSide::Right);
    for layer in 0..layers {
        let y = wall_top.saturating_add(1).saturating_add(layer);
        let is_apex = layer + 1 == layers;
        // Layer L sits at distance L from the LOW edge along the slope
        // axis: the lowest row hugs the low wall, every step inward raises
        // y by 1 and moves the row one cell toward the high wall. When
        // the high edge sits at the +axis (Front=+z, Right=+x), the LOW
        // side is at coord 0 so `slope_index = layer`. When the high edge
        // sits at the -axis (Back=-z, Left=-x), the LOW side is at
        // `slope_span - 1` so we walk inward from there.
        let slope_index = if high_on_max {
            layer
        } else {
            slope_span.saturating_sub(1).saturating_sub(layer)
        };
        let face = if is_apex {
            ShedFace::Apex
        } else {
            ShedFace::Slope
        };
        for i in 0..perp_span {
            let pos = if axis_is_z {
                (i, y, slope_index)
            } else {
                (slope_index, y, i)
            };
            out.push(ShedVoxel { pos, face });
        }
    }
    out
}

// ------------------------------------------------------------------ hip

/// Where on a hip roof a voxel sits, in terms of which side or corner of
/// the inset rectangle frame produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HipFace {
    /// North-side slope row (`z = layer`), `facing=south`, `half=bottom`.
    SlopeNorth,
    /// South-side slope row (`z = roof_h - 1 - layer`), `facing=north`.
    SlopeSouth,
    /// West-side slope column (`x = layer`), `facing=east`.
    SlopeWest,
    /// East-side slope column (`x = roof_w - 1 - layer`), `facing=west`.
    SlopeEast,
    /// North-west corner — `facing=south`, `shape=outer_left`.
    CornerNorthWest,
    /// North-east corner — `facing=south`, `shape=outer_right`.
    CornerNorthEast,
    /// South-west corner — `facing=north`, `shape=outer_right`.
    CornerSouthWest,
    /// South-east corner — `facing=north`, `shape=outer_left`.
    CornerSouthEast,
    /// Apex ridge cell on the long axis (rectangular hip only). The ridge
    /// runs along `x` for an x-axis ridge, along `z` for a z-axis ridge.
    /// Facing matches the gable rule (`south` for x-ridge, `east` for
    /// z-ridge); `half=top`.
    ApexRidge,
    /// Apex cap cell on a square hip (one or two voxels at the very top).
    /// Facing matches the ridge axis; `half=top`.
    ApexCap,
}

/// One stair voxel produced by [`hip_voxels`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HipVoxel {
    /// Grid position `(x, y, z)` to write the stair into.
    pub pos: (u32, u32, u32),
    /// Which face this voxel belongs to.
    pub face: HipFace,
}

/// Vertical rise of a hip roof above its wall top.
///
/// Identical math to [`gable_extra_height`] — the short-axis slope
/// reaches the ridge at `ceil(short_span / 2)`. The long-axis slope is
/// shorter or equal and so finishes at or before the same layer.
#[must_use]
pub fn hip_extra_height(roof_w: u32, roof_h: u32) -> u32 {
    let short = roof_w.min(roof_h);
    short.div_ceil(2).max(1)
}

/// Build the [`BlockState`] for one face of a hip roof.
#[must_use]
pub fn hip_stair_state(ridge_axis: Axis, face: HipFace) -> BlockState {
    let (facing, shape) = match face {
        HipFace::SlopeNorth => (Cardinal::South, StairShape::Straight),
        HipFace::SlopeSouth => (Cardinal::North, StairShape::Straight),
        HipFace::SlopeWest => (Cardinal::East, StairShape::Straight),
        HipFace::SlopeEast => (Cardinal::West, StairShape::Straight),
        HipFace::CornerNorthWest => (Cardinal::South, StairShape::OuterLeft),
        HipFace::CornerNorthEast => (Cardinal::South, StairShape::OuterRight),
        HipFace::CornerSouthWest => (Cardinal::North, StairShape::OuterRight),
        HipFace::CornerSouthEast => (Cardinal::North, StairShape::OuterLeft),
        HipFace::ApexRidge | HipFace::ApexCap => {
            let facing = match ridge_axis {
                Axis::X => Cardinal::South,
                Axis::Z => Cardinal::East,
            };
            (facing, StairShape::Straight)
        }
    };
    let half = match face {
        HipFace::ApexRidge | HipFace::ApexCap => "top",
        _ => "bottom",
    };
    stair_state(STAIR_BASE_ID, facing, half, shape)
}

/// Generate every stair voxel position for a hip roof.
///
/// Each layer `L` is the inset rectangle frame at `z = L` (north row),
/// `z = roof_h - 1 - L` (south row), `x = L` (west column), and
/// `x = roof_w - 1 - L` (east column). The four corners are written
/// separately as `shape=outer_*` cells so the diagonal closes neatly
/// where two slopes meet.
///
/// The apex layer (`L == extra_height - 1`) caps the roof.
///
/// - **Square footprint (`roof_w == roof_h`)**: one cell (odd span) or a
///   2x2 block (even span) tagged [`HipFace::ApexCap`].
/// - **Rectangular footprint**: the short slopes meet at the centre of
///   the short axis, leaving a ridge row along the long axis. The ridge
///   row is tagged [`HipFace::ApexRidge`]; both ends of the ridge are
///   left to the apex-cap geometry (handled by the same row, since the
///   short-side terminations sit at the ridge endpoints).
#[must_use]
pub fn hip_voxels(roof_w: u32, roof_h: u32, wall_top: u32) -> Vec<HipVoxel> {
    let layers = hip_extra_height(roof_w, roof_h);
    let mut out: Vec<HipVoxel> = Vec::new();
    for layer in 0..layers {
        let y = wall_top.saturating_add(1).saturating_add(layer);
        let is_apex = layer + 1 == layers;

        // After insetting by `layer` on every side, the remaining
        // interior runs from `[layer, roof_w-1-layer]` on x and the same
        // shape on z. When either span has collapsed to <= 0 (or to a
        // single line that the next layer would invert), we are at the
        // apex.
        let lo_x = layer;
        let hi_x = roof_w.saturating_sub(1).saturating_sub(layer);
        let lo_z = layer;
        let hi_z = roof_h.saturating_sub(1).saturating_sub(layer);
        if lo_x > hi_x || lo_z > hi_z {
            // Defensive: should not be hit under the layer count
            // computed by `hip_extra_height` for non-zero dims.
            continue;
        }

        if !is_apex {
            emit_hip_frame(&mut out, lo_x, hi_x, lo_z, hi_z, y);
            continue;
        }

        // Apex layer: the inset rectangle has collapsed to the ridge.
        let span_x = hi_x - lo_x + 1;
        let span_z = hi_z - lo_z + 1;
        if span_x == 1 && span_z == 1 {
            // Square footprint with odd short span → single apex cell.
            out.push(HipVoxel {
                pos: (lo_x, y, lo_z),
                face: HipFace::ApexCap,
            });
        } else if roof_w == roof_h {
            // Square footprint with even short span → 2x2 cap.
            for x in lo_x..=hi_x {
                for z in lo_z..=hi_z {
                    out.push(HipVoxel {
                        pos: (x, y, z),
                        face: HipFace::ApexCap,
                    });
                }
            }
        } else if span_x >= span_z {
            // Long axis = x → ridge runs along x at `z = lo_z..=hi_z`
            // (always a single row when `roof_w > roof_h` and short span
            // is odd; a 2-row band when short span is even).
            for x in lo_x..=hi_x {
                for z in lo_z..=hi_z {
                    out.push(HipVoxel {
                        pos: (x, y, z),
                        face: HipFace::ApexRidge,
                    });
                }
            }
        } else {
            // Long axis = z → ridge runs along z.
            for z in lo_z..=hi_z {
                for x in lo_x..=hi_x {
                    out.push(HipVoxel {
                        pos: (x, y, z),
                        face: HipFace::ApexRidge,
                    });
                }
            }
        }
    }
    out
}

fn emit_hip_frame(out: &mut Vec<HipVoxel>, lo_x: u32, hi_x: u32, lo_z: u32, hi_z: u32, y: u32) {
    // Corners go first so the iteration order is stable and palette
    // intern order is deterministic. When the frame has collapsed to a
    // single line on one axis the corner cells still emit (the four
    // corners coincide with the only two slope cells; the last write at
    // a given grid position wins, so corner shape ends up applied).
    out.push(HipVoxel {
        pos: (lo_x, y, lo_z),
        face: HipFace::CornerNorthWest,
    });
    if hi_x > lo_x {
        out.push(HipVoxel {
            pos: (hi_x, y, lo_z),
            face: HipFace::CornerNorthEast,
        });
    }
    if hi_z > lo_z {
        out.push(HipVoxel {
            pos: (lo_x, y, hi_z),
            face: HipFace::CornerSouthWest,
        });
    }
    if hi_x > lo_x && hi_z > lo_z {
        out.push(HipVoxel {
            pos: (hi_x, y, hi_z),
            face: HipFace::CornerSouthEast,
        });
    }
    // North and south rows, excluding the corners.
    if hi_x >= lo_x + 2 {
        for x in (lo_x + 1)..hi_x {
            out.push(HipVoxel {
                pos: (x, y, lo_z),
                face: HipFace::SlopeNorth,
            });
            if hi_z > lo_z {
                out.push(HipVoxel {
                    pos: (x, y, hi_z),
                    face: HipFace::SlopeSouth,
                });
            }
        }
    }
    // East and west columns, excluding the corners.
    if hi_z >= lo_z + 2 {
        for z in (lo_z + 1)..hi_z {
            out.push(HipVoxel {
                pos: (lo_x, y, z),
                face: HipFace::SlopeWest,
            });
            if hi_x > lo_x {
                out.push(HipVoxel {
                    pos: (hi_x, y, z),
                    face: HipFace::SlopeEast,
                });
            }
        }
    }
}

// ----------------------------------------------------------------- flat

/// Vertical rise of a flat roof above its wall top. Always `1` — a flat
/// roof is a single solid layer of [`FLAT_BASE_ID`] at `y = wall_top + 1`.
#[must_use]
pub const fn flat_extra_height() -> u32 {
    1
}

/// Build the [`BlockState`] for a flat roof's deck.
#[must_use]
pub fn flat_block_state() -> BlockState {
    BlockState {
        id: FLAT_BASE_ID.to_owned(),
        properties: IndexMap::new(),
    }
}

/// Generate every voxel position for a flat roof's deck layer.
///
/// A flat roof fills the entire inflated bounding box at `y = wall_top + 1`
/// with [`FLAT_BASE_ID`]; the `overhang=` parameter therefore extends the
/// deck past the walls without any special handling. Returns positions in
/// `(z, x)` order so the palette layout and lockfile hash stay stable.
#[must_use]
pub fn flat_voxels(roof_w: u32, roof_h: u32, wall_top: u32) -> Vec<(u32, u32, u32)> {
    let y = wall_top.saturating_add(1);
    let mut out: Vec<(u32, u32, u32)> = Vec::with_capacity((roof_w as usize) * (roof_h as usize));
    for z in 0..roof_h {
        for x in 0..roof_w {
            out.push((x, y, z));
        }
    }
    out
}

// --------------------------------------------------------------- shared

fn stair_state(id: &str, facing: Cardinal, half: &str, shape: StairShape) -> BlockState {
    let mut properties: IndexMap<String, String> = IndexMap::new();
    properties.insert("facing".to_owned(), facing.as_str().to_owned());
    properties.insert("half".to_owned(), half.to_owned());
    properties.insert("shape".to_owned(), shape.as_str().to_owned());
    BlockState {
        id: id.to_owned(),
        properties,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------- gable --------

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
            assert_eq!(v.face, StairFace::ApexLow);
            assert_eq!(v.pos.2, 4);
        }
        let state = gable_stair_state(Axis::X, StairFace::ApexLow);
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

    // -------- roof kind parsing --------

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

    // -------- shed --------

    #[test]
    fn shed_extra_height_equals_slope_span() {
        assert_eq!(shed_extra_height(7), 7);
        assert_eq!(shed_extra_height(1), 1);
        assert_eq!(shed_extra_height(0), 1);
    }

    #[test]
    fn shed_voxels_climb_one_layer_per_step_toward_high_side() {
        // Invariant: layer L sits at distance L from the low edge along
        // the slope axis. For `slope_to=front` the low edge is the back
        // wall (z=0) and the high edge is the front wall (z=slope_span-1).
        // So layer 0 sits at z=0 with the lowest y, and the apex sits at
        // z=slope_span-1 with the highest y. `roof_w=5, roof_h=4,
        // slope_to=front` → 4 layers × 5 perpendicular cells.
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
        // Stairs face the high side: slope_to=front → facing=south.
        let slope_state = shed_stair_state(WallSide::Front, ShedFace::Slope);
        assert_eq!(slope_state.properties.get("facing").unwrap(), "south");
        assert_eq!(slope_state.properties.get("half").unwrap(), "bottom");
        let apex_state = shed_stair_state(WallSide::Front, ShedFace::Apex);
        assert_eq!(apex_state.properties.get("half").unwrap(), "top");
    }

    #[test]
    fn shed_voxels_left_high_runs_along_x() {
        // slope_to=Left (-x high). Slope axis is x, perp is z.
        // layers = roof_w = 4.
        let voxels = shed_voxels(4, 5, 3, WallSide::Left);
        // Layer 0 at lowest y=4, on the LOW side x=roof_w-1=3 (Right).
        let l0: Vec<&ShedVoxel> = voxels.iter().filter(|v| v.pos.1 == 4).collect();
        assert_eq!(l0.len(), 5);
        for v in &l0 {
            assert_eq!(v.pos.0, 3, "low side at x=3 for slope_to=left");
        }
        // Stair facing for Left high side = west (-x).
        let state = shed_stair_state(WallSide::Left, ShedFace::Slope);
        assert_eq!(state.properties.get("facing").unwrap(), "west");
    }

    // -------- hip --------

    #[test]
    fn hip_extra_height_matches_gable_short_span_formula() {
        assert_eq!(hip_extra_height(11, 9), 5); // square ceil(9/2)
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
        // roof_w=8, roof_h=4 → short=4 (even), extra_height=2. Apex layer
        // L=1 leaves the inset rectangle `lo_x=1..=hi_x=6` (6 cells along
        // x) × `lo_z=1..=hi_z=2` (2 cells along z). The two-row ridge
        // band must all carry `HipFace::ApexRidge` and `hip_stair_state`
        // must give every cell `facing=south, half=top`, matching the
        // spec §4.5 single-facing rule (no opposite-facing close-up like
        // gable's even-span apex). This pins the "open V on even short
        // span" behaviour as deliberate.
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
        // roof_w=9, roof_h=5 → ridge along x. extra_height = ceil(5/2)=3.
        // Apex layer L=2: ridge rectangle at lo_x=2..=hi_x=6 (5 cells),
        // lo_z=2..=hi_z=2 (1 cell). Ridge row of 5 cells along x.
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
        // roof_w=9, roof_h=5. Layer 0 frame: north row z=0 (x=0..=8),
        // south row z=4 (x=0..=8), west column x=0 (z=0..=4), east x=8.
        // Total perimeter cells = 2*9 + 2*5 - 4 corners = 24.
        let voxels = hip_voxels(9, 5, 4);
        let l0: Vec<&HipVoxel> = voxels.iter().filter(|v| v.pos.1 == 5).collect();
        assert_eq!(l0.len(), 2 * 9 + 2 * 5 - 4);
        // Each corner present and unique.
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

    // -------- flat --------

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
    fn flat_block_state_is_spruce_planks() {
        let bs = flat_block_state();
        assert_eq!(bs.id, FLAT_BASE_ID);
        assert!(bs.properties.is_empty());
    }
}
