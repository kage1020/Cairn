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
//! one's exact ridge / corner rule lives in `spec/compilation` — "Gable roof
//! voxel rules", "Shed roof voxel rules", "Hip roof voxel rules" and "Flat
//! roof voxel rules" — and is mirrored by the per-kind generator below.
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
//! - **Stair facing.** Sloped roof voxels take `facing` pointed *toward
//!   the ridge* (the riser's upper-step side ends up on the inward side)
//!   and `half=bottom`. Apex caps reuse the slope facings with `half=top`
//!   so the upside-down stair closes the peak. Which stair species wears
//!   them is the `mat_slot=` binding's to choose — see the next bullet.
//! - **Material id.** A `mat_slot=` binding chooses the species. `gable`,
//!   `shed`, and `hip` require a member of the stair family ([`is_stair`])
//!   because they attach `facing` / `half` / `shape` to whatever they
//!   paint, and those states do not exist on a whole block; a binding
//!   outside the family fires `W_DEFERRED_MEMBER` in `lower.rs` and the
//!   roof falls back to [`STAIR_BASE_ID`] rather than stapling stair
//!   states onto an id that has none. `flat` attaches no states and so
//!   constrains nothing, falling back to [`FLAT_BASE_ID`] only when there
//!   is no binding to read.

use indexmap::IndexMap;

use super::BlockState;
use super::openings::WallSide;

/// Stair id a sloped roof falls back to when its `mat_slot=` binds nothing
/// usable.
///
/// Not a ceiling on what a roof may be made of: any member of the stair
/// family binds, and the registry pack ships four roof species that do
/// (`roof.dark_wood` → `dark_oak_stairs`, and so on). What the fallback
/// covers is a roof with no `mat_slot=` at all, and one whose binding
/// [`is_stair`] refuses — see `lower.rs`.
pub const STAIR_BASE_ID: &str = "minecraft:spruce_stairs";

/// Planks id a `flat` roof falls back to when it has no `mat_slot=`.
///
/// Unlike [`STAIR_BASE_ID`] this constrains nothing. A flat roof is one
/// layer of whole blocks with no blockstate attached, so every id is as
/// valid as this one.
pub const FLAT_BASE_ID: &str = "minecraft:spruce_planks";

/// Whether `id` names a member of the stair family.
///
/// The whole family shares one blockstate vocabulary — `facing`, `half`,
/// `shape` — which is what makes the family, rather than any one species,
/// the right unit for a geometry pass to require.
///
/// Two rules, doing different work. The `_stairs` suffix is what rejects
/// `mystairs`. Reading it off the *path* rather than the whole identifier is
/// what rejects `weird_stairs:cobblestone`, where the family word sits in
/// the namespace and the block is not a stair at all.
///
/// One owner on purpose. `cairn-lang-formats` asks the same question when
/// it translates a blockstate for Bedrock, and two copies of this rule
/// could disagree about an id that this crate paints and that crate then
/// has to write.
#[must_use]
pub fn is_stair(id: &str) -> bool {
    let path = id.rsplit_once(':').map_or(id, |(_, path)| path);
    path.ends_with("_stairs")
}

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
    /// Every kind the roof dispatch has a generator for.
    ///
    /// The enum is closed, so this is the whole set a `kind=` can name and
    /// lower. Held as a constant because a table elsewhere has to be
    /// compared against it: `MemberRole::conditional_arguments` says which
    /// of these read `slope_to=`, and a kind added here without a row there
    /// would go back to dropping the direction in silence.
    /// `every_roof_kind_the_dispatch_knows_has_an_arm` below is what fails
    /// instead.
    pub const ALL: [Self; 4] = [Self::Gable, Self::Shed, Self::Hip, Self::Flat];

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

    /// Every kind name, as a message lists them: `gable, shed, hip, flat`.
    ///
    /// Derived from [`Self::ALL`] so the closed set a refusal quotes cannot
    /// fall behind the set it is refusing against.
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
    /// The single cap on an apex row where the two slopes converge —
    /// an odd short span. Keeps the low slope's facing, which is the
    /// facing `spec/compilation` "Gable roof voxel rules" picks for it.
    /// `half=top`.
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
    // Each layer steps inward by 1 on each slope, so a span of `s` reaches
    // an apex at layer `ceil(s / 2)`.
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

