//! Block-array IR — the universal voxel pivot below the semantic layer.
//!
//! Every format frontend and backend meets at this layer: it holds a voxel
//! grid, a palette of [`BlockState`]s, and (eventually) block entities and
//! entities, all neutral to file format, edition, and version. Compile diffs,
//! `IoU` comparisons, and serialisation hang off this single shape (see
//! `spec/architecture.md` §3.1).
//!
//! The [`lower::lower_to_block_array`] pass handles `floor`, `walls`,
//! `door`, `window`, `stair`, `pressure_plate`, the `level y=N` grouping,
//! and all four roof kinds. A role it does not yet voxelise degrades to
//! air with a `W_DEFERRED_MEMBER` warning rather than failing, so a partial
//! build is still inspectable; the same warning covers an abstract material
//! token reached with no registry pack to lift it.
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
mod wall_column;

use indexmap::IndexMap;
use serde::Serialize;

pub use lower::{BUILTIN_BLOCK_IDS, lower_to_block_array};
pub use material::{
    BlockIdSet, IdOrigin, MaterialDeferred, TargetRegistry, UnknownId, resolve_block_state,
};
pub use roof::is_stair;
// `port_world_position` is deliberately not re-exported: it can only be
// asked correctly with the wall column the body was lowered against,
// which nothing outside this module holds. Exporting it invited the
// second derivation the port used to carry.
pub use walkway::{
    BlockedIndex, RoutePathError, WalkwayLayout, build_walkway_array, l_path, route_path,
};

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
    /// Block states referenced by the [`voxels`] grid, in the canonical
    /// order [`Palette::canonicalize`] fixes: air at slot `0`, the rest
    /// ascending by `(id, properties)`. Index `0` is always
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

/// Largest voxel volume the lowering pass will build for one scope.
///
/// A 256-cube. It exists to turn an arithmetic accident into a diagnostic,
/// not to constrain anything anyone writes: the largest shipped example is
/// under 2500 voxels, while `size=100000x100000` asks for 10^10 and
/// `walls height=2147483647` for more again. Every one of those numbers is
/// a valid `u32` on its own, so no per-key range check catches them — only
/// the product does.
///
/// A structure at the bound already costs 32 MB for the index vector alone
/// (`PaletteIndex` is two bytes), so the ceiling is well past where the
/// output stops being useful and well below where the allocator gives up.
///
/// Per scope, not per module: a source declaring a thousand structs can
/// still ask for a thousand times this. Bounding the total would need a
/// budget threaded through the whole pass, and nothing today streams
/// scopes to disk as they finish, so the two would have to land together.
pub const MAX_STRUCTURE_VOLUME: usize = 256 * 256 * 256;

// Walkways are the other thing the pass allocates, and they are bounded by
// `walkway::ROUTE_AREA_CAP` instead — a cell count over a one-block-thick
// plane, so it is directly comparable. Keeping it under the volume bound is
// what makes `Dims::volume`'s claim below true of every array this pass
// produces, walkways included.
const _: () = assert!(
    crate::block_array::walkway::ROUTE_AREA_CAP <= MAX_STRUCTURE_VOLUME as u64,
    "a walkway inside the router's area cap must also be inside the structure volume bound",
);

impl Dims {
    /// Total voxel count, or `None` when the product overflows `usize`.
    ///
    /// The only honest way to ask: `size=4294967295x4294967295` overflows
    /// the multiplication itself, which panics in a debug build and — worse
    /// — wraps in a release one, handing back a small number that then
    /// disagrees with [`Self::index`] and turns into an out-of-bounds write.
    #[must_use]
    pub fn checked_volume(self) -> Option<usize> {
        (self.x as usize)
            .checked_mul(self.y as usize)?
            .checked_mul(self.z as usize)
    }

    /// Whether this extent is within [`MAX_STRUCTURE_VOLUME`].
    ///
    /// Callers that are about to allocate must ask this first; the lowering
    /// pass reports [`crate::check::DiagnosticCode::StructureTooLarge`] and
    /// skips the scope when it says no.
    #[must_use]
    pub fn fits_volume_budget(self) -> bool {
        self.checked_volume()
            .is_some_and(|volume| volume <= MAX_STRUCTURE_VOLUME)
    }

