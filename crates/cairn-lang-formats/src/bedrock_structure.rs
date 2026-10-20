//! A [`BlockArray`] as a Bedrock `.mcstructure`. Each `block_indices` layer runs z fastest,
//! `(x * size_y + y) * size_z + z`, unlike Java's `blocks`; the second, waterlogging layer is all
//! `-1`, since lowering authors none.

use std::collections::HashMap;

use cairn_lang_core::block_array::{BlockArray, PALETTE_CAPACITY, PaletteIndex};
use cairn_lang_core::resolve::DroppedIntent;
pub use cairn_lang_nbt::Compound;
use cairn_lang_nbt::tag::{List, Tag};
use cairn_lang_nbt::{
    CompoundStream, NbtIoError, check_string, stream_bedrock_uncompressed,
    write_bedrock_uncompressed,
};
use thiserror::Error;

use crate::bedrock_state::{BedrockStateError, translate_states};
use crate::block_id::is_concrete_id;
use crate::data_version::BedrockTarget;
use crate::dims::{VolumeError, dims_to_i32, list_volume};

/// One intent a palette entry dropped to fit Bedrock, reported as `W_INTENT_DEGRADED`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParityNote {
    /// The entry's block id.
    pub id: String,
    /// The entry's `key=value` pairs, comma-joined, the spelling of
    /// [`DegradedEntry::states`](cairn_lang_core::resolve::DegradedEntry::states).
    pub states: String,
    /// The intent dropped, which [`degradation_detail`](crate::bedrock_state::degradation_detail)
    /// words.
    pub dropped: DroppedIntent,
}

/// Why a [`BlockArray`] cannot be written as a `.mcstructure`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BedrockStructureError {
    /// An NBT writer failure, only I/O once [`prepare_mcstructure`] has passed.
    #[error("nbt: {0}")]
    Nbt(#[from] NbtIoError),
    /// A palette id is not `namespace:identifier`.
    #[error("palette entry `{id}` is abstract; expected `namespace:identifier`")]
    AbstractPaletteEntry {
        /// The id.
        id: String,
    },
    /// A palette entry's properties have no Bedrock `states` mapping.
    #[error(transparent)]
    State(#[from] BedrockStateError),
    /// A voxel names a slot the palette lacks, reachable only through the public fields.
    #[error("voxel palette index {index} is outside the {len}-entry palette")]
    PaletteIndexOutOfRange {
        /// The slot.
        index: u16,
        /// The palette's length.
        len: usize,
    },
    /// A dimension past the `i32` wire width.
    #[error("dimension {axis} = {value} exceeds NBT i32 wire limit")]
    DimensionOverflow {
        /// The axis, `"x"`, `"y"` or `"z"`.
        axis: &'static str,
        /// The dimension.
        value: u32,
    },
    /// The voxel count `x * y * z` overflows `usize`.
    #[error("dimensions {x}x{y}x{z} hold more voxels than this platform can count")]
    VolumeOverflow {
        /// The x extent.
        x: u32,
        /// The y extent.
        y: u32,
        /// The z extent.
        z: u32,
    },
    /// A list longer than an NBT list's `i32` length prefix allows.
    #[error("the `{list}` list would hold {len} entries, past the NBT list limit of 2147483647")]
    ListTooLong {
        /// `"block_indices"`.
        list: &'static str,
        /// Its length.
        len: usize,
    },
    /// The grid's length is not its dims' voxel count, reachable only through the public fields.
    #[error("dimensions hold {volume} voxels but the grid has {voxels}")]
    VoxelCountMismatch {
        /// `x * y * z`.
        volume: usize,
        /// `voxels.len()`.
        voxels: usize,
    },
    /// A palette entry translating to a new Bedrock block once `block_palette` holds
    /// [`PALETTE_CAPACITY`] blocks, reachable only through the public fields.
    #[error(
        "palette entry {index}, `{id}`, translates to a new Bedrock block, but `block_palette` \
         already holds the {capacity} a palette index can name",
        capacity = PALETTE_CAPACITY
    )]
    BlockPaletteFull {
        /// The entry's position in the palette.
        index: usize,
        /// The entry's id.
        id: String,
    },
    /// A palette string the NBT encoder cannot write, checked first since the file writes it last.
    #[error("palette entry `{id}`: {field} cannot be written as NBT: {source}")]
    UnencodablePaletteString {
        /// The entry's id.
        id: String,
        /// Which string: the id, a state name or a state value.
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

/// A [`BlockArray`] checked to write as a `.mcstructure` for its target, its palette built, so
/// writing it can fail only on I/O.
#[derive(Clone)]
pub struct McStructure<'a> {
    block_array: &'a BlockArray,
    size: [i32; 3],
    /// Each `block_indices` layer's length, checked to fit `i32` and to equal `voxels.len()`.
    volume: usize,
    /// The `palette` compound, one `block_palette` entry per distinct translated block.
    palette: Compound,
    block_palette_len: usize,
    /// Each palette entry's `block_palette` slot; entries translation merges share one.
    slots: Vec<u16>,
}