/// Build the [`BlockState`] for one stair variant of a gable roof.
///
/// Pure: the same `(ridge_axis, face)` always produces the same state, so
/// callers can intern a small lookup table once and avoid the per-voxel
/// clone the earlier shape forced.
#[must_use]
pub fn gable_stair_state(ridge_axis: Axis, face: StairFace) -> BlockState {
    // A `half=top` stair fills the upper half of its voxel plus the lower
    // quarter on its facing side. An even-span apex is a *pair*
    // straddling the ridge, so each of the two faces away from it and
    // puts that quarter on the outer face; facing them inward instead
    // leaves the quarter empty there — a 0.5 x 0.5 undercut running the
    // length of the roof along both outer faces.
    //
    // A converged apex is one cell wide, so both of its faces are outer
    // ones and a stair can only serve one. The void is unavoidable there,
    // and `spec/compilation` "Gable roof voxel rules" settles which side
    // keeps it by naming the low slope's facing rather than by weighing the
    // two.
    let facing = match (ridge_axis, face) {
        // x-ridge: short axis is z. Low slope is on -z; its riser faces
        // toward +z (south) — the upper-step side ends up on the inward
        // side (toward the ridge). `ApexHigh` reaches the same `south`
        // from the opposite premise: it sits on +z, and +z facing
        // outward is south too.
        (Axis::X, StairFace::LowSlope | StairFace::Apex | StairFace::ApexHigh) => Cardinal::South,
        (Axis::X, StairFace::HighSlope | StairFace::ApexLow) => Cardinal::North,
        // z-ridge: short axis is x. Mirror the same rule onto the x axis.
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
        let low_index = layer;
        let high_index = span.saturating_sub(1).saturating_sub(layer);
        let converged = low_index == high_index;
        // Layer 0 sits directly on the wall top, so it is a slope
        // (`half=bottom`) whatever else it is. A roof one layer tall —
        // a short span of 1 or 2 — is both the first layer and the apex,
        // and capping it instead of seating it left the whole course
        // `half=top`: a half-block slit running the length of the roof
        // between the wall and the stone above it. An apex cap only
        // closes a peak something below it has already raised.
        let is_apex = layer > 0 && layer + 1 == layers;

        if converged {
            // The two slopes meet on one row, so it is emitted once. At the
            // apex that is the single cap of `spec/compilation` "Gable roof
            // voxel rules"; on layer 0 it is a short span of 1 — one row of
            // slope, with no ridge to straddle.
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
    /// `half=bottom` slope row. Every layer from `0..extra_height-1` uses
    /// this face.
    Slope,
    /// `half=top` apex cap on the highest layer.
    Apex,
}

/// One stair voxel produced by [`shed_voxels`].
pub type ShedVoxel = RoofVoxel<ShedFace>;

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
pub type HipVoxel = RoofVoxel<HipFace>;

/// Vertical rise of a hip roof above its wall top.
///
/// The short-axis slope reaches the ridge at `ceil(short_span / 2)`, as a
/// gable's does. The long-axis slope is shorter or equal and so finishes
/// at or before the same layer.
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
        // Layer 0 seats on the wall top and is always the inset frame,
        // even when it is also the last layer — a short span of 1 or 2
        // gives the roof a single course, and capping that course
        // instead of framing it dropped the `outer_*` corners and the
        // per-edge facings and left every cell `half=top`, a half-block
        // slit running the whole perimeter between the wall and the
        // roof above it.
        let is_apex = layer > 0 && layer + 1 == layers;

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
    // intern order is deterministic. Each of the four is guarded against
    // the axis it would collapse onto, and the row / column loops start
    // one cell inside the corners, so no position is emitted twice however
    // far the frame has collapsed — a frame one cell wide emits its north
    // and south rows and nothing else. The lowering relies on that: a
    // member writing one cell to two different blocks would be reported
    // against itself, which is a finding about two source lines and has
    // nothing to say about a generator.
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

/// Build a stair [`BlockState`] with the given base id, facing, half, and
/// shape properties. Exposed at `pub(super)` so the block-array lowering
/// module can build eave stair states without duplicating the property
/// layout (which pinned the on-disk NBT palette).
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
