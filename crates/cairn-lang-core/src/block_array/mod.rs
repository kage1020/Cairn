//! Block-array IR — the universal voxel pivot below the semantic layer.
//!
//! Every format frontend and backend meets at this layer: a voxel grid, a
//! palette of [`BlockState`]s, and slots for block entities and entities,
//! all neutral to file format, edition, and version. Compile diffs, `IoU`
//! comparisons, and serialisation hang off this one shape
//! (`spec/architecture` "The block-array IR is the universal pivot").
//!
//! A member [`lower::lower_to_block_array`] cannot voxelise becomes air with
//! a `W_DEFERRED_MEMBER` warning rather than failing, so a partial build is
//! still inspectable; the same warning covers an abstract material token
//! reached with no registry pack to lift it.
//!
//! ## Overhang convention
//!
//! A `roof overhang=N` member inflates the struct's bounding box by `N`
//! blocks on each horizontal axis (so `size=9x7 overhang=1` produces a
//! `Dims { x: 11, y: …, z: 9 }`). Floors, walls, doors, and windows are
//! authored against the *interior* `size`; lowering shifts them inward by
//! `+N` along x and z so the overhang shows up only at the roof level.

mod args;
mod lower;
mod material;
mod openings;
mod ports;
mod roof;
mod walkway;
mod wall_column;

use indexmap::IndexMap;
use serde::Serialize;

pub use lower::{BUILTIN_BLOCK_IDS, lower_to_block_array};
pub use material::{
    BlockIdSet, IdOrigin, MaterialDeferred, TargetRegistry, UnknownId, resolve_block_state,
};
pub use roof::is_stair;
// `port_world_position` is not re-exported: it answers correctly only with
// the wall column the body was lowered against, which nothing outside this
// module holds.

use crate::diagnostic::Diagnostic;
use crate::ids::{PlaceId, SiteName, WalkwayEndpoint, WalkwayScopeKey};

/// Lowered block-array IR for a whole [`crate::intent::IntentModule`].
///
/// Total plus diagnostics, like [`crate::resolve::Resolution`], so a caller
/// surfaces every finding even when some structures produced no voxels.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockArrayIr {
    /// One entry per concretised structure, keyed by `kind::name`
    /// (`struct::cottage`, `site::hamlet::home1`), the keys of
    /// [`crate::resolve::Resolution::scopes`].
    pub structures: IndexMap<String, BlockArray>,
    /// Per-`place` site coordinates, under the same keys as
    /// [`Self::structures`]. Omitted from the JSON when empty, so a source
    /// with no `site` block keeps its shape.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub placements: IndexMap<String, Placement>,
    /// Per-`connect` walkway metadata. Each walkway's [`BlockArray`] sits in
    /// [`Self::structures`] under the wire form of the same key. Omitted
    /// from the JSON when empty, like [`Self::placements`].
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub walkways: IndexMap<WalkwayScopeKey, Walkway>,
    /// Lowering's findings, kept apart from the resolver's
    /// [`crate::resolve::Resolution::diagnostics`].
    #[serde(skip)]
    pub diagnostics: Vec<Diagnostic>,
}

/// One resolved `connect` row laid as a walkway between two ports.
///
/// The path material is kept as its canonical id so the lockfile can
/// record the walkway without rehydrating its [`BlockArray`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Walkway {
    /// Bare site name (no `site::` IR-key prefix).
    pub site: SiteName,
    /// `(place, port)` of the walkway's first end.
    pub from: WalkwayEndpoint,
    /// `(place, port)` of the walkway's second end.
    pub to: WalkwayEndpoint,
    /// World-space origin `(x, y, z)` of the walkway's [`BlockArray`];
    /// signed because `north_of` placements reach negative `z`.
    pub origin: (i32, i32, i32),
    /// Horizontal extents of the walkway's one-block-thick [`BlockArray`];
    /// [`Footprint::to_dims_y1`] gives its [`BlockArray::dims`].
    pub footprint: Footprint,
    /// Canonical id of the path material, e.g. `minecraft:gravel`.
    pub path_material: String,
}

/// One resolved `place` line from a `site` block: where its [`BlockArray`]
/// (under the same key in [`BlockArrayIr::structures`]) sits, and the def
/// and theme that produced it, so the lockfile records them without
/// re-walking the AST.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Placement {
    /// Site this placement belongs to (the bare `site` name, not the IR key).
    pub site: SiteName,
    /// `place id=` value.
    pub place_id: PlaceId,
    /// `place use=`: the `def` whose body this place lowers.
    pub source_def: String,
    /// `place theme=` value applied during cross-scope theme resolution.
    pub theme: String,
    /// World-space origin `(x, y, z)` resolved from the constraint chain
    /// (`at=origin`, `east_of=`, `north_of=`); signed because `north_of`
    /// reaches negative `z`.
    pub origin: (i32, i32, i32),
    /// Extents of the place's [`BlockArray`], copied here so the lockfile
    /// pins them without rehydrating it.
    pub dims: Dims,
}