    /// Total voxel count, saturating at `usize::MAX`.
    ///
    /// Safe to call on any [`Dims`], but only meaningful on one that has
    /// already passed [`Self::fits_volume_budget`] — which every
    /// [`BlockArray`] the lowering pass produces has, so readers of a built
    /// array can use it directly.
    #[must_use]
    pub fn volume(self) -> usize {
        (self.x as usize)
            .saturating_mul(self.y as usize)
            .saturating_mul(self.z as usize)
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

impl BlockArray {
    /// Put the palette in canonical order and renumber the grid onto it.
    ///
    /// The pass that paints a body numbers its palette by first use, and
    /// first use is a paint, so two members that share no voxel still
    /// reach the palette in the order their lines happen to be written —
    /// and the numbering rides into the `.nbt` bytes, `cairn info`'s
    /// per-entry rows, and `resolved_ir_hash`. Calling this once the grid
    /// is finished makes all three a function of the grid alone
    /// (`spec/compilation.md` §4.8).
    ///
    /// A voxel naming a slot the palette does not have is left as it is
    /// rather than remapped onto air: the fields here are public, so a
    /// caller can assemble a disagreeing pair by hand, and quietly
    /// rewriting one is how [`Self::first_index_outside_palette`] — which
    /// both backends ask before they assemble anything — would stop
    /// seeing it.
    pub fn canonicalize_palette(&mut self) {
        let remap = self.palette.canonicalize();
        for index in &mut self.voxels {
            if let Some(moved) = remap.get(usize::from(index.0)) {
                *index = *moved;
            }
        }
    }

    /// The first voxel whose index is not a slot of [`Self::palette`], as
    /// `(index, palette length)`.
    ///
    /// [`Palette::intern`] is the only source of a [`PaletteIndex`] inside
    /// the compiler, so a lowered array always satisfies this — but the
    /// fields above are public, so a caller assembling one by hand can
    /// have the grid and the palette disagree. A serialiser that writes
    /// the index anyway produces a file naming a slot the reader has to
    /// invent, which is why both backends ask this before they assemble
    /// anything.
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

/// Deduplicating palette. [`Palette::intern`] appends, so the order while
/// a body is being painted is insertion order; [`Palette::canonicalize`]
/// then rewrites it into the order the artifact carries. The slot at index
/// `0` is always [`BlockState::AIR`] under both.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Palette {
    /// Block states, in insertion order while the body is painted and in
    /// [`Palette::canonicalize`]'s order once it is finished.
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

    /// Reorder the entries into the canonical order and return the
    /// old-slot → new-slot map, so the caller can renumber a voxel grid
    /// that was painted against the old numbering.
    ///
    /// Air keeps slot `0` — it is the one entry a reader may assume the
    /// position of, and `minecraft:acacia_log` sorts ahead of
    /// `minecraft:air` — and every other entry is placed by
    /// [`BlockState::canonical_key`]: the id, then the properties sorted
    /// by name. The result is a function of the *set* of states the body
    /// contains, which is what makes the palette a rendering of the
    /// finished grid rather than a log of the order the paints happened
    /// to run in. Two sources that differ only in the order they declare
    /// two members that share no voxel then produce the same `.nbt` bytes
    /// and the same `resolved_ir_hash` (`spec/compilation.md` §4.8).
    ///
    /// Only the palette moves: this hands back the map rather than
    /// touching a grid it does not own, because a [`Palette`] outside a
    /// [`BlockArray`] has no grid. [`BlockArray::canonicalize_palette`]
    /// is the pairing that does both.
    ///
    /// # Panics
    ///
    /// Panics if the palette holds more than `u16::MAX` entries, which
    /// [`Self::intern`] refuses to build in the first place — the two
    /// share [`PaletteIndex`]'s width.
    #[must_use]
    pub fn canonicalize(&mut self) -> Vec<PaletteIndex> {
        // Sort a permutation rather than the entries, because the map back
        // to the old numbering is the half the caller needs and sorting
        // the entries in place throws it away.
        let mut order: Vec<usize> = (1..self.entries.len()).collect();
        order.sort_by(|a, b| {
            self.entries[*a]
                .canonical_key()
                .cmp(&self.entries[*b].canonical_key())
        });
        let mut remap = vec![PaletteIndex::AIR; self.entries.len()];
        for (new_slot, old_slot) in order.iter().enumerate() {
            remap[*old_slot] = PaletteIndex(
                u16::try_from(new_slot + 1).expect("a reordering is no longer than the original"),
            );
        }
        let mut entries = Vec::with_capacity(self.entries.len());
        // `swap_remove`-free: take the air slot first, then walk `order`,
        // which is a permutation of `1..len`, so every entry moves exactly
        // once and none is cloned.
        let mut taken: Vec<Option<BlockState>> = self.entries.drain(..).map(Some).collect();
        if let Some(air) = taken.first_mut().and_then(Option::take) {
            entries.push(air);
        }
        for old_slot in order {
            entries.push(
                taken[old_slot]
                    .take()
                    .expect("each slot appears once in the permutation"),
            );
        }
        self.entries = entries;
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

    /// Total order over block states, used by [`Palette::canonicalize`] to
    /// place every non-air slot.
    ///
    /// The properties are sorted by name here rather than read in the
    /// order they were inserted. [`IndexMap`] compares as a map, so two
    /// states carrying the same pairs in different orders are already
    /// [`PartialEq`]-equal and [`Palette::intern`] folds them onto one
    /// slot — but their iteration orders differ, and a key that read them
    /// in place would order the palette by which of the two was interned
    /// first, which is the source-order dependence this ordering exists to
    /// remove.
    ///
    /// Borrows rather than owning: it is called `O(n log n)` times inside
    /// one sort, and every id and value it names is already in the
    /// palette.
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

    fn state(id: &str, properties: &[(&str, &str)]) -> BlockState {
        BlockState {
            id: id.to_owned(),
            properties: properties
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    fn ids(p: &Palette) -> Vec<&str> {
        p.entries.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn canonicalize_sorts_by_id_and_pins_air_at_slot_zero() {
        // `minecraft:acacia_log` sorts ahead of `minecraft:air`, so the
        // pin is doing work here rather than agreeing with the sort.
        let mut p = Palette::new_with_air();
        p.intern(BlockState::bare("minecraft:oak_planks"));
        p.intern(BlockState::bare("minecraft:acacia_log"));
        p.intern(BlockState::bare("minecraft:cobblestone"));
        let remap = p.canonicalize();
        assert_eq!(
            ids(&p),
            [
                "minecraft:air",
                "minecraft:acacia_log",
                "minecraft:cobblestone",
                "minecraft:oak_planks",
            ],
        );
        // The map has to name where each old slot went, or a grid painted
        // against the old numbering cannot follow.
        assert_eq!(
            remap,
            vec![
                PaletteIndex(0),
                PaletteIndex(3),
                PaletteIndex(1),
                PaletteIndex(2),
            ],
        );
    }

    #[test]
    fn canonicalize_breaks_an_id_tie_on_the_properties() {
        let mut p = Palette::new_with_air();
        p.intern(state("minecraft:oak_stairs", &[("facing", "south")]));
        p.intern(state("minecraft:oak_stairs", &[("facing", "north")]));
        let _ = p.canonicalize();
        let facings: Vec<&str> = p.entries[1..]
            .iter()
            .map(|s| s.properties["facing"].as_str())
            .collect();
        assert_eq!(facings, ["north", "south"]);
    }

    #[test]
    fn canonicalize_reads_properties_in_name_order_not_insertion_order() {
        // `IndexMap` compares as a map, so these two are `==` and `intern`
        // would fold them onto one slot — the palettes below are built by
        // hand for that reason. Their iteration orders differ, so a key
        // that read the pairs in place would order the two palettes
        // differently while they describe the same block.
        let mut one = Palette {
            entries: vec![
                BlockState::air(),
                state(
                    "minecraft:oak_stairs",
                    &[("facing", "north"), ("half", "top")],
                ),
                BlockState::bare("minecraft:oak_planks"),
            ],
        };
        let mut other = Palette {
            entries: vec![
                BlockState::air(),
                state(
                    "minecraft:oak_stairs",
                    &[("half", "top"), ("facing", "north")],
                ),
                BlockState::bare("minecraft:oak_planks"),
            ],
        };
        let _ = one.canonicalize();
        let _ = other.canonicalize();
        assert_eq!(ids(&one), ids(&other));
    }

    #[test]
    fn canonicalizing_a_block_array_renumbers_the_grid_onto_the_new_slots() {
        let mut palette = Palette::new_with_air();
        let planks = palette.intern(BlockState::bare("minecraft:oak_planks"));
        let cobble = palette.intern(BlockState::bare("minecraft:cobblestone"));
        let mut array = BlockArray {
            dims: Dims { x: 2, y: 1, z: 1 },
            palette,
            voxels: vec![planks, cobble],
            block_entities: Vec::new(),
            entities: Vec::new(),
            source_scope: "struct::t".to_owned(),
        };
        array.canonicalize_palette();
        // Same two blocks in the same two cells, read back through the
        // palette the reordering left behind.
        let read: Vec<&str> = array
            .voxels
            .iter()
            .map(|i| array.palette.entries[usize::from(i.0)].id.as_str())
            .collect();
        assert_eq!(read, ["minecraft:oak_planks", "minecraft:cobblestone"]);
        assert_eq!(
            ids(&array.palette),
            [
                "minecraft:air",
                "minecraft:cobblestone",
                "minecraft:oak_planks",
            ],
        );
    }

    #[test]
    fn canonicalizing_leaves_an_index_the_palette_does_not_have_alone() {
        // A hand-assembled disagreement stays visible to
        // `first_index_outside_palette`, which is what both backends ask
        // before they write anything.
        let mut array = BlockArray {
            dims: Dims { x: 1, y: 1, z: 1 },
            palette: Palette::new_with_air(),
            voxels: vec![PaletteIndex(7)],
            block_entities: Vec::new(),
            entities: Vec::new(),
            source_scope: "struct::t".to_owned(),
        };
        array.canonicalize_palette();
        assert_eq!(array.first_index_outside_palette(), Some((7, 1)));
    }
}
