//! `BlockArray` → Bedrock `.mcstructure` NBT tag tree.
//!
//! The `.mcstructure` file is uncompressed little-endian NBT with an
//! unnamed root [`Compound`] of:
//!
//! ```text
//! format_version:         Int (always 1)
//! size:                   List<Int>[3]                     (x, y, z)
//! structure:              Compound
//!   block_indices:        List<List<Int>>[2]               (block layer, waterlog layer)
//!   entities:             List<empty>
//!   palette:              Compound
//!     default:            Compound
//!       block_palette:    List<Compound>  (each: { name, states, version })
//!       block_position_data: Compound
//! structure_world_origin: List<Int>[3]
//! ```
//!
//! Layout reference: wiki.bedrock.dev's "mcstructure" page. Each
//! `block_indices` layer is a flat array in `(x, y, z)` nesting with **z
//! fastest** — `index = (x * size_y + y) * size_z + z` — which differs
//! from the `(y, z, x)` order the Java `blocks` list uses. The second
//! layer carries co-located blocks (waterlogging); Cairn's lowering never
//! authors those today, so it is `-1`-filled ("no block here").
//!
//! Blockstate properties are translated per edition by
//! [`crate::bedrock_state`]: the stair family's `facing` / `half` become
//! Bedrock's `weirdo_direction` / `upside_down_bit`, and intent Bedrock
//! cannot express (stair `shape`) is dropped with a degradation note rather
//! than silently, per `spec/versioning-editions` "Backend = data tables",
//! "Fail-loud and minimum-version inference" and "Java / Bedrock
//! portability". A block with properties outside a mapped family is still a
//! hard error.

use cairn_lang_core::block_array::{BlockArray, PaletteIndex};
pub use cairn_lang_nbt::Compound;
use cairn_lang_nbt::tag::{List, Tag};
use cairn_lang_nbt::{
    CompoundStream, NbtIoError, check_string, stream_bedrock_uncompressed,
    write_bedrock_uncompressed,
};
use thiserror::Error;

use crate::bedrock_state::{BedrockStateError, degradation_message, java_state, translate_states};
use crate::data_version::BedrockTarget;
use crate::dims::{VolumeError, dims_to_i32, fits_list_limit, list_volume};
use crate::java_structure::is_concrete_id;

/// A palette entry whose intent was degraded to fit Bedrock — surfaced by the
/// caller as `W_INTENT_DEGRADED`. The serialiser has no source span (a
/// [`cairn_lang_core::block_array::BlockState`] carries none), so the note is
/// keyed by the palette entry's id and left for the CLI to attribute to the
/// enclosing structure scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityNote {
    /// Concrete id of the palette entry whose intent degraded.
    pub id: String,
    /// Human-readable degradation message. It already names the entry, as
    /// its Java state `id[key=value,...]`, so a caller that prefixes
    /// [`Self::id`] to it prints the id twice.
    pub message: String,
}