/// One concretised structure as a voxel volume.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockArray {
    /// Voxel extents along X (east), Y (up), Z (south). Front of a struct is
    /// `+z` (`spec/syntax` "Selectors") so the source `size=WxH` literal
    /// becomes `(W, _, H)` here; the Y extent is derived from member
    /// contributions.
    pub dims: Dims,
    /// Block states the [`Self::voxels`] index into; slot `0` is always
    /// [`BlockState::air()`].
    ///
    /// Lowering leaves it in [`Palette::canonicalize`]'s order. Nothing
    /// enforces that on an array assembled by hand, as the format backends'
    /// tests do.
    pub palette: Palette,
    /// Palette indices in `(y, z, x)` order: `voxels[((y * dims.z) + z) *
    /// dims.x + x]`. Y-major lets ASCII renderers walk one slice at a time
    /// without re-striding.
    pub voxels: Vec<PaletteIndex>,
    /// Block entities (chests, signs, ...) on the grid. Always empty: no
    /// member lowers to one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub block_entities: Vec<BlockEntity>,
    /// Free entities (frames, paintings, ...). Always empty, like
    /// [`Self::block_entities`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<Entity>,
    /// Scope this volume was lowered from, as its
    /// [`crate::resolve::Resolution`] key (`struct::cottage`), so a
    /// serialiser can name the output file without the
    /// [`crate::intent::IntentModule`].
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

/// Horizontal extents of a one-block-thick volume, such as a [`Walkway`].
/// Having no `y` keeps the thickness in the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Footprint {
    /// East/west extent.
    pub x: u32,
    /// North/south extent.
    pub z: u32,
}

impl Footprint {
    /// The [`Dims`] of this footprint, one block high.
    #[must_use]
    pub fn to_dims_y1(self) -> Dims {
        Dims {
            x: self.x,
            y: 1,
            z: self.z,
        }
    }
}

/// Largest voxel volume the lowering pass builds for one scope: a 256-cube.
///
/// It turns an arithmetic accident into a diagnostic rather than
/// constraining real builds: `size=100000x100000` or
/// `walls height=2147483647` passes every per-key range check, and only the
/// product is too large.
///
/// At the bound the index vector alone is 32 MB (`PaletteIndex` is two
/// bytes): far past a useful output, far below what the allocator refuses.
/// `cairn-lang-formats` writes from this vector as it goes, adding a fixed
/// amount rather than a multiple of it.
///
/// Per scope, not per module: a thousand structs may ask for a thousand
/// times this. Bounding the total needs a budget threaded through the whole
/// pass, which only pays once scopes stream to disk as they finish.
pub const MAX_STRUCTURE_VOLUME: usize = 256 * 256 * 256;

// Walkways are bounded by `walkway::ROUTE_AREA_CAP` instead, a cell count
// over a one-block-thick plane. Keeping it under this bound keeps
// `Dims::volume`'s promise for walkway arrays too.
const _: () = assert!(
    crate::block_array::walkway::ROUTE_AREA_CAP <= MAX_STRUCTURE_VOLUME as u64,
    "a walkway inside the router's area cap must also be inside the structure volume bound",
);

impl Dims {
    /// Total voxel count, or `None` when the product overflows `usize`, as
    /// `size=4294967295x4294967295` does. Unchecked, that product panics in
    /// a debug build and wraps in a release one, to a count that disagrees
    /// with [`Self::index`].
    #[must_use]
    pub fn checked_volume(self) -> Option<usize> {
        (self.x as usize)
            .checked_mul(self.y as usize)?
            .checked_mul(self.z as usize)
    }

    /// Whether this extent is within [`MAX_STRUCTURE_VOLUME`].
    ///
    /// Callers that are about to allocate must ask this first; the lowering
    /// pass reports [`crate::diagnostic::DiagnosticCode::StructureTooLarge`] and
    /// skips the scope when it says no.
    #[must_use]
    pub fn fits_volume_budget(self) -> bool {
        self.checked_volume()
            .is_some_and(|volume| volume <= MAX_STRUCTURE_VOLUME)
    }

    /// Total voxel count, saturating at `usize::MAX`. Exact for any [`Dims`]
    /// that passed [`Self::fits_volume_budget`], as every lowered
    /// [`BlockArray`]'s has.
    #[must_use]
    pub fn volume(self) -> usize {
        (self.x as usize)
            .saturating_mul(self.y as usize)
            .saturating_mul(self.z as usize)
    }

