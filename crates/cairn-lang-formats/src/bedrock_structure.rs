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

use cairn_lang_core::block_array::BlockArray;
pub use cairn_lang_nbt::Compound;
use cairn_lang_nbt::tag::{List, Tag};
use cairn_lang_nbt::{
    CompoundStream, NbtIoError, stream_bedrock_uncompressed, write_bedrock_uncompressed,
};
use thiserror::Error;

use crate::bedrock_state::{BedrockStateError, degradation_message, translate_states};
use crate::data_version::BedrockTarget;
use crate::dims::dims_to_i32;
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
    /// Human-readable degradation message.
    pub message: String,
}

/// Errors raised while serialising a [`BlockArray`] to a `.mcstructure`.
#[derive(Debug, Error)]
pub enum BedrockStructureError {
    /// Forwarded I/O / encoding failure from the NBT writer.
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
}

impl From<crate::dims::DimensionOverflow> for BedrockStructureError {
    fn from(
        crate::dims::DimensionOverflow { axis, value }: crate::dims::DimensionOverflow,
    ) -> Self {
        Self::DimensionOverflow { axis, value }
    }
}

/// A [`BlockArray`] checked to serialise as a `.mcstructure`, with its
/// palette already translated to Bedrock `states` for the target it is
/// written for.
///
/// Holding one means every refusal [`build_mcstructure_tag`] can raise has
/// already been ruled out, so [`Self::write`] can only fail on I/O or on a
/// string the NBT encoder refuses. The two per-voxel `block_indices` layers
/// are encoded straight from the grid as they are written.
#[derive(Debug, Clone)]
pub struct McStructure<'a> {
    block_array: &'a BlockArray,
    size: [i32; 3],
    block_version: i32,
    /// One translated `states` compound per palette entry, in palette order.
    palette_states: Vec<Compound>,
}

/// Check that `block_array` serialises as a `.mcstructure` for `target`,
/// translating each palette entry's blockstate properties to Bedrock's
/// `states` vocabulary, alongside any [`ParityNote`]s the translation
/// raised. Nothing per voxel is built.
///
/// # Errors
///
/// Returns [`BedrockStructureError::AbstractPaletteEntry`] for an
/// unresolved abstract token, [`BedrockStructureError::State`] for a palette
/// entry whose properties cannot be mapped to Bedrock (see
/// [`crate::bedrock_state`]),
/// [`BedrockStructureError::PaletteIndexOutOfRange`] when a voxel names a
/// slot the palette does not have, and
/// [`BedrockStructureError::DimensionOverflow`] when a dimension does not
/// fit the wire width.
pub fn prepare_mcstructure<'a>(
    block_array: &'a BlockArray,
    target: &BedrockTarget,
) -> Result<(McStructure<'a>, Vec<ParityNote>), BedrockStructureError> {
    // Ahead of the translation loop, as on the Java side: the check does
    // not depend on anything the loop produces, and a rejected array should
    // cost nothing beyond the walk.
    if let Some((index, len)) = block_array.first_index_outside_palette() {
        return Err(BedrockStructureError::PaletteIndexOutOfRange { index, len });
    }

    // Translate every palette entry up front: a mapping failure (abstract
    // token, unmapped stateful block) aborts the whole build before
    // anything is written, mirroring the Java backend's fail-loud contract.
    let mut palette_states: Vec<Compound> = Vec::with_capacity(block_array.palette.entries.len());
    let mut notes: Vec<ParityNote> = Vec::new();
    for entry in &block_array.palette.entries {
        if !is_concrete_id(&entry.id) {
            return Err(BedrockStructureError::AbstractPaletteEntry {
                id: entry.id.clone(),
            });
        }
        let translated = translate_states(&entry.id, &entry.properties)?;
        for dropped in &translated.degraded {
            notes.push(ParityNote {
                id: entry.id.clone(),
                message: degradation_message(&entry.id, dropped),
            });
        }
        palette_states.push(translated.states);
    }

    let prepared = McStructure {
        block_array,
        size: dims_to_i32(&block_array.dims)?,
        block_version: target.block_version,
        palette_states,
    };
    Ok((prepared, notes))
}