/// Errors raised while serialising a [`BlockArray`] to a `.mcstructure`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BedrockStructureError {
    /// Forwarded failure from the NBT writer. [`prepare_mcstructure`]
    /// rules out every encoding error, so from a checked structure's write
    /// this is I/O.
    #[error("nbt: {0}")]
    Nbt(#[from] NbtIoError),
    /// A palette entry's `id` lacks a `namespace:identifier` form. Same
    /// contract as the Java backend: abstract material tokens must be
    /// resolved before reaching a serialiser.
    #[error("palette entry `{id}` is abstract; expected `namespace:identifier`")]
    AbstractPaletteEntry {
        /// Offending id verbatim.
        id: String,
    },
    /// A palette entry's blockstate properties could not be translated to
    /// Bedrock's `states` vocabulary (an unmapped block family, or a value
    /// outside the Java domain). The forwarded error carries the
    /// self-correction triple so the lint loop can act on it.
    #[error(transparent)]
    State(#[from] BedrockStateError),
    /// A voxel named a palette slot the palette does not have. Same hole
    /// as the Java backend's, reached the same way: public struct fields
    /// plus a public builder.
    #[error("voxel palette index {index} is outside the {len}-entry palette")]
    PaletteIndexOutOfRange {
        /// The index the grid asked for.
        index: u16,
        /// How many entries the palette actually has.
        len: usize,
    },
    /// A voxel dimension overflowed the `i32` wire width NBT uses.
    #[error("dimension {axis} = {value} exceeds NBT i32 wire limit")]
    DimensionOverflow {
        /// Which axis overflowed (`"x"` / `"y"` / `"z"`).
        axis: &'static str,
        /// Offending dimension value.
        value: u32,
    },
    /// The dims' voxel count `x * y * z` overflows `usize`. Same check as
    /// the Java backend's.
    #[error("dimensions {x}x{y}x{z} hold more voxels than this platform can count")]
    VolumeOverflow {
        /// Extent along x.
        x: u32,
        /// Extent along y.
        y: u32,
        /// Extent along z.
        z: u32,
    },
    /// A list the file holds would have more entries than the `i32` length
    /// prefix of an NBT list can declare.
    #[error("the `{list}` list would hold {len} entries, past the NBT list limit of 2147483647")]
    ListTooLong {
        /// Which list (`"block_indices"` for each layer, `"block_palette"`).
        list: &'static str,
        /// How many entries it would hold.
        len: usize,
    },
    /// The grid holds a different number of voxels than its dims describe.
    /// Same hole as [`Self::PaletteIndexOutOfRange`], reached the same way.
    #[error("dimensions hold {volume} voxels but the grid has {voxels}")]
    VoxelCountMismatch {
        /// `x * y * z`.
        volume: usize,
        /// `voxels.len()`.
        voxels: usize,
    },
    /// A palette string the NBT encoder cannot write: a NUL or non-ASCII
    /// byte, or more bytes than the `u16` length prefix carries. Checked up
    /// front because `.mcstructure` writes its palette after both per-voxel
    /// layers, so the writer would only find it once the whole volume had
    /// been encoded, and could only report the byte.
    #[error("palette entry `{id}`: {field} cannot be written as NBT: {source}")]
    UnencodablePaletteString {
        /// The entry's id verbatim.
        id: String,
        /// Which string of the entry: its id, a state name or a state
        /// value.
        field: String,
        /// The encoder's refusal.
        source: NbtIoError,
    },
}

impl From<crate::dims::DimensionOverflow> for BedrockStructureError {
    fn from(
        crate::dims::DimensionOverflow { axis, value }: crate::dims::DimensionOverflow,
    ) -> Self {
        Self::DimensionOverflow { axis, value }
    }
}

impl From<VolumeError> for BedrockStructureError {
    fn from(err: VolumeError) -> Self {
        match err {
            VolumeError::Overflow { x, y, z } => Self::VolumeOverflow { x, y, z },
            VolumeError::PastListLimit { volume } => Self::ListTooLong {
                list: "block_indices",
                len: volume,
            },
            VolumeError::Mismatch { volume, voxels } => Self::VoxelCountMismatch { volume, voxels },
        }
    }
}

/// A [`BlockArray`] checked to serialise as a `.mcstructure`, with its
/// `palette` compound already built for the target it is written for.
///
/// Holding one means every refusal [`build_mcstructure_tag`] can raise has
/// already been ruled out, and so has every refusal the NBT encoder could
/// raise on this structure: each palette string is encodable, the voxel
/// count fits a list's length prefix and is the grid's length, and
/// `block_palette`, measured once translated duplicates are merged, fits
/// one too. [`Self::write`] can therefore fail only on I/O, and has no
/// panic of its own to reach. The two per-voxel `block_indices` layers are
/// encoded from the grid as they are written, each voxel through `slots`
/// to the `block_palette` slot its palette entry was written to.
#[derive(Clone)]
pub struct McStructure<'a> {
    block_array: &'a BlockArray,
    size: [i32; 3],
    /// Each `block_indices` layer's declared length: the dims' voxel count,
    /// checked to fit `i32` and to equal `block_array.voxels.len()`.
    volume: usize,
    /// The finished `palette` compound: one `block_palette` entry per
    /// distinct translated block, in the order each first appears in the
    /// palette, with its translated `states`. Built once here so writing it
    /// needs no copy.
    palette: Compound,
    /// How many entries `block_palette` holds: the palette's, less those
    /// translation merged into an earlier one.
    block_palette_len: usize,
    /// The `block_palette` slot each palette entry was written to, indexed
    /// by the entry's [`PaletteIndex`].
    ///
    /// Translation can map several palette entries to one Bedrock block —
    /// stairs differing only in `shape` do, once `shape` is dropped — and
    /// those share the first one's slot.
    slots: Vec<u16>,
}

