//! Roof voxel generators: each kind's rules are `spec/compilation` "Gable roof voxel rules",
//! "Shed roof voxel rules", "Hip roof voxel rules" and "Flat roof voxel rules".
//! A roof spans the overhang-inflated box from `wall_top + 1`, its stairs facing the ridge.

use indexmap::IndexMap;

use super::BlockState;
use super::openings::WallSide;

/// Stair id a sloped roof falls back to when its `mat_slot=` binds nothing usable.
pub const STAIR_BASE_ID: &str = "minecraft:spruce_stairs";

/// Planks id a `flat` roof falls back to when it has no `mat_slot=`.
pub const FLAT_BASE_ID: &str = "minecraft:spruce_planks";

/// Whether `id` names a stair: its path, not its namespace, ends in `_stairs`.
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
    /// Four-sided stair pyramid, with a ridge on a rectangular footprint.
    Hip,
    /// Single layer of solid planks above the wall top.
    Flat,
}

impl RoofKind {
    /// Every kind, the whole set a `kind=` can name.
    ///
    /// A kind added here needs its `slope_to=` row in `MemberRole::conditional_arguments`.
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

    /// Whether this kind attaches stair blockstates, and so needs a stair ([`is_stair`]).
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
    /// Outer corner missing its step on the left of the facing.
    OuterLeft,
    /// Outer corner missing its step on the right of the facing.
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

/// Which face of the gable a voxel belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StairFace {
    /// `half=bottom` slope on the short axis's low side, facing the ridge.
    LowSlope,
    /// `half=bottom` slope on the short axis's high side, facing the ridge.
    HighSlope,
    /// `half=top` single cap where an odd span converges, with the low slope's facing.
    Apex,
    /// `half=top` low-side cap of an even-span apex pair, facing away from the ridge.
    ApexLow,
    /// `half=top` high-side cap of an even-span apex pair, facing away from the ridge.
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

/// Vertical rise of a gable roof above its wall top, apex layer included.
#[must_use]
pub fn gable_extra_height(short_span: u32) -> u32 {
    short_span.div_ceil(2).max(1)
}

/// The ridge axis: the longer one, ties to `x` (`spec/compilation` "Gable roof voxel rules").
#[must_use]
pub fn gable_ridge_axis(roof_w: u32, roof_h: u32) -> Axis {
    if roof_w >= roof_h { Axis::X } else { Axis::Z }
}

/// The [`BlockState`] for one face of a gable roof.
#[must_use]
pub fn gable_stair_state(ridge_axis: Axis, face: StairFace) -> BlockState {
    // A `half=top` stair also fills the lower quarter on its facing side, so an even-span apex
    // pair faces outward to close both outer faces.
    let facing = match (ridge_axis, face) {
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

/// Every stair voxel of a gable roof over a `roof_w` × `roof_h` box, from `wall_top + 1`.
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
        // Layer 0 is always slope: capped, it would leave a half-block slit above the wall.
        let is_apex = layer > 0 && layer + 1 == layers;

        if converged {
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

/// Vertical rise of a shed roof above its wall top: one layer per voxel of `slope_span`.
#[must_use]
pub fn shed_extra_height(slope_span: u32) -> u32 {
    slope_span.max(1)
}

/// Cardinal direction the high edge of a shed roof sits on, which its stairs face.
#[must_use]
pub fn shed_high_side(slope_to: WallSide) -> Cardinal {
    match slope_to {
        WallSide::Front => Cardinal::South,
        WallSide::Back => Cardinal::North,
        WallSide::Left => Cardinal::West,
        WallSide::Right => Cardinal::East,
    }
}

/// Span of a shed roof along its slope axis: `z` for a front or back high edge, `x` otherwise.
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

/// Every stair voxel of a shed roof whose high edge is on `slope_to`.
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

/// Which side or corner of a hip roof's inset frame a voxel belongs to.
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
    /// `half=top` ridge cell along the long axis of a rectangular hip, faced like a gable's.
    ApexRidge,
    /// `half=top` cap of a square hip, one cell or a 2x2 block.
    ApexCap,
}

/// One stair voxel produced by [`hip_voxels`].
pub type HipVoxel = RoofVoxel<HipFace>;

/// Vertical rise of a hip roof above its wall top, as a gable's over the short span.
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

/// Every stair voxel of a hip roof: an inset frame per layer, capped by [`HipFace::ApexCap`] on
/// a square footprint or [`HipFace::ApexRidge`] on a rectangular one.
#[must_use]
pub fn hip_voxels(roof_w: u32, roof_h: u32, wall_top: u32) -> Vec<HipVoxel> {
    let layers = hip_extra_height(roof_w, roof_h);
    let mut out: Vec<HipVoxel> = Vec::new();
    for layer in 0..layers {
        let y = wall_top.saturating_add(1).saturating_add(layer);
        // Layer 0 is always the frame: capped, a one-course roof would leave a slit round its edge.
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
    // No position is emitted twice, even for a collapsed frame: lowering reports a cell written
    // twice as a conflict.
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

/// Every voxel of a flat roof's deck: the whole inflated box at `y = wall_top + 1`.
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

/// A stair [`BlockState`], its properties in name order as a parsed state literal's are.
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
