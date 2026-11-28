//! A [`BlockArrayIr`]'s structures as Sponge Schematic version 3 files: one for each struct,
//! placement and walkway, each offset by where it sits in its site.

use std::collections::HashMap;
use std::io::Write;

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, BlockState};
use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{CompoundStream, NbtIoError, check_java_string, stream_java_uncompressed};
use thiserror::Error;

use super::STATE_SEPARATORS;
use crate::data_version::JavaTarget;
use crate::java_structure::{JavaStructureError, placed_block_entities, prepare_structure};
use crate::nbt_text::excerpt;
use crate::site_frame::site_origin;

/// The Sponge Schematic version written.
const VERSION: i32 = 3;

/// What one `.schem` file holds: a block array and the offset it is pasted at.
#[derive(Clone)]
pub struct SchemFile<'a> {
    /// The [`BlockArrayIr::structures`] key the file is written from.
    pub scope: &'a str,
    /// The file name without its extension, the `.nbt` file's, which `Metadata.Name` repeats.
    pub stem: String,
    /// `Offset`: where the array's minimum corner sits in its site, or the origin for a struct.
    pub offset: (i32, i32, i32),
    /// The voxels.
    pub block_array: &'a BlockArray,
}

/// Leaves out the grid, which can hold millions of voxels.
impl std::fmt::Debug for SchemFile<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemFile")
            .field("scope", &self.scope)
            .field("stem", &self.stem)
            .field("offset", &self.offset)
            .field("dims", &self.block_array.dims)
            .finish_non_exhaustive()
    }
}

/// The `.schem` files `ir` writes to, one per structure, in the IR's order.
#[must_use]
pub fn schem_files(ir: &BlockArrayIr) -> Vec<SchemFile<'_>> {
    ir.structures
        .iter()
        .map(|(scope, block_array)| SchemFile {
            scope,
            stem: ir.artifact_stem(scope),
            offset: site_origin(ir, scope).map_or((0, 0, 0), |(_, origin)| origin),
            block_array,
        })
        .collect()
}

/// Why a [`SchemFile`] cannot be written.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SchemError {
    /// The array fails a check the `.nbt` writer applies to it too.
    #[error(transparent)]
    Structure(#[from] JavaStructureError),
    /// An extent past the unsigned short it is stored as.
    #[error("the array is {value} blocks along {axis}, past the 65535 `{field}` holds")]
    DimensionOverflow {
        /// `"x"`, `"y"` or `"z"`.
        axis: &'static str,
        /// `"Width"`, `"Height"` or `"Length"`.
        field: &'static str,
        /// The extent.
        value: u32,
    },
    /// A palette entry whose spelling would not read back as the same block state.
    #[error(
        "palette entry `{state}` cannot be spelled as a Sponge block state: its id holds `[`, or a \
         property name is empty or a name or value holds `[`, `]`, `,` or `=`"
    )]
    UnspellableState {
        /// The entry, as `id[name=value,...]`.
        state: String,
    },
    /// Two palette entries spelled alike, which one `Palette` key cannot map to both, reachable
    /// only through the public [`Palette::entries`](cairn_lang_core::block_array::Palette::entries).
    #[error("palette entries {first} and {second} are both `{state}`")]
    DuplicateState {
        /// The spelling.
        state: String,
        /// The first entry's index.
        first: usize,
        /// The second entry's index.
        second: usize,
    },
    /// `Metadata.Name` is not a string the NBT encoder writes.
    #[error(
        "the name `{}` cannot be written as NBT: {source}. Fix: shorten the name of the struct, \
         site, placement or walkway it comes from",
        excerpt(name)
    )]
    UnencodableName {
        /// The name.
        name: String,
        /// The encoder's refusal.
        source: NbtIoError,
    },
    /// A palette entry whose spelling as a `Palette` key is not a string the NBT encoder writes.
    #[error(
        "palette entry `{}` cannot be written as NBT: {source}",
        excerpt(state)
    )]
    UnencodableState {
        /// The entry, as its `Palette` key spells it.
        state: String,
        /// The encoder's refusal.
        source: NbtIoError,
    },
    /// Varint block data longer than an NBT byte array holds.
    #[error("`Blocks.Data` would hold {len} bytes, past the {limit} an NBT byte array holds")]
    DataTooLong {
        /// The bytes the data takes.
        len: usize,
        /// The most an NBT byte array holds.
        limit: usize,
    },
    /// A block entity with no string `id` to write as its `Id`: a `raw_block` `nbt=` compound
    /// that writes none, or writes one of another type.
    #[error("the block entity at {pos:?} has no string `id`, which a `.schem` file names it by")]
    BlockEntityWithoutId {
        /// The block entity's voxel.
        pos: (u32, u32, u32),
    },
}