/// Leaves out the grid, which can hold millions of voxels.
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

/// Checks that `block_array` writes as a `.mcstructure` for `target`, translating its palette,
/// with the [`ParityNote`]s the translation raised, in palette order.
///
/// # Errors
///
/// The first fault found, as any [`BedrockStructureError`] but [`BedrockStructureError::Nbt`].
pub fn prepare_mcstructure<'a>(
    block_array: &'a BlockArray,
    target: &BedrockTarget,
) -> Result<(McStructure<'a>, Vec<ParityNote>), BedrockStructureError> {
    let entries = &block_array.palette.entries;
    let mut block_palette: Vec<Compound> = Vec::with_capacity(entries.len());
    // Keyed by name: blocks of two names never merge, and translation gives one name few states.
    let mut slots_by_name: HashMap<&str, Vec<u16>> = HashMap::new();
    let mut slots: Vec<u16> = Vec::with_capacity(entries.len());
    let mut notes: Vec<ParityNote> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if !is_concrete_id(&entry.id) {
            return Err(BedrockStructureError::AbstractPaletteEntry {
                id: entry.id.clone(),
            });
        }
        let translated = translate_states(entry)?;
        check_palette_strings(&entry.id, &translated.states)?;
        // Named by the Java entry: several can share one Bedrock state once the intent drops.
        if !translated.degraded.is_empty() {
            let states = entry.properties_text();
            notes.extend(translated.degraded.into_iter().map(|dropped| ParityNote {
                id: entry.id.clone(),
                states: states.clone(),
                dropped,
            }));
        }
        let compound = palette_entry(&entry.id, translated.states, target.block_version);
        // The palette is a set (`spec/compilation` "Within-phase conflicts and the palette"):
        // an entry whose translation folds into an earlier one takes that slot, so air stays at 0.
        let named = slots_by_name.entry(&entry.id).or_default();
        let earlier = named
            .iter()
            .copied()
            .find(|&slot| block_palette[usize::from(slot)] == compound);
        let slot = if let Some(slot) = earlier {
            slot
        } else {
            let slot = u16::try_from(block_palette.len()).map_err(|_| {
                BedrockStructureError::BlockPaletteFull {
                    index,
                    id: entry.id.clone(),
                }
            })?;
            block_palette.push(compound);
            named.push(slot);
            slot
        };
        slots.push(slot);
    }
    let block_palette_len = block_palette.len();
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

/// Refuses the first string of a palette entry the NBT encoder could not write.
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
    fn slot(&self, voxel: PaletteIndex) -> i32 {
        i32::from(self.slots[usize::from(voxel.0)])
    }

    /// Writes the structure as uncompressed little-endian NBT, encoding the voxels as it goes.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::Io`] only, since [`prepare_mcstructure`] ruled out every encoding error.
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

    /// The two `block_indices` layers, in [`Self::block_indices`]'s order and shape.
    fn write_block_indices<W: std::io::Write>(
        &self,
        structure: &mut CompoundStream<'_, W>,
    ) -> Result<(), NbtIoError> {
        let block_array = self.block_array;
        let volume = self.volume;
        structure.list("block_indices", 9, 2, |layers| {
            layers.list(3, volume, |layer0| {
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

    /// The two `block_indices` layers: each voxel's `block_palette` slot, then all `-1`.
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
        // An empty layer must declare `TAG_End`, as `List::of_tags` makes it.
        List {
            element_type_id: 9,
            items: vec![
                Tag::List(List::of_tags(3, layer0)),
                Tag::List(List::of_tags(3, layer1)),
            ],
        }
    }
}

/// The root [`Compound`] of a `.mcstructure`, with its [`ParityNote`]s; to write a large one, use
/// [`McStructure::write`].
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

/// Writes a `.mcstructure` root as uncompressed little-endian NBT under the empty root name.
///
/// # Errors
///
/// An I/O failure, or a tag the encoder rejects.
pub fn write_mcstructure<W: std::io::Write>(
    writer: &mut W,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_bedrock_uncompressed(writer, "", root)
}

/// One `block_palette` entry; a bare block still carries the empty `states` the game expects.
fn palette_entry(id: &str, states: Compound, block_version: i32) -> Compound {
    let mut compound = Compound::new();
    compound.insert("name", Tag::String(id.to_owned()));
    compound.insert("states", Tag::Compound(states));
    compound.insert("version", Tag::Int(block_version));
    compound
}

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