    /// Linear offset of `(x, y, z)` into a `(y, z, x)`-ordered flat array,
    /// or `None` outside [`Self`].
    #[must_use]
    pub fn index(self, x: u32, y: u32, z: u32) -> Option<usize> {
        if x >= self.x || y >= self.y || z >= self.z {
            return None;
        }
        let row = (y as usize) * (self.z as usize) + (z as usize);
        Some(row * (self.x as usize) + (x as usize))
    }
}

/// Index into a [`Palette`]. [`Self::AIR`] (`0`) is reserved for
/// [`BlockState::air()`].
///
/// A newtype rather than a bare `u16` so widening it does not ripple
/// through every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PaletteIndex(pub u16);

impl PaletteIndex {
    /// Index reserved for air.
    pub const AIR: Self = Self(0);
}

impl BlockArray {
    /// Put the palette in [`Palette::canonicalize`]'s order and renumber
    /// the grid onto it, once the grid is finished.
    ///
    /// A voxel naming a slot the palette lacks is left as it is rather than
    /// mapped to air, so [`Self::first_index_outside_palette`] still
    /// reports it.
    pub fn canonicalize_palette(&mut self) {
        let remap = self.palette.canonicalize();
        for index in &mut self.voxels {
            if let Some(moved) = remap.get(usize::from(index.0)) {
                *index = *moved;
            }
        }
    }

    /// The first voxel index that is not a slot of [`Self::palette`], as
    /// `(index, palette length)`.
    ///
    /// Always `None` for a lowered array, but a hand-assembled one can
    /// disagree, and a serialiser that wrote it would name a slot the
    /// reader has to invent; both backends ask this first.
    #[must_use]
    pub fn first_index_outside_palette(&self) -> Option<(u16, usize)> {
        let len = self.palette.entries.len();
        self.voxels
            .iter()
            .map(|index| index.0)
            .find(|index| usize::from(*index) >= len)
            .map(|index| (index, len))
    }
}

/// The most entries one [`Palette`] holds, air included: one per value a
/// [`PaletteIndex`] can name.
pub const PALETTE_CAPACITY: usize = u16::MAX as usize + 1;

/// A new block state was refused because the palette already held
/// [`Self::capacity`] entries.
///
/// It carries the capacity in force because lowering's tests paint against
/// one smaller than [`PALETTE_CAPACITY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "the palette already holds {capacity} entries, its capacity, and refuses a new block state"
)]
#[non_exhaustive]
pub struct PaletteFull {
    /// The most entries the palette could hold, air included; never more
    /// than [`PALETTE_CAPACITY`].
    pub capacity: usize,
}

/// Deduplicating palette. [`Palette::intern`] appends, so the order while
/// a body is being painted is insertion order; [`Palette::canonicalize`]
/// then rewrites it into the order the artifact carries. The slot at index
/// `0` is always [`BlockState::air()`] under both.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Palette {
    /// The block states, air first.
    pub entries: Vec<BlockState>,
}

impl Palette {
    /// A palette holding only [`BlockState::air()`], at index `0`. Lowering
    /// builds every palette from this so [`PaletteIndex::AIR`] holds.
    #[must_use]
    pub fn new_with_air() -> Self {
        Self {
            entries: vec![BlockState::air()],
        }
    }

    /// Look up or append a block state and return its index. A linear
    /// scan: palettes are small enough that a hash side-table would cost
    /// more than it saves.
    ///
    /// Only for a caller whose states are bounded by construction, such as
    /// a walkway. A body is not: block ids and state-literal properties no
    /// pinned target checks (`W_STATE_LITERAL_UNCHECKED`) reach any number
    /// of states, so it interns through [`Self::try_intern`].
    ///
    /// # Panics
    ///
    /// Panics when `state` is new and the palette already holds
    /// [`PALETTE_CAPACITY`] entries, and wherever [`Self::try_intern`]
    /// panics.
    pub fn intern(&mut self, state: BlockState) -> PaletteIndex {
        self.try_intern(state)
            .expect("palette grew past PALETTE_CAPACITY entries; widen PaletteIndex first")
    }

    /// [`Self::intern`], answering [`PaletteFull`] instead of panicking. A
    /// state already present answers its index however full the palette
    /// is.
    ///
    /// # Errors
    ///
    /// [`PaletteFull`] when `state` is new and the palette already holds
    /// [`PALETTE_CAPACITY`] entries. Nothing is appended.
    ///
    /// # Panics
    ///
    /// Panics if the first entry equal to `state` sits past index
    /// `u16::MAX`, which only [`Self::entries`] assembled by hand can hold.
    pub fn try_intern(&mut self, state: BlockState) -> Result<PaletteIndex, PaletteFull> {
        self.try_intern_within(state, PALETTE_CAPACITY)
    }