/// A [`SchemFile`] checked to write for its target, so writing it can fail only on I/O.
#[derive(Debug)]
pub struct Schem<'a> {
    file: SchemFile<'a>,
    data_version: i32,
    size: [u16; 3],
    palette: Vec<String>,
    data_len: usize,
}

/// Checks that `file` writes as a `.schem` for `target`, without building its tag tree.
///
/// # Errors
///
/// The first fault found.
pub fn prepare_schem<'a>(
    file: SchemFile<'a>,
    target: &JavaTarget,
) -> Result<Schem<'a>, SchemError> {
    let block_array = file.block_array;
    let dims = block_array.dims;
    let mut size = [0; 3];
    for (slot, (axis, field, value)) in size.iter_mut().zip([
        ("x", "Width", dims.x),
        ("y", "Height", dims.y),
        ("z", "Length", dims.z),
    ]) {
        *slot = u16::try_from(value).map_err(|_| SchemError::DimensionOverflow {
            axis,
            field,
            value,
        })?;
    }
    prepare_structure(block_array, target)?;
    if let Some((pos, _)) = placed_block_entities(block_array, target.data_version)
        .into_iter()
        .find(|(_, nbt)| !matches!(nbt.entries.get("id"), Some(Tag::String(_))))
    {
        return Err(SchemError::BlockEntityWithoutId { pos });
    }
    check_java_string(&file.stem).map_err(|source| SchemError::UnencodableName {
        name: file.stem.clone(),
        source,
    })?;
    let mut palette = Vec::with_capacity(block_array.palette.entries.len());
    let mut seen = HashMap::with_capacity(palette.capacity());
    for (index, state) in block_array.palette.entries.iter().enumerate() {
        let spelled = spell(state).ok_or_else(|| SchemError::UnspellableState {
            state: if state.properties.is_empty() {
                state.id.clone()
            } else {
                format!("{}[{}]", state.id, state.properties_text())
            },
        })?;
        check_java_string(&spelled).map_err(|source| SchemError::UnencodableState {
            state: spelled.clone(),
            source,
        })?;
        if let Some(first) = seen.insert(spelled.clone(), index) {
            return Err(SchemError::DuplicateState {
                state: spelled,
                first,
                second: index,
            });
        }
        palette.push(spelled);
    }
    let data_len = block_array
        .voxels
        .iter()
        .map(|index| varint_len(index.0))
        .sum();
    if i32::try_from(data_len).is_err() {
        return Err(SchemError::DataTooLong {
            len: data_len,
            limit: i32::MAX as usize,
        });
    }
    Ok(Schem {
        data_version: target.data_version,
        size,
        palette,
        data_len,
        file,
    })
}

/// `state` as a `Palette` key, its properties sorted by name, or `None` when the spelling would
/// read back as another state.
fn spell(state: &BlockState) -> Option<String> {
    if state.id.contains('[') {
        return None;
    }
    let (id, properties) = state.canonical_key();
    if properties.is_empty() {
        return Some(id.to_owned());
    }
    let mut spelled = format!("{id}[");
    for (position, (name, value)) in properties.into_iter().enumerate() {
        if name.is_empty() || name.contains(STATE_SEPARATORS) || value.contains(STATE_SEPARATORS) {
            return None;
        }
        if position > 0 {
            spelled.push(',');
        }
        spelled.push_str(name);
        spelled.push('=');
        spelled.push_str(value);
    }
    spelled.push(']');
    Some(spelled)
}

