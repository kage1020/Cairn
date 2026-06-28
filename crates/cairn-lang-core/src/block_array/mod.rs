//! Block-array IR — the universal voxel pivot below the semantic layer.
//!
//! Every format frontend and backend meets at this layer: it holds a voxel
//! grid, a palette of [`BlockState`]s, and (eventually) block entities and
//! entities, all neutral to file format, edition, and version. Compile diffs,
//! `IoU` comparisons, and serialisation hang off this single shape (see
//! `spec/architecture.md` §3.1).
//!
//! The [`lower::lower_to_block_array`] pass handles the cottage example's
//! roster end-to-end: `floor`, `walls`, `door`, `window`, and
//! `roof kind=gable`. Other roles (`stair`, `level`, `pressure_plate`, ...),
//! roof kinds (`hip`, `shed`, ...), and abstract material tokens degrade
//! to air with a `W_DEFERRED_MEMBER` warning rather than failing, so a
//! partial build is still inspectable.
//!
//! ## Overhang convention
//!
//! A `roof overhang=N` member inflates the struct's bounding box by `N`
//! blocks on each horizontal axis (so `size=9x7 overhang=1` produces a
//! `Dims { x: 11, y: …, z: 9 }`). Floors, walls, doors, and windows are
//! authored against the *interior* `size`; lowering shifts them inward by
//! `+N` along x and z so the overhang shows up only at the roof level.

mod lower;
mod material;
mod openings;
mod roof;
mod walkway;

use indexmap::IndexMap;
use serde::Serialize;

pub use lower::lower_to_block_array;
pub use material::{AbstractMaterialResolver, MaterialDeferred, resolve_block_state};
pub use walkway::{WalkwayLayout, build_walkway_array, l_path, port_world_position};

use crate::check::Diagnostic;
use crate::ids::{PlaceId, SiteName, WalkwayEndpoint, WalkwayScopeKey};

/// Lowered block-array IR for a whole [`crate::intent::IntentModule`].
///
/// Mirrors the total-plus-diagnostics shape of [`crate::resolve::Resolution`]
/// so a caller can pipeline `parse → lower → resolve → lower_to_block_array`
/// and surface every finding in one pass, even when some structures produced
/// no voxels.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockArrayIr {
    /// One entry per concretised structure, keyed by `kind::name`
    /// (`struct::cottage`, `site::hamlet::home1`). The key shape matches
    /// [`crate::resolve::Resolution::scopes`] so the two maps line up
    /// without an extra translation step.
    pub structures: IndexMap<String, BlockArray>,
    /// Per-`place` site coordinates. Keyed identically to [`structures`]
    /// (`site::HAMLET::PLACE_ID`) so a consumer can join the two without an
    /// extra index. Empty for any source with no `site` blocks, which keeps
    /// the JSON shape stable for cottage/themed-tower fixtures pre-dating
    /// site lowering.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub placements: IndexMap<String, Placement>,
    /// Per-`connect` walkway metadata, keyed by [`WalkwayScopeKey`]. The
    /// matching [`BlockArray`] sits under the same wire-format key in
    /// [`structures`], so a consumer can join the two against the same
    /// key the way `placements` does. Empty for any source with no
    /// `connect` rows, keeping the JSON shape stable for cottage /
    /// themed-tower fixtures.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub walkways: IndexMap<WalkwayScopeKey, Walkway>,
    /// Diagnostics emitted during lowering. Kept separate from
    /// [`crate::resolve::Resolution::diagnostics`]: the resolver owns
    /// theme-binding hygiene, this list owns voxel-lowering deferrals.
    #[serde(skip)]
    pub diagnostics: Vec<Diagnostic>,
}

/// One resolved `connect` row laid as a walkway between two ports.
///
/// Mirrors the shape of [`Placement`]: site / endpoint provenance plus
/// the world origin and footprint of the [`BlockArray`] this walkway
/// lowers to. The path material is recorded as the canonical Minecraft
/// id so the lockfile can round-trip the walkway palette without
/// rehydrating the per-walkway [`BlockArray`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Walkway {
    /// Bare site name (no `site::` IR-key prefix).
    pub site: SiteName,
    /// `from` endpoint — `(place, port)` of the walkway's first end.
    pub from: WalkwayEndpoint,
    /// `to` endpoint — `(place, port)` of the walkway's second end.
    pub to: WalkwayEndpoint,
    /// Absolute world-space origin `(x, y, z)` of the walkway
    /// [`BlockArray`]. `i32` because `north_of` placements grow the
    /// walkway into negative `z` just like the buildings themselves.
    pub origin: (i32, i32, i32),
    /// Horizontal extents of the walkway [`BlockArray`]. Walkways are
    /// always a single block thick, so the Y axis is fixed at `1` and
    /// elided from this field; lowering into the full
    /// [`BlockArray::dims`] re-attaches `y = 1` via
    /// [`Footprint::to_dims_y1`].
    pub footprint: Footprint,
    /// Canonical Minecraft id of the path material laid into the
    /// walkway (e.g. `minecraft:gravel`).
    pub path_material: String,
}