/// The dims and palette size rather than the grid, which can hold millions
/// of voxels.
impl std::fmt::Debug for McStructure<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McStructure")
            .field("source_scope", &self.block_array.source_scope)
            .field("size", &self.size)
            .field("volume", &self.volume)
            .field("palette_len", &self.block_array.palette.entries.len())
            .field("block_palette_len", &self.block_palette_len)
            .finish_non_exhaustive()
    }
}

/// Check that `block_array` serialises as a `.mcstructure` for `target`,
/// translating each palette entry's blockstate properties to Bedrock's
/// `states` vocabulary, alongside any [`ParityNote`]s the translation
/// raised. Nothing per voxel is built.
///
/// The palette is checked before the grid, as on the Java side: its checks
/// cost one pass over the palette, the grid's a pass over every voxel.
///
/// # Errors
///
/// Returns [`BedrockStructureError::AbstractPaletteEntry`] for an
/// unresolved abstract token, [`BedrockStructureError::State`] for a palette
/// entry whose properties cannot be mapped to Bedrock (see
/// [`crate::bedrock_state`]),
/// [`BedrockStructureError::UnencodablePaletteString`] when an entry's id
/// or a translated state's name or string value cannot be written as an
/// NBT string, [`BedrockStructureError::DimensionOverflow`] when a
/// dimension does not fit the wire width,
/// [`BedrockStructureError::VolumeOverflow`] when the voxel count overflows
/// `usize`, [`BedrockStructureError::ListTooLong`] when the voxel count is
/// past an NBT list's length limit or `block_palette` is (measured, and
/// reported as `len`, once translated duplicates are merged),
/// [`BedrockStructureError::VoxelCountMismatch`] when the grid's length is
/// not the voxel count, and
/// [`BedrockStructureError::PaletteIndexOutOfRange`] when a voxel names a
/// slot the palette does not have.
///
/// # Panics
///
/// Panics if `block_palette` would need a slot past `u16::MAX`. A slot is
/// never past its entry's palette index, so that takes a palette longer
/// than [`Palette::intern`](cairn_lang_core::block_array::Palette::intern)
/// builds: one assembled by hand.
pub fn prepare_mcstructure<'a>(
    block_array: &'a BlockArray,
    target: &BedrockTarget,
) -> Result<(McStructure<'a>, Vec<ParityNote>), BedrockStructureError> {
    // Translate every palette entry up front: a mapping failure (abstract
    // token, unmapped stateful block, unencodable string) aborts before any
    // `McStructure` is handed back, so no caller can reach the write path
    // with a half-translated palette.
    let entries = &block_array.palette.entries;
    let mut block_palette: Vec<Compound> = Vec::with_capacity(entries.len());
    let mut slots: Vec<u16> = Vec::with_capacity(entries.len());
    let mut notes: Vec<ParityNote> = Vec::new();
    for entry in entries {
        if !is_concrete_id(&entry.id) {
            return Err(BedrockStructureError::AbstractPaletteEntry {
                id: entry.id.clone(),
            });
        }
        let translated = translate_states(&entry.id, &entry.properties)?;
        check_palette_strings(&entry.id, &translated.states)?;
        // Only an entry with properties degrades, so `state` is never a
        // bare block's. It is named by the Java state it came from, as
        // `cairn info` names it: the Bedrock state is what remains once
        // the intent is dropped, so two entries that differ only in what
        // was dropped would read the same by it.
        if !translated.degraded.is_empty() {
            let state = java_state(&entry.id, &entry.properties);
            for dropped in &translated.degraded {
                notes.push(ParityNote {
                    id: entry.id.clone(),
                    message: degradation_message(&state, dropped),
                });
            }
        }
        let compound = palette_entry(&entry.id, translated.states, target.block_version);
        // The palette is a set (`spec/compilation` "Within-phase conflicts
        // and the palette"), and translation can fold distinct Java states
        // into one Bedrock block. The first entry keeps its slot and the
        // rest are written to it, so air stays at 0. A linear scan, as in
        // `Palette::intern`: `Compound` can hold a float, so it has no
        // `Hash`, and these palettes are small.
        let slot = block_palette
            .iter()
            .position(|written| *written == compound)
            .unwrap_or_else(|| {
                block_palette.push(compound);
                block_palette.len() - 1
            });
        // A slot is never past its entry's palette index, so it fits a
        // `u16` for every palette `Palette::intern` builds. Only a longer
        // one, assembled by hand, reaches the `expect`, which panics as
        // `intern` does rather than write a slot no voxel could name.
        slots.push(
            u16::try_from(slot)
                .expect("block_palette grew past u16::MAX slots; widen PaletteIndex first"),
        );
    }
    let block_palette_len = block_palette.len();
    if !fits_list_limit(block_palette_len) {
        return Err(BedrockStructureError::ListTooLong {
            list: "block_palette",
            len: block_palette_len,
        });
    }
    let size = dims_to_i32(&block_array.dims)?;
    let volume = list_volume(block_array)?;
    if let Some((index, len)) = block_array.first_index_outside_palette() {
        return Err(BedrockStructureError::PaletteIndexOutOfRange { index, len });
    }

    let prepared = McStructure {
        block_array,
        size,
        volume,
        palette: palette_compound(block_palette),
        block_palette_len,
        slots,
    };
    Ok((prepared, notes))
}

