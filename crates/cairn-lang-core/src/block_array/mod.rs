//! Block-array IR: a voxel grid and its palette, neutral to file format, edition and version
//! (`spec/architecture` "The block-array IR is the universal pivot").
//!
//! A roof's `overhang=N` widens the box by `N` on each side; every other member shifts `+N`
//! inward along x and z.

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
// `port_world_position` stays private: it needs the wall column the body was lowered against.

use crate::diagnostic::Diagnostic;
use crate::ids::{PlaceId, SiteName, WalkwayEndpoint, WalkwayScopeKey};

/// Lowered block-array IR for a whole [`crate::intent::IntentModule`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockArrayIr {
    /// One entry per concretised structure, keyed by `kind::name` (`struct::cottage`).
    pub structures: IndexMap<String, BlockArray>,
    /// Site coordinates per `place`, keyed like [`Self::structures`].
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub placements: IndexMap<String, Placement>,
    /// Walkway metadata per `connect`; each walkway's array sits in [`Self::structures`].
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub walkways: IndexMap<WalkwayScopeKey, Walkway>,
    /// Lowering's findings, apart from the resolver's.
    #[serde(skip)]
    pub diagnostics: Vec<Diagnostic>,
}

/// One resolved `connect` row laid as a walkway between two ports.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Walkway {
    /// Bare site name (no `site::` IR-key prefix).
    pub site: SiteName,
    /// `(place, port)` of the walkway's first end.
    pub from: WalkwayEndpoint,
    /// `(place, port)` of the walkway's second end.
    pub to: WalkwayEndpoint,
    /// World position of the walkway array's `(0, 0, 0)` cell.
    pub origin: (i32, i32, i32),
    /// Horizontal extents of the walkway's one-block-thick array.
    pub footprint: Footprint,
    /// Canonical id of the path material, e.g. `minecraft:gravel`.
    pub path_material: String,
}

/// One resolved `place` line from a `site` block.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Placement {
    /// Bare site name this placement belongs to.
    pub site: SiteName,
    /// `place id=` value.
    pub place_id: PlaceId,
    /// `place use=`: the `def` whose body this place lowers.
    pub source_def: String,
    /// `place theme=` value.
    pub theme: String,
    /// World position of the place's array's `(0, 0, 0)` cell.
    pub origin: (i32, i32, i32),
    /// Extents of the place's [`BlockArray`].
    pub dims: Dims,
}

/// One concretised structure as a voxel volume.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockArray {
    /// Voxel extents.
    pub dims: Dims,
    /// Block states the voxels index into; slot `0` is always air.
    pub palette: Palette,
    /// Palette indices in `(y, z, x)` order, as [`Dims::index`] lays them out.
    pub voxels: Vec<PaletteIndex>,
    /// Block entities on the grid; always empty, as no member lowers to one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub block_entities: Vec<BlockEntity>,
    /// Free entities; always empty, like [`Self::block_entities`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<Entity>,
    /// The [`crate::resolve::Resolution`] key this volume was lowered from (`struct::cottage`).
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
pub const MAX_STRUCTURE_VOLUME: usize = 256 * 256 * 256;

const _: () = assert!(
    crate::block_array::walkway::ROUTE_AREA_CAP <= MAX_STRUCTURE_VOLUME as u64,
    "a walkway inside the router's area cap must also be inside the structure volume bound",
);

impl Dims {
    /// Total voxel count, or `None` when the product overflows `usize`.
    #[must_use]
    pub fn checked_volume(self) -> Option<usize> {
        (self.x as usize)
            .checked_mul(self.y as usize)?
            .checked_mul(self.z as usize)
    }

    /// Whether this extent is within [`MAX_STRUCTURE_VOLUME`]; ask before allocating.
    #[must_use]
    pub fn fits_volume_budget(self) -> bool {
        self.checked_volume()
            .is_some_and(|volume| volume <= MAX_STRUCTURE_VOLUME)
    }

    /// Total voxel count, saturating at `usize::MAX`.
    #[must_use]
    pub fn volume(self) -> usize {
        (self.x as usize)
            .saturating_mul(self.y as usize)
            .saturating_mul(self.z as usize)
    }

    /// Linear offset of `(x, y, z)` into a `(y, z, x)`-ordered flat array, or `None` outside.
    #[must_use]
    pub fn index(self, x: u32, y: u32, z: u32) -> Option<usize> {
        if x >= self.x || y >= self.y || z >= self.z {
            return None;
        }
        let row = (y as usize) * (self.z as usize) + (z as usize);
        Some(row * (self.x as usize) + (x as usize))
    }
}