impl McStructure<'_> {
    /// Write the structure under the empty root name the game expects, as
    /// raw (uncompressed) little-endian NBT. The bytes are the ones
    /// [`write_mcstructure`] writes for the tree [`build_mcstructure_tag`]
    /// builds, but nothing per voxel is held in memory.
    ///
    /// # Errors
    ///
    /// Propagates I/O failure from `writer` and any encoding error the NBT
    /// writer raises (a palette string it cannot carry, a list past the
    /// wire length).
    pub fn write<W: std::io::Write>(&self, writer: &mut W) -> Result<(), NbtIoError> {
        stream_bedrock_uncompressed(writer, "", |root| {
            root.tag("format_version", &Tag::Int(1))?;
            root.tag("size", &Tag::List(List::of_ints(self.size)))?;
            root.compound("structure", |structure| {
                self.write_block_indices(structure)?;
                structure.tag("entities", &Tag::List(List::empty()))?;
                structure.tag("palette", &Tag::Compound(self.palette_compound()))
            })?;
            root.tag(
                "structure_world_origin",
                &Tag::List(List::of_ints([0, 0, 0])),
            )
        })
    }

    /// The two `block_indices` layers, streamed in [`block_indices`]'s
    /// order and shape.
    fn write_block_indices<W: std::io::Write>(
        &self,
        structure: &mut CompoundStream<'_, W>,
    ) -> Result<(), NbtIoError> {
        let block_array = self.block_array;
        let volume = block_array.dims.volume();
        // An empty layer declares `TAG_End`, as `List::of_tags` does.
        let element_type_id = if volume == 0 { 0 } else { 3 };
        structure.list("block_indices", 9, 2, |layers| {
            layers.list(element_type_id, volume, |layer0| {
                for x in 0..block_array.dims.x {
                    for y in 0..block_array.dims.y {
                        for z in 0..block_array.dims.z {
                            let i = block_array
                                .dims
                                .index(x, y, z)
                                .expect("voxel coordinate in dims by construction");
                            layer0.item(&Tag::Int(i32::from(block_array.voxels[i].0)))?;
                        }
                    }
                }
                Ok(())
            })?;
            layers.list(element_type_id, volume, |layer1| {
                for _ in 0..volume {
                    layer1.item(&Tag::Int(-1))?;
                }
                Ok(())
            })
        })
    }

    /// The `palette` compound for this structure's translated states.
    fn palette_compound(&self) -> Compound {
        palette_compound(
            self.block_array,
            self.block_version,
            self.palette_states.clone(),
        )
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
    structure.insert("block_indices", Tag::List(block_indices(block_array)));
    structure.insert("entities", Tag::List(List::empty()));
    structure.insert("palette", Tag::Compound(prepared.palette_compound()));

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

/// The two `block_indices` layers. Layer 0 is the palette index per
/// voxel; layer 1 is the co-located (waterlog) layer, `-1`-filled because
/// Cairn's lowering never authors co-located blocks today.
fn block_indices(block_array: &BlockArray) -> List {
    let volume = block_array.dims.volume();
    let mut layer0: Vec<Tag> = Vec::with_capacity(volume);
    for x in 0..block_array.dims.x {
        for y in 0..block_array.dims.y {
            for z in 0..block_array.dims.z {
                let i = block_array
                    .dims
                    .index(x, y, z)
                    .expect("voxel coordinate in dims by construction");
                layer0.push(Tag::Int(i32::from(block_array.voxels[i].0)));
            }
        }
    }
    let layer1: Vec<Tag> = vec![Tag::Int(-1); volume];
    // The outer list always has two items, so a literal is safe; each layer
    // goes through `List::of_tags` because an empty one must declare
    // `TAG_End` rather than `3`.
    List {
        element_type_id: 9,
        items: vec![
            Tag::List(List::of_tags(3, layer0)),
            Tag::List(List::of_tags(3, layer1)),
        ],
    }
}

fn palette_compound(
    block_array: &BlockArray,
    block_version: i32,
    palette_states: Vec<Compound>,
) -> Compound {
    let entries: Vec<Compound> = block_array
        .palette
        .entries
        .iter()
        .zip(palette_states)
        .map(|(state, states)| {
            let mut compound = Compound::new();
            compound.insert("name", Tag::String(state.id.clone()));
            // Bedrock `states` translated from the Java properties by
            // `prepare_mcstructure`; an empty compound for a bare block
            // (the game still expects the key).
            compound.insert("states", Tag::Compound(states));
            compound.insert("version", Tag::Int(block_version));
            compound
        })
        .collect();

    let mut default = Compound::new();
    default.insert("block_palette", Tag::List(List::of_compounds(entries)));
    default.insert("block_position_data", Tag::Compound(Compound::new()));

    let mut palette = Compound::new();
    palette.insert("default", Tag::Compound(default));
    palette
}
