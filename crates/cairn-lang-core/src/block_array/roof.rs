//! Roof voxel generators.
//!
//! Each of the four roof kinds (`gable`, `shed`, `hip`, `flat`) is
//! voxelised by a pure function returning `(grid_pos, face_tag)` entries;
//! lowering interns one [`BlockState`] per face tag and writes the grid.
//! Pure so the unit tests pin the exact stair layout without a whole
//! `BlockArray`, and so the caller clones no state per voxel.
//!
//! The kinds share the conventions below; each one's ridge and corner
//! rules are `spec/compilation` "Gable roof voxel rules", "Shed roof voxel
//! rules", "Hip roof voxel rules" and "Flat roof voxel rules".
//!
//! ## Common geometry conventions
//!
//! - **Bounding box.** The roof sits in a `roof_w x roof_h` box where
//!   `roof_w = size.w + 2*overhang` and `roof_h = size.h + 2*overhang`.
//!   Floors, walls, doors, and windows stay on their authored
//!   coordinates and shift inward by `overhang` on x/z; the roof spans
//!   the full inflated box so eaves extend past the wall ring.
//! - **Wall top.** The lowest roof voxel sits at `y = wall_top + 1`,
//!   where `wall_top` is the highest row any `walls` member paints, or `0`
//!   without walls.
//! - **Stair facing.** Sloped roof voxels face *toward the ridge* with
//!   `half=bottom`. Apex caps reuse the slope facings with `half=top` so
//!   the upside-down stair closes the peak.
//! - **Material id.** The `mat_slot=` binding chooses the species.
//!   `gable`, `shed` and `hip` attach `facing`, `half` and `shape`, which
//!   only the stair family has ([`is_stair`]), so a binding outside it is
//!   refused (`E_INCOMPATIBLE_MATERIAL`, in lowering's `roof_fill`) and
//!   the roof falls back to [`STAIR_BASE_ID`]. `flat` attaches no states,
//!   so it takes any id and falls back to [`FLAT_BASE_ID`] only when there
//!   is no binding.

use indexmap::IndexMap;

use super::BlockState;
use super::openings::WallSide;

/// Stair id a sloped roof falls back to when its `mat_slot=` binds nothing
/// usable: no binding, or one [`is_stair`] refuses. Any stair species
/// binds, such as the registry pack's `roof.dark_wood` → `dark_oak_stairs`.
pub const STAIR_BASE_ID: &str = "minecraft:spruce_stairs";

/// Planks id a `flat` roof falls back to when it has no `mat_slot=`. A
/// flat roof attaches no blockstate, so any id serves as well.
pub const FLAT_BASE_ID: &str = "minecraft:spruce_planks";

/// Whether `id` names a member of the stair family, which shares the
/// `facing`, `half` and `shape` vocabulary the sloped roofs attach.
///
/// The `_stairs` suffix rejects `mystairs`; reading it off the *path*
/// rejects `weird_stairs:cobblestone`, where the word sits in the
/// namespace.
///
/// `cairn-lang-formats` asks the same question when it translates a
/// blockstate for Bedrock, so it calls this rather than a copy that could
/// disagree about an id this crate paints.
#[must_use]
pub fn is_stair(id: &str) -> bool {
    let path = id.rsplit_once(':').map_or(id, |(_, path)| path);
    path.ends_with("_stairs")
}

/// One of the four supported roof kinds.
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
    /// Every kind, the whole set a `kind=` can name.
    ///
    /// `MemberRole::conditional_arguments` says which of these read
    /// `slope_to=`; a kind added without a row there would drop the
    /// direction silently, which the test
    /// `every_roof_kind_the_dispatch_knows_has_an_arm` checks against this.
    pub const ALL: [Self; 4] = [Self::Gable, Self::Shed, Self::Hip, Self::Flat];

    /// The [`RoofKind`] a `kind=` identifier names, or `None`.
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

    /// Every kind name, as a message lists them: `gable, shed, hip, flat`.
    #[must_use]
    pub fn names() -> String {
        Self::ALL
            .iter()
            .map(|kind| kind.name())
            .collect::<Vec<_>>()
            .join(", ")
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

    /// Block id this kind paints when no `mat_slot=` binding supplies one.
    #[must_use]
    pub fn base_block_id(self) -> &'static str {
        match self {
            Self::Gable | Self::Shed | Self::Hip => STAIR_BASE_ID,
            Self::Flat => FLAT_BASE_ID,
        }
    }

    /// Whether this kind attaches stair blockstates to the block it paints,
    /// and therefore needs one from the stair family ([`is_stair`]).
    #[must_use]
    pub fn paints_stairs(self) -> bool {
        match self {
            Self::Gable | Self::Shed | Self::Hip => true,
            Self::Flat => false,
        }
    }
}