/// Refuse the first string of a palette entry that the `palette` compound
/// would carry and the NBT encoder could not: the id, then each state's
/// name and, for a string state, its value.
fn check_palette_strings(id: &str, states: &Compound) -> Result<(), BedrockStructureError> {
    let refuse =
        |field: String, source: NbtIoError| BedrockStructureError::UnencodablePaletteString {
            id: id.to_owned(),
            field,
            source,
        };
    check_string(id).map_err(|source| refuse("its id".to_owned(), source))?;
    for (key, value) in &states.entries {
        check_string(key).map_err(|source| refuse(format!("state name `{key}`"), source))?;
        if let Tag::String(value) = value {
            check_string(value)
                .map_err(|source| refuse(format!("the value of state `{key}`"), source))?;
        }
    }
    Ok(())
}

impl McStructure<'_> {
    /// The `block_palette` slot a voxel is written as.
    ///
    /// The indexing cannot panic only because `slots` has one entry per
    /// palette entry and [`prepare_mcstructure`] runs
    /// `first_index_outside_palette` before it builds a `McStructure`.
    fn slot(&self, voxel: PaletteIndex) -> i32 {
        i32::from(self.slots[usize::from(voxel.0)])
    }

    /// Write the structure under the empty root name the game expects, as
    /// raw (uncompressed) little-endian NBT. The bytes are the ones
    /// [`write_mcstructure`] writes for the tree [`build_mcstructure_tag`]
    /// builds, but nothing per voxel is held in memory.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::Io`] for I/O failure on `writer`.
    /// [`prepare_mcstructure`] has ruled out every encoding error.
    pub fn write<W: std::io::Write>(&self, writer: &mut W) -> Result<(), NbtIoError> {
        stream_bedrock_uncompressed(writer, "", |root| {
            root.tag("format_version", &Tag::Int(1))?;
            root.tag("size", &Tag::List(List::of_ints(self.size)))?;
            root.compound("structure", |structure| {
                self.write_block_indices(structure)?;
                structure.tag("entities", &Tag::List(List::empty()))?;
                structure.compound_tag("palette", &self.palette)
            })?;
            root.tag(
                "structure_world_origin",
                &Tag::List(List::of_ints([0, 0, 0])),
            )
        })
    }

    /// The two `block_indices` layers, streamed in [`Self::block_indices`]'s
    /// order and shape.
    fn write_block_indices<W: std::io::Write>(
        &self,
        structure: &mut CompoundStream<'_, W>,
    ) -> Result<(), NbtIoError> {
        let block_array = self.block_array;
        let volume = self.volume;
        structure.list("block_indices", 9, 2, |layers| {
            layers.list(3, volume, |layer0| {
                // `prepare_mcstructure` checked `voxels` holds `volume`
                // entries, and `Dims::index` is below that.
                for x in 0..block_array.dims.x {
                    for y in 0..block_array.dims.y {
                        for z in 0..block_array.dims.z {
                            let i = block_array
                                .dims
                                .index(x, y, z)
                                .expect("voxel coordinate in dims by construction");
                            layer0.item(&Tag::Int(self.slot(block_array.voxels[i])))?;
                        }
                    }
                }
                Ok(())
            })?;
            layers.list(3, volume, |layer1| {
                for _ in 0..volume {
                    layer1.item(&Tag::Int(-1))?;
                }
                Ok(())
            })
        })
    }

    /// The two `block_indices` layers. Layer 0 is the `block_palette` slot
    /// per voxel, which is not its palette index once translation has
    /// merged entries; layer 1 is the co-located (waterlog) layer,
    /// `-1`-filled because Cairn's lowering never authors co-located blocks
    /// today.
    fn block_indices(&self) -> List {
        let block_array = self.block_array;
        let volume = self.volume;
        let mut layer0: Vec<Tag> = Vec::with_capacity(volume);
        for x in 0..block_array.dims.x {
            for y in 0..block_array.dims.y {
                for z in 0..block_array.dims.z {
                    let i = block_array
                        .dims
                        .index(x, y, z)
                        .expect("voxel coordinate in dims by construction");
                    layer0.push(Tag::Int(self.slot(block_array.voxels[i])));
                }
            }
        }
        let layer1: Vec<Tag> = vec![Tag::Int(-1); volume];
        // The outer list always has two items, so a literal is safe; each
        // layer goes through `List::of_tags` because an empty one must
        // declare `TAG_End` rather than `3`.
        List {
            element_type_id: 9,
            items: vec![
                Tag::List(List::of_tags(3, layer0)),
                Tag::List(List::of_tags(3, layer1)),
            ],
        }
    }
}