    /// [`Self::try_intern`] against `capacity` entries, clamped to
    /// [`PALETTE_CAPACITY`]. Lowering's tests pass a small one so the
    /// refusal is reachable without painting 65,536 states.
    pub(crate) fn try_intern_within(
        &mut self,
        state: BlockState,
        capacity: usize,
    ) -> Result<PaletteIndex, PaletteFull> {
        if let Some(i) = self.entries.iter().position(|s| s == &state) {
            return Ok(PaletteIndex(
                u16::try_from(i).expect("palette index fits in u16 by construction"),
            ));
        }
        let capacity = capacity.min(PALETTE_CAPACITY);
        if self.entries.len() >= capacity {
            return Err(PaletteFull { capacity });
        }
        let idx = u16::try_from(self.entries.len())
            .expect("a palette below PALETTE_CAPACITY has an index left");
        self.entries.push(state);
        Ok(PaletteIndex(idx))
    }

    /// Reorder the entries into canonical order and return the old-slot →
    /// new-slot map, for renumbering a grid painted against the old order.
    /// [`BlockArray::canonicalize_palette`] does both.
    ///
    /// Air keeps slot `0`, the one position a reader may assume (and
    /// `minecraft:acacia_log` sorts ahead of it); every other entry is
    /// placed by [`BlockState::canonical_key`]. Painting numbers a palette
    /// by first use, so without this, two members sharing no voxel would
    /// number it by the order their lines are written, and that numbering
    /// reaches the `.nbt` bytes, `cairn info`'s rows and `resolved_ir_hash`.
    /// Sorted, it is a function of the set of states alone
    /// (`spec/compilation` "Within-phase conflicts and the palette").
    ///
    /// # Panics
    ///
    /// Panics on a palette longer than [`PALETTE_CAPACITY`] entries, which
    /// only hand assembly can build.
    #[must_use]
    pub fn canonicalize(&mut self) -> Vec<PaletteIndex> {
        // A hand-assembled palette may be empty, and `drain(1..)` would
        // panic on it.
        if self.entries.is_empty() {
            return Vec::new();
        }
        let mut tagged: Vec<(usize, BlockState)> = self
            .entries
            .drain(1..)
            .enumerate()
            .map(|(i, state)| (i + 1, state))
            .collect();
        tagged.sort_by(|a, b| a.1.canonical_key().cmp(&b.1.canonical_key()));

        let mut remap = vec![PaletteIndex::AIR; tagged.len() + 1];
        for (new_slot, (old_slot, _)) in tagged.iter().enumerate() {
            remap[*old_slot] = PaletteIndex(
                u16::try_from(new_slot + 1).expect("a reordering is no longer than the original"),
            );
        }
        self.entries
            .extend(tagged.into_iter().map(|(_, state)| state));
        remap
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
/// Property values are strings because typing them is the registry pack's
/// job, a layer above this IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockState {
    /// Canonical id, e.g. `minecraft:cobblestone`.
    pub id: String,
    /// State properties. A state literal's arrive sorted by name, so the
    /// order a source spelled them in reaches no output.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub properties: IndexMap<String, String>,
}

impl BlockState {
    /// Canonical Minecraft id for the air block.
    pub const AIR_ID: &'static str = "minecraft:air";

    /// The [`Self::AIR_ID`] state, with no properties.
    #[must_use]
    pub fn air() -> Self {
        Self::bare(Self::AIR_ID)
    }

    /// A state with no properties, such as a bare `@cobblestone`'s.
    #[must_use]
    pub fn bare(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            properties: IndexMap::new(),
        }
    }

    /// The key [`Palette::canonicalize`] orders non-air slots by: the id,
    /// then the properties sorted by name.
    ///
    /// Sorted rather than in insertion order: two states with the same
    /// pairs in different orders are equal and intern to one slot, and
    /// reading the pairs in place would order the palette by whichever of
    /// them was interned first.
    #[must_use]
    pub fn canonical_key(&self) -> (&str, Vec<(&str, &str)>) {
        let mut properties: Vec<(&str, &str)> = self
            .properties
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        properties.sort_unstable();
        (self.id.as_str(), properties)
    }

    /// The properties as a state literal writes them between the brackets:
    /// `key=value` pairs joined by commas, in insertion order, such as
    /// `facing=east,half=top`. Empty for a state with no properties.
    #[must_use]
    pub fn properties_text(&self) -> String {
        self.properties
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// A block entity on the grid, without its NBT payload: no member lowers to
/// a block entity, so there is no use yet to choose the payload's shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockEntity {
    /// Voxel coordinate the block entity sits at.
    pub pos: (u32, u32, u32),
    /// Block entity id, e.g. `minecraft:chest`.
    pub id: String,
}

/// An entity (frames, paintings, item displays, ...), without its payload
/// for the same reason as [`BlockEntity`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entity {
    /// World-space coordinate the entity sits at.
    pub pos: (f64, f64, f64),
    /// Entity id, e.g. `minecraft:item_frame`.
    pub id: String,
}

#[cfg(test)]
mod tests;