/// One resolved `place` line from a `site` block.
///
/// Carries the world-space origin the per-place [`BlockArray`] (stored under
/// the same key in [`BlockArrayIr::structures`]) lives at, plus the
/// def/theme provenance pair so the lockfile can record the inputs that
/// produced the absolute coordinates without re-walking the AST. `origin` is
/// `i32` because `north_of` placements grow into negative `z`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Placement {
    /// Site this placement belongs to (the bare `site` name, not the IR key).
    pub site: SiteName,
    /// `place id=` value.
    pub place_id: PlaceId,
    /// `place use=` target — the `def` whose body lowered into the per-place
    /// volume.
    pub source_def: String,
    /// `place theme=` value applied during cross-scope theme resolution.
    pub theme: String,
    /// Absolute world-space origin `(x, y, z)` resolved from the topological
    /// constraint chain (`at=origin`, `east_of=`, `north_of=`).
    pub origin: (i32, i32, i32),
    /// Voxel extents of the lowered per-place [`BlockArray`]. Mirrored here
    /// so the lockfile can pin the dims without rehydrating every
    /// [`BlockArray`] to compare against.
    pub dims: Dims,
}

/// One concretised structure as a voxel volume.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockArray {
    /// Voxel extents along X (east), Y (up), Z (south). Front of a struct is
    /// `+z` (`spec/components-editing-sites.md` §5.4) so the source
    /// `size=WxH` literal becomes `(W, _, H)` here; the Y extent is derived
    /// from member contributions.
    pub dims: Dims,
    /// Block states referenced by the [`voxels`] grid. Index `0` is always
    /// [`BlockState::AIR`]; [`Palette::intern`] preserves that invariant.
    pub palette: Palette,
    /// Palette indices in `(y, z, x)` order: `voxels[((y * dims.z) + z) *
    /// dims.x + x]`. Y-major lets ASCII renderers walk one slice at a time
    /// without re-striding.
    pub voxels: Vec<PaletteIndex>,
    /// Block entities (chests, signs, ...) that sit on the grid. Empty
    /// for now — the IR shape is reserved for the fixtures phase.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub block_entities: Vec<BlockEntity>,
    /// Free entities (frames, paintings, ...). Empty for the same reason
    /// as [`block_entities`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<Entity>,
    /// Source scope this volume was lowered from, mirroring the
    /// [`crate::resolve::Resolution`] key (`struct::cottage`). Carried in
    /// the IR so a downstream serialiser can stamp it into the output
    /// filename without re-threading the originating [`IntentModule`].
    pub source_scope: String,
}

/// Voxel extents of a [`BlockArray`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Dims {
    /// East/west extent (the source `size=WxH` `W`).
    pub x: u32,
    /// Up/down extent (derived from member contributions).
    pub y: u32,
    /// North/south extent (the source `size=WxH` `H`).
    pub z: u32,
}

/// Horizontal extents of a 1-block-thick volume.
///
/// Used by [`Walkway`] because walkways are always laid at a single Y;
/// having only `x` and `z` keeps the invariant in the type rather than
/// in an implicit `y == 1` convention spread across the codebase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Footprint {
    /// East/west extent.
    pub x: u32,
    /// North/south extent.
    pub z: u32,
}

impl Footprint {
    /// Inflate to a full [`Dims`] with `y = 1`. The single place that
    /// re-hydrates `y` for the lockfile / NBT path.
    #[must_use]
    pub fn to_dims_y1(self) -> Dims {
        Dims {
            x: self.x,
            y: 1,
            z: self.z,
        }
    }
}

impl Dims {
    /// Total voxel count. Returns `usize` rather than `u32` because
    /// downstream allocations index a `Vec`.
    #[must_use]
    pub fn volume(self) -> usize {
        (self.x as usize) * (self.y as usize) * (self.z as usize)
    }

    /// Linear offset of `(x, y, z)` into a `(y, z, x)`-ordered flat array.
    /// Returns `None` when the coordinate falls outside [`Self`] so callers
    /// can fail loud instead of indexing into the wrong cell.
    #[must_use]
    pub fn index(self, x: u32, y: u32, z: u32) -> Option<usize> {
        if x >= self.x || y >= self.y || z >= self.z {
            return None;
        }
        let row = (y as usize) * (self.z as usize) + (z as usize);
        Some(row * (self.x as usize) + (x as usize))
    }
}

/// Index into a [`Palette`]. `0` is reserved for [`BlockState::AIR`].
///
/// A newtype rather than a bare `u16` so a future change of the underlying
/// width (e.g. `u32` for very large palettes) does not ripple through every
/// call site, and so `palette.entries[i.0]` reads as "the entry the index
/// names" instead of an arbitrary number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PaletteIndex(pub u16);

impl PaletteIndex {
    /// Index reserved for air. Used by [`Palette::new_with_air`] and by the
    /// lowering pass to leave a cell empty.
    pub const AIR: Self = Self(0);
}