/// Cardinal direction a stair can face.
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
    /// Straight stair, for every cell that is not a hip corner.
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

/// Which face of the gable a voxel belongs to; [`gable_stair_state`] gives
/// each its [`BlockState`].
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
    /// The single cap on an apex row where the two slopes converge (an
    /// odd short span), with the low slope's facing (`spec/compilation`
    /// "Gable roof voxel rules"). `half=top`.
    Apex,
    /// Low-side cap of an even-span apex pair, facing *away* from the
    /// ridge. `half=top`.
    ApexLow,
    /// High-side cap of an even-span apex pair, facing away from the
    /// ridge on the other side. `half=top`.
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

/// One stair voxel of a roof surface, tagged with the face it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoofVoxel<F> {
    /// Grid position `(x, y, z)` to write the stair into.
    pub pos: (u32, u32, u32),
    /// Which face this voxel belongs to.
    pub face: F,
}

/// One stair voxel produced by [`gable_voxels`].
pub type GableVoxel = RoofVoxel<StairFace>;

/// Vertical rise of a gable roof above its wall top.
///
/// `short_span` is the span of the slope (the *short* horizontal axis of
/// the roof bounding box, measured in voxels). The ridge sits this many
/// layers above the wall top, including the apex layer.
#[must_use]
pub fn gable_extra_height(short_span: u32) -> u32 {
    // Each layer steps inward by 1 on each slope.
    short_span.div_ceil(2).max(1)
}

/// Choose the ridge axis for a given roof bounding box.
///
/// The ridge runs along the longer axis so the slopes stay as short as
/// possible. Ties break to `x` (`spec/compilation` "Gable roof voxel rules").
#[must_use]
pub fn gable_ridge_axis(roof_w: u32, roof_h: u32) -> Axis {
    if roof_w >= roof_h { Axis::X } else { Axis::Z }
}