impl Schem<'_> {
    /// The file name without its extension.
    #[must_use]
    pub fn stem(&self) -> &str {
        &self.file.stem
    }

    /// Gzip-writes the schematic, encoding `Blocks.Data` from the grid as it goes.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::Io`] only, since [`prepare_schem`] ruled out every encoding error.
    pub fn write_gzip<W: Write>(&self, writer: &mut W) -> Result<(), NbtIoError> {
        let gz = flate2::write::GzEncoder::new(writer, flate2::Compression::default());
        // The stream writes a few bytes at a time, and each write into the encoder compresses.
        let mut gz = std::io::BufWriter::new(gz);
        stream_java_uncompressed(&mut gz, "", |root| {
            root.compound("Schematic", |schematic| self.write_schematic(schematic))
        })?;
        gz.into_inner()
            .map_err(std::io::IntoInnerError::into_error)?
            .finish()?;
        Ok(())
    }

    /// The `Schematic` compound's entries, in the order the specification lists them.
    fn write_schematic<W: Write>(
        &self,
        schematic: &mut CompoundStream<'_, W>,
    ) -> Result<(), NbtIoError> {
        schematic.tag("Version", &Tag::Int(VERSION))?;
        schematic.tag("DataVersion", &Tag::Int(self.data_version))?;
        schematic.compound_tag("Metadata", &self.metadata())?;
        for (field, extent) in ["Width", "Height", "Length"].into_iter().zip(self.size) {
            schematic.tag(field, &Tag::Short(extent.cast_signed()))?;
        }
        let (x, y, z) = self.file.offset;
        schematic.tag("Offset", &Tag::IntArray(vec![x, y, z]))?;
        schematic.compound("Blocks", |blocks| self.write_blocks(blocks))
    }

    /// `Metadata`, without the date that would make a build's bytes vary.
    fn metadata(&self) -> Compound {
        let mut metadata = Compound::new();
        metadata.insert("Name", Tag::String(self.file.stem.clone()));
        metadata.insert("Author", Tag::String("Cairn".to_owned()));
        metadata
    }

    /// The `Blocks` container: `Palette`, `Data` and `BlockEntities`.
    fn write_blocks<W: Write>(&self, blocks: &mut CompoundStream<'_, W>) -> Result<(), NbtIoError> {
        blocks.compound("Palette", |palette| {
            for (index, spelled) in self.palette.iter().enumerate() {
                let index = i32::try_from(index).expect("a block array's palette fits an i32");
                palette.tag(spelled, &Tag::Int(index))?;
            }
            Ok(())
        })?;
        let voxels = &self.file.block_array.voxels;
        let data = voxels.iter().flat_map(|index| varint(index.0));
        blocks.byte_array("Data", self.data_len, data)?;
        let block_entities: Vec<Compound> =
            placed_block_entities(self.file.block_array, self.data_version)
                .into_iter()
                .map(|((x, y, z), mut data)| {
                    let id = data
                        .entries
                        .shift_remove("id")
                        .expect("prepare_schem refuses a block entity without a string id");
                    let pos = [x, y, z].map(|v| i32::try_from(v).expect("dims checked to fit u16"));
                    let mut nbt = Compound::new();
                    nbt.insert("Pos", Tag::IntArray(pos.to_vec()));
                    nbt.insert("Id", id);
                    nbt.insert("Data", Tag::Compound(data));
                    nbt
                })
                .collect();
        blocks.tag(
            "BlockEntities",
            &Tag::List(List::of_compounds(block_entities)),
        )
    }
}

/// How many bytes `index` takes as a varint.
fn varint_len(index: u16) -> usize {
    match index {
        0..=0x7f => 1,
        0x80..=0x3fff => 2,
        _ => 3,
    }
}

/// `index` as a varint: seven bits a byte from the low end, every byte but the last with its high
/// bit set.
fn varint(index: u16) -> impl Iterator<Item = i8> {
    let mut rest = Some(index);
    std::iter::from_fn(move || {
        let value = rest?;
        let low = u8::try_from(value & 0x7f).expect("seven bits fit a byte");
        let more = value >> 7;
        rest = (more != 0).then_some(more);
        let byte = if more == 0 { low } else { low | 0x80 };
        Some(byte.cast_signed())
    })
}