/// Index into a [`Palette`]; [`Self::AIR`] (`0`) is reserved for air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PaletteIndex(pub u16);

impl PaletteIndex {
    /// Index reserved for air.
    pub const AIR: Self = Self(0);
}

impl BlockArray {
    /// Put the palette in canonical order and renumber the grid onto it.
    ///
    /// A voxel outside the palette keeps its index, for [`Self::first_index_outside_palette`].
    pub fn canonicalize_palette(&mut self) {
        let remap = self.palette.canonicalize();
        for index in &mut self.voxels {
            if let Some(moved) = remap.get(usize::from(index.0)) {
                *index = *moved;
            }
        }
    }

    /// The first voxel index that is not a slot of [`Self::palette`], as `(index, palette len)`.
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

/// The most entries one [`Palette`] holds, air included.
pub const PALETTE_CAPACITY: usize = u16::MAX as usize + 1;

/// A new block state was refused because the palette already held [`Self::capacity`] entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "the palette already holds {capacity} entries, its capacity, and refuses a new block state"
)]
#[non_exhaustive]
pub struct PaletteFull {
    /// The most entries the palette could hold, air included.
    pub capacity: usize,
}

/// Deduplicating block-state palette; slot `0` is always [`BlockState::air()`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Palette {
    /// The block states, air first.
    pub entries: Vec<BlockState>,
}

impl Palette {
    /// A palette holding only [`BlockState::air()`], at index `0`.
    #[must_use]
    pub fn new_with_air() -> Self {
        Self {
            entries: vec![BlockState::air()],
        }
    }

    /// Look up or append a block state and return its index.
    ///
    /// Only for states bounded by construction, like a walkway's; a body uses [`Self::try_intern`].
    ///
    /// # Panics
    ///
    /// When `state` is new and the palette already holds [`PALETTE_CAPACITY`] entries.
    pub fn intern(&mut self, state: BlockState) -> PaletteIndex {
        self.try_intern(state)
            .expect("palette grew past PALETTE_CAPACITY entries; widen PaletteIndex first")
    }

    /// [`Self::intern`], answering [`PaletteFull`] instead of panicking.
    ///
    /// # Errors
    ///
    /// [`PaletteFull`] when `state` is new and the palette is full.
    ///
    /// # Panics
    ///
    /// If the first entry equal to `state` sits past index `u16::MAX`.
    pub fn try_intern(&mut self, state: BlockState) -> Result<PaletteIndex, PaletteFull> {
        self.try_intern_within(state, PALETTE_CAPACITY)
    }

    /// [`Self::try_intern`] against `capacity` entries, clamped to [`PALETTE_CAPACITY`].
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

    /// Reorder the entries canonically and return the old-slot → new-slot map.
    ///
    /// Air keeps slot `0`; the rest sort by [`BlockState::canonical_key`] (`spec/compilation`
    /// "Within-phase conflicts and the palette").
    ///
    /// # Panics
    ///
    /// On a palette longer than [`PALETTE_CAPACITY`] entries.
    #[must_use]
    pub fn canonicalize(&mut self) -> Vec<PaletteIndex> {
        // A hand-assembled palette may be empty, and `drain(1..)` panics on it.
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

/// A resolved Minecraft block state: canonical id plus stringly-typed properties.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockState {
    /// Canonical id, e.g. `minecraft:cobblestone`.
    pub id: String,
    /// State properties; a state literal's arrive sorted by name.
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

    /// The key [`Palette::canonicalize`] orders non-air slots by: the id, then the properties
    /// sorted by name.
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

    /// The properties as a state literal writes them between the brackets, such as
    /// `facing=east,half=top`.
    #[must_use]
    pub fn properties_text(&self) -> String {
        self.properties
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// A block entity on the grid, without its NBT payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockEntity {
    /// Voxel coordinate the block entity sits at.
    pub pos: (u32, u32, u32),
    /// Block entity id, e.g. `minecraft:chest`.
    pub id: String,
}

/// An entity (frames, paintings, item displays, ...), without its payload.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entity {
    /// World-space coordinate the entity sits at.
    pub pos: (f64, f64, f64),
    /// Entity id, e.g. `minecraft:item_frame`.
    pub id: String,
}

#[cfg(test)]
mod tests;