/// Append-only palette with deduplication on insertion. Insertion order is
/// preserved so the slot at index `0` is always [`BlockState::AIR`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Palette {
    /// Block states in insertion order.
    pub entries: Vec<BlockState>,
}

impl Palette {
    /// Construct a palette pre-seeded with [`BlockState::AIR`] at index `0`.
    /// This is the only constructor lowering should use so the AIR invariant
    /// holds for every [`BlockArray`].
    #[must_use]
    pub fn new_with_air() -> Self {
        Self {
            entries: vec![BlockState::air()],
        }
    }

    /// Look up or append a block state and return its index. O(n) over the
    /// current palette; the builds the compiler emits today are small enough
    /// (cottage = 3 distinct states) that a hash side-table would cost more
    /// than it saves.
    ///
    /// # Panics
    ///
    /// Panics if the palette would exceed `u16::MAX` entries. That cap
    /// matches [`PaletteIndex`]'s width and is far beyond any vanilla
    /// block-state count (~20k) intentionally lowered into one volume.
    pub fn intern(&mut self, state: BlockState) -> PaletteIndex {
        if let Some(i) = self.entries.iter().position(|s| s == &state) {
            return PaletteIndex(
                u16::try_from(i).expect("palette index fits in u16 by construction"),
            );
        }
        let idx = u16::try_from(self.entries.len())
            .expect("palette grew past u16::MAX entries; widen PaletteIndex first");
        self.entries.push(state);
        PaletteIndex(idx)
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::new_with_air()
    }
}

/// A resolved Minecraft block state: canonical id plus stringly-typed
/// properties.
///
/// The properties map is `String -> String` rather than a typed sum because
/// the block-array IR sits below the materials/registry layer and treats
/// state values as opaque payload — typing happens against the registry
/// pack one layer up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockState {
    /// Canonical id, e.g. `minecraft:cobblestone`.
    pub id: String,
    /// State properties in source-stable order.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub properties: IndexMap<String, String>,
}

impl BlockState {
    /// Canonical Minecraft id for the air block.
    pub const AIR_ID: &'static str = "minecraft:air";

    /// Construct the [`Self::AIR_ID`] state with no properties. Stays a
    /// function rather than an `AIR: BlockState` const so callers do not
    /// share an [`IndexMap`] allocation across palettes.
    #[must_use]
    pub fn air() -> Self {
        Self {
            id: Self::AIR_ID.to_owned(),
            properties: IndexMap::new(),
        }
    }

    /// Construct a property-free state. Shorthand for the common case where
    /// a slot binds to a bare `@cobblestone` with no state literal.
    #[must_use]
    pub fn bare(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            properties: IndexMap::new(),
        }
    }
}

/// Placeholder for an inhabited block entity slot.
///
/// The NBT payload is intentionally off this struct: no member role
/// currently lowers to a block entity, so committing to a concrete payload
/// shape now would lock in a representation before the format backends and
/// the `cairn-lang-nbt` crate exist to inform that choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockEntity {
    /// Voxel coordinate the block entity sits at.
    pub pos: (u32, u32, u32),
    /// Block entity id, e.g. `minecraft:chest`.
    pub id: String,
}

/// Placeholder for an entity (frames, paintings, item displays, ...).
///
/// Same deferral story as [`BlockEntity`]: the entity payload shape is the
/// format backend's choice and is intentionally absent for now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entity {
    /// World-space coordinate the entity sits at.
    pub pos: (f64, f64, f64),
    /// Entity id, e.g. `minecraft:item_frame`.
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dims_index_round_trips() {
        let d = Dims { x: 3, y: 2, z: 4 };
        // (1, 1, 2) → ((1 * 4) + 2) * 3 + 1 = 19
        assert_eq!(d.index(1, 1, 2), Some(19));
    }

    #[test]
    fn dims_index_rejects_out_of_bounds() {
        let d = Dims { x: 3, y: 2, z: 4 };
        assert!(d.index(3, 0, 0).is_none());
        assert!(d.index(0, 2, 0).is_none());
        assert!(d.index(0, 0, 4).is_none());
    }

    #[test]
    fn palette_seeds_air_at_index_zero() {
        let p = Palette::new_with_air();
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.entries[0].id, BlockState::AIR_ID);
    }

    #[test]
    fn palette_intern_dedupes() {
        let mut p = Palette::new_with_air();
        let a = p.intern(BlockState::bare("minecraft:cobblestone"));
        let b = p.intern(BlockState::bare("minecraft:cobblestone"));
        assert_eq!(a, b);
        assert_eq!(p.entries.len(), 2, "palette grew despite duplicate insert");
    }

    #[test]
    fn palette_intern_appends_distinct_states() {
        let mut p = Palette::new_with_air();
        let cobble = p.intern(BlockState::bare("minecraft:cobblestone"));
        let planks = p.intern(BlockState::bare("minecraft:oak_planks"));
        assert_ne!(cobble, planks);
        assert_eq!(p.entries.len(), 3);
    }
}