/// Build the unnamed root [`Compound`] for a `.mcstructure` file from a
/// lowered [`BlockArray`], alongside any [`ParityNote`]s raised while
/// translating blockstate properties to Bedrock's `states` vocabulary.
///
/// The tree holds both per-voxel `block_indices` layers as tags, so it is
/// for inspecting a structure rather than for writing a large one:
/// [`McStructure::write`] writes the same bytes without it.
///
/// # Errors
///
/// Every refusal of [`prepare_mcstructure`].
pub fn build_mcstructure_tag(
    block_array: &BlockArray,
    target: &BedrockTarget,
) -> Result<(Compound, Vec<ParityNote>), BedrockStructureError> {
    let (prepared, notes) = prepare_mcstructure(block_array, target)?;

    let mut structure = Compound::new();
    structure.insert("block_indices", Tag::List(prepared.block_indices()));
    structure.insert("entities", Tag::List(List::empty()));
    structure.insert("palette", Tag::Compound(prepared.palette));

    let mut root = Compound::new();
    root.insert("format_version", Tag::Int(1));
    root.insert("size", Tag::List(List::of_ints(prepared.size)));
    root.insert("structure", Tag::Compound(structure));
    root.insert(
        "structure_world_origin",
        Tag::List(List::of_ints([0, 0, 0])),
    );
    Ok((root, notes))
}

/// Write an already-built `.mcstructure` root under the empty root name
/// the game expects, as raw (uncompressed) little-endian NBT, for a caller
/// holding a tree it built or edited. [`McStructure::write`] writes a
/// structure without building one.
///
/// # Errors
///
/// Propagates I/O failure from `writer` and any encoding error raised by
/// the tag tree.
pub fn write_mcstructure<W: std::io::Write>(
    writer: &mut W,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_bedrock_uncompressed(writer, "", root)
}

/// One `block_palette` entry: the id, its Bedrock `states` (an empty
/// compound for a bare block, since the game still expects the key), and
/// the target's block version.
fn palette_entry(id: &str, states: Compound, block_version: i32) -> Compound {
    let mut compound = Compound::new();
    compound.insert("name", Tag::String(id.to_owned()));
    compound.insert("states", Tag::Compound(states));
    compound.insert("version", Tag::Int(block_version));
    compound
}

/// The `palette` compound around the `block_palette` entries.
fn palette_compound(block_palette: Vec<Compound>) -> Compound {
    let mut default = Compound::new();
    default.insert(
        "block_palette",
        Tag::List(List::of_compounds(block_palette)),
    );
    default.insert("block_position_data", Tag::Compound(Compound::new()));

    let mut palette = Compound::new();
    palette.insert("default", Tag::Compound(default));
    palette
}