/// Build the [`BlockState`] for one stair variant of a gable roof. The
/// same `(ridge_axis, face)` always gives the same state, so a caller
/// interns one per face.
#[must_use]
pub fn gable_stair_state(ridge_axis: Axis, face: StairFace) -> BlockState {
    // A `half=top` stair fills the upper half of its voxel plus the lower
    // quarter on its facing side. An even-span apex is a pair straddling
    // the ridge, so each faces away from it to put that quarter on the
    // outer face; facing inward would leave a 0.5 x 0.5 undercut along
    // both outer faces.
    //
    // A converged apex is one cell wide, so both its faces are outer and
    // one keeps the void; `spec/compilation` "Gable roof voxel rules"
    // gives it the low slope's facing.
    let facing = match (ridge_axis, face) {
        // x-ridge: the low slope is on -z and faces +z (south), toward
        // the ridge. `ApexHigh` sits on +z and faces outward, also south.
        (Axis::X, StairFace::LowSlope | StairFace::Apex | StairFace::ApexHigh) => Cardinal::South,
        (Axis::X, StairFace::HighSlope | StairFace::ApexLow) => Cardinal::North,
        (Axis::Z, StairFace::LowSlope | StairFace::Apex | StairFace::ApexHigh) => Cardinal::East,
        (Axis::Z, StairFace::HighSlope | StairFace::ApexLow) => Cardinal::West,
    };
    let half = match face {
        StairFace::Apex | StairFace::ApexLow | StairFace::ApexHigh => "top",
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
        let low_index = layer;
        let high_index = span.saturating_sub(1).saturating_sub(layer);
        let converged = low_index == high_index;
        // Layer 0 sits on the wall top, so it is a slope even when it is
        // also the last layer (a short span of 1 or 2): capped, the whole
        // course would be `half=top`, leaving a half-block slit between
        // wall and roof. A cap only closes a peak something below raised.
        let is_apex = layer > 0 && layer + 1 == layers;

        if converged {
            // The slopes meet on one row, emitted once: the single cap at
            // the apex, or on layer 0 a span-1 row of slope.
            let face = if is_apex {
                StairFace::Apex
            } else {
                StairFace::LowSlope
            };
            emit_gable_row(&mut out, ridge_axis, long_axis_len, y, low_index, face);
        } else {
            let (low_face, high_face) = if is_apex {
                (StairFace::ApexLow, StairFace::ApexHigh)
            } else {
                (StairFace::LowSlope, StairFace::HighSlope)
            };
            emit_gable_row(&mut out, ridge_axis, long_axis_len, y, low_index, low_face);
            emit_gable_row(
                &mut out,
                ridge_axis,
                long_axis_len,
                y,
                high_index,
                high_face,
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
    /// `half=bottom` slope row, on every layer below the top.
    Slope,
    /// `half=top` apex cap on the highest layer.
    Apex,
}

/// One stair voxel produced by [`shed_voxels`].
pub type ShedVoxel = RoofVoxel<ShedFace>;

/// Vertical rise of a shed roof above its wall top: one layer per voxel of
/// `slope_span`, the bounding box's extent along the slope.
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

/// Span of a shed roof along its slope axis: `z` for a front or back high
/// edge, `x` for left or right.
///
/// Lowering's volume sizing and [`shed_voxels`] both take the axis from
/// here, so they cannot disagree on it.
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
#[must_use]
pub fn shed_voxels(roof_w: u32, roof_h: u32, wall_top: u32, slope_to: WallSide) -> Vec<ShedVoxel> {
    let mut out: Vec<ShedVoxel> = Vec::new();
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
        // Layer L sits L cells from the low edge, which is at coordinate
        // 0 when the high edge is on the + side (front, right) and at
        // `slope_span - 1` otherwise.
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
pub type HipVoxel = RoofVoxel<HipFace>;

/// Vertical rise of a hip roof above its wall top.
///
/// The short-axis slope reaches the ridge at `ceil(short_span / 2)`, as a
/// gable's does, and the long-axis slope finishes no later.
#[must_use]
pub fn hip_extra_height(roof_w: u32, roof_h: u32) -> u32 {
    gable_extra_height(roof_w.min(roof_h))
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
///   the short axis, leaving a ridge row along the long axis, tagged
///   [`HipFace::ApexRidge`] end to end.
#[must_use]
pub fn hip_voxels(roof_w: u32, roof_h: u32, wall_top: u32) -> Vec<HipVoxel> {
    let layers = hip_extra_height(roof_w, roof_h);
    let mut out: Vec<HipVoxel> = Vec::new();
    for layer in 0..layers {
        let y = wall_top.saturating_add(1).saturating_add(layer);
        // Layer 0 is always the frame, as in `gable_voxels`: capped, a
        // single-course roof would lose its `outer_*` corners and leave a
        // half-block slit around the whole perimeter.
        let is_apex = layer > 0 && layer + 1 == layers;

        let lo_x = layer;
        let hi_x = roof_w.saturating_sub(1).saturating_sub(layer);
        let lo_z = layer;
        let hi_z = roof_h.saturating_sub(1).saturating_sub(layer);
        if lo_x > hi_x || lo_z > hi_z {
            // Unreachable for non-zero dims under `hip_extra_height`.
            continue;
        }

        if !is_apex {
            emit_hip_frame(&mut out, lo_x, hi_x, lo_z, hi_z, y);
            continue;
        }

        let span_x = hi_x - lo_x + 1;
        let span_z = hi_z - lo_z + 1;
        if span_x == 1 && span_z == 1 {
            out.push(HipVoxel {
                pos: (lo_x, y, lo_z),
                face: HipFace::ApexCap,
            });
        } else if roof_w == roof_h {
            for x in lo_x..=hi_x {
                for z in lo_z..=hi_z {
                    out.push(HipVoxel {
                        pos: (x, y, z),
                        face: HipFace::ApexCap,
                    });
                }
            }
        } else if span_x >= span_z {
            // One row for an odd short span, two for an even one.
            for x in lo_x..=hi_x {
                for z in lo_z..=hi_z {
                    out.push(HipVoxel {
                        pos: (x, y, z),
                        face: HipFace::ApexRidge,
                    });
                }
            }
        } else {
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
    // No position is emitted twice however far the frame has collapsed:
    // each corner is guarded against the axis it would collapse onto, and
    // the rows and columns start one cell inside the corners. Lowering
    // relies on that, since a member writing one cell to two blocks is
    // reported as a conflict between source lines.
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

/// Vertical rise of a flat roof above its wall top: one solid layer.
#[must_use]
pub const fn flat_extra_height() -> u32 {
    1
}

/// Build the [`BlockState`] for a flat roof's deck.
#[must_use]
pub fn flat_block_state() -> BlockState {
    BlockState::bare(FLAT_BASE_ID)
}

/// Generate every voxel position for a flat roof's deck layer: the whole
/// inflated bounding box at `y = wall_top + 1`, so `overhang=` extends the
/// deck past the walls.
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

/// Build a stair [`BlockState`], its properties in name order as a parsed
/// state literal's are. Lowering's `stair` member builds through this too,
/// so the two write equal states alike.
pub(super) fn stair_state(id: &str, facing: Cardinal, half: &str, shape: StairShape) -> BlockState {
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
mod tests;
