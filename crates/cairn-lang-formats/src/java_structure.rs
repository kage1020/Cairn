//! A [`BlockArray`] as a Java structure file: `size`, `palette`, `blocks`, an empty `entities`
//! and `DataVersion`. Lowering never fills the IR's entities or block entities, so none is written.

use cairn_lang_core::block_array::{BlockArray, BlockState};
pub use cairn_lang_nbt::Compound;
use cairn_lang_nbt::tag::{List, Tag};
use cairn_lang_nbt::{
    CompoundStream, NbtIoError, check_string, stream_java_uncompressed, write_java_gzip,
};
use thiserror::Error;

use crate::block_id::is_concrete_id;
use crate::data_version::JavaTarget;
use crate::dims::{VolumeError, dim_to_i32, dims_to_i32, fits_list_limit, list_volume};

/// Why a [`BlockArray`] cannot be written as a Java structure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum JavaStructureError {
    /// An NBT writer failure, only I/O once [`prepare_structure`] has passed.
    #[error("nbt: {0}")]
    Nbt(#[from] NbtIoError),
    /// A palette id is not `namespace:identifier`, which the game would load as air.
    #[error("palette entry `{id}` is abstract; expected `namespace:identifier`")]
    AbstractPaletteEntry {
        /// The id.
        id: String,
    },
    /// A voxel names a slot the palette lacks, reachable only through the public fields.
    #[error("voxel palette index {index} is outside the {len}-entry palette")]
    PaletteIndexOutOfRange {
        /// The slot.
        index: u16,
        /// The palette's length.
        len: usize,
    },
    /// A dimension past the `i32` wire width.
    #[error("dimension {axis} = {value} exceeds Java NBT i32 wire limit")]
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
        /// `"blocks"` or `"palette"`.
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
    /// A palette string the NBT encoder cannot write.
    #[error("palette entry `{id}`: {field} cannot be written as NBT: {source}")]
    UnencodablePaletteString {
        /// The entry's id.
        id: String,
        /// Which string: the id, a property name or a property value.
        field: String,
        /// The encoder's refusal.
        source: NbtIoError,
    },
}

impl From<crate::dims::DimensionOverflow> for JavaStructureError {
    fn from(
        crate::dims::DimensionOverflow { axis, value }: crate::dims::DimensionOverflow,
    ) -> Self {
        Self::DimensionOverflow { axis, value }
    }
}

impl From<VolumeError> for JavaStructureError {
    fn from(err: VolumeError) -> Self {
        match err {
            VolumeError::Overflow { x, y, z } => Self::VolumeOverflow { x, y, z },
            VolumeError::PastListLimit { volume } => Self::ListTooLong {
                list: "blocks",
                len: volume,
            },
            VolumeError::Mismatch { volume, voxels } => Self::VoxelCountMismatch { volume, voxels },
        }
    }
}

/// A [`BlockArray`] checked to write as a Java structure for its target, so writing it can fail
/// only on I/O.
#[derive(Clone, Copy)]
pub struct JavaStructure<'a> {
    block_array: &'a BlockArray,
    size: [i32; 3],
    /// The `blocks` list's length, checked to fit `i32` and to equal `voxels.len()`.
    volume: usize,
    data_version: i32,
}

/// Leaves out the grid, which can hold millions of voxels.
impl std::fmt::Debug for JavaStructure<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JavaStructure")
            .field("source_scope", &self.block_array.source_scope)
            .field("size", &self.size)
            .field("volume", &self.volume)
            .field("palette_len", &self.block_array.palette.entries.len())
            .field("data_version", &self.data_version)
            .finish()
    }
}

/// Checks that `block_array` writes as a Java structure for `target`, without building anything.
///
/// # Errors
///
/// The first fault found, as any [`JavaStructureError`] but [`JavaStructureError::Nbt`].
pub fn prepare_structure<'a>(
    block_array: &'a BlockArray,
    target: &JavaTarget,
) -> Result<JavaStructure<'a>, JavaStructureError> {
    let entries = &block_array.palette.entries;
    for entry in entries {
        if !is_concrete_id(&entry.id) {
            return Err(JavaStructureError::AbstractPaletteEntry {
                id: entry.id.clone(),
            });
        }
        check_palette_strings(entry)?;
    }
    if !fits_list_limit(entries.len()) {
        return Err(JavaStructureError::ListTooLong {
            list: "palette",
            len: entries.len(),
        });
    }
    let size = dims_to_i32(&block_array.dims)?;
    let volume = list_volume(block_array)?;
    if let Some((index, len)) = block_array.first_index_outside_palette() {
        return Err(JavaStructureError::PaletteIndexOutOfRange { index, len });
    }
    Ok(JavaStructure {
        block_array,
        size,
        volume,
        data_version: target.data_version,
    })
}

/// Refuses the first string of `entry` the NBT encoder could not write.
fn check_palette_strings(entry: &BlockState) -> Result<(), JavaStructureError> {
    let refuse = |field: String, source: NbtIoError| JavaStructureError::UnencodablePaletteString {
        id: entry.id.clone(),
        field,
        source,
    };
    check_string(&entry.id).map_err(|source| refuse("its id".to_owned(), source))?;
    for (key, value) in &entry.properties {
        check_string(key).map_err(|source| refuse(format!("property name `{key}`"), source))?;
        check_string(value)
            .map_err(|source| refuse(format!("the value of property `{key}`"), source))?;
    }
    Ok(())
}

impl JavaStructure<'_> {
    /// Gzip-writes the structure, encoding each `blocks` entry from the grid as it goes.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::Io`] only, since [`prepare_structure`] ruled out every encoding error.
    pub fn write_gzip<W: std::io::Write>(&self, writer: &mut W) -> Result<(), NbtIoError> {
        // As `write_java_gzip` does; the streaming API names no `flate2` type.
        let mut gz = flate2::write::GzEncoder::new(writer, flate2::Compression::default());
        stream_java_uncompressed(&mut gz, "", |root| self.write_root(root))?;
        gz.finish()?;
        Ok(())
    }

    /// The root's entries, in [`build_structure_tag`]'s order.
    fn write_root<W: std::io::Write>(
        &self,
        root: &mut CompoundStream<'_, W>,
    ) -> Result<(), NbtIoError> {
        let block_array = self.block_array;
        root.tag("size", &Tag::List(List::of_ints(self.size)))?;
        root.tag(
            "palette",
            &Tag::List(palette_list(&block_array.palette.entries)),
        )?;
        root.list("blocks", 10, self.volume, |blocks| {
            // `blocks_list`'s (y, z, x) order.
            for y in 0..block_array.dims.y {
                let yi = i32::try_from(y).expect("dims checked to fit i32");
                for z in 0..block_array.dims.z {
                    let zi = i32::try_from(z).expect("dims checked to fit i32");
                    for x in 0..block_array.dims.x {
                        let xi = i32::try_from(x).expect("dims checked to fit i32");
                        let i = block_array
                            .dims
                            .index(x, y, z)
                            .expect("voxel coordinate in dims by construction");
                        let state = i32::from(block_array.voxels[i].0);
                        blocks.compound(|entry| {
                            entry.tag("state", &Tag::Int(state))?;
                            entry.list("pos", 3, 3, |pos| {
                                pos.item(&Tag::Int(xi))?;
                                pos.item(&Tag::Int(yi))?;
                                pos.item(&Tag::Int(zi))
                            })
                        })?;
                    }
                }
            }
            Ok(())
        })?;
        root.tag("entities", &Tag::List(List::empty()))?;
        root.tag("DataVersion", &Tag::Int(self.data_version))
    }
}

/// The root [`Compound`] of a Java structure, a compound per voxel; to write a large one, use
/// [`JavaStructure::write_gzip`].
///
/// # Errors
///
/// Every refusal of [`prepare_structure`].
pub fn build_structure_tag(
    block_array: &BlockArray,
    target: &JavaTarget,
) -> Result<Compound, JavaStructureError> {
    let prepared = prepare_structure(block_array, target)?;

    let mut root = Compound::new();
    root.insert("size", Tag::List(List::of_ints(prepared.size)));
    root.insert(
        "palette",
        Tag::List(palette_list(&block_array.palette.entries)),
    );
    root.insert("blocks", Tag::List(blocks_list(block_array)?));
    root.insert("entities", Tag::List(List::empty()));
    root.insert("DataVersion", Tag::Int(prepared.data_version));
    Ok(root)
}

/// Checks and gzip-writes `block_array`, leaving `writer` untouched when a check fails.
///
/// # Errors
///
/// Every refusal of [`prepare_structure`], or [`JavaStructureError::Nbt`] for an I/O failure.
pub fn write_structure_gzip<W: std::io::Write>(
    writer: &mut W,
    block_array: &BlockArray,
    target: &JavaTarget,
) -> Result<(), JavaStructureError> {
    prepare_structure(block_array, target)?.write_gzip(writer)?;
    Ok(())
}

/// Gzip-writes a structure [`Compound`] under the empty root name.
///
/// # Errors
///
/// An I/O failure, or a tag the encoder rejects.
pub fn write_compound_gzip<W: std::io::Write>(
    writer: &mut W,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_java_gzip(writer, "", root)
}

/// The file extension of a compiled structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputExt {
    /// Java vanilla structure (`.nbt`).
    Nbt,
    /// Bedrock structure (`.mcstructure`).
    Mcstructure,
}

impl OutputExt {
    /// The extension without its dot.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            OutputExt::Nbt => "nbt",
            OutputExt::Mcstructure => "mcstructure",
        }
    }
}

/// The file name of one structure, such as `"cottage.nbt"` for `"struct::cottage"`.
#[must_use]
pub fn output_filename(source_scope: &str, ext: OutputExt) -> String {
    format!(
        "{}.{}",
        cairn_lang_core::artifact_stem(source_scope),
        ext.as_str()
    )
}

fn palette_list(entries: &[BlockState]) -> List {
    let compounds: Vec<Compound> = entries.iter().map(palette_entry).collect();
    List::of_compounds(compounds)
}

fn palette_entry(state: &BlockState) -> Compound {
    let mut compound = Compound::new();
    compound.insert("Name", Tag::String(state.id.clone()));
    if !state.properties.is_empty() {
        let mut props = Compound::new();
        for (k, v) in &state.properties {
            props.insert(k.clone(), Tag::String(v.clone()));
        }
        compound.insert("Properties", Tag::Compound(props));
    }
    compound
}

fn blocks_list(block_array: &BlockArray) -> Result<List, JavaStructureError> {
    // Air is written too, as the game's structure blocks do, so placement can tell it from void.
    let mut entries: Vec<Compound> = Vec::with_capacity(block_array.dims.volume());
    for y in 0..block_array.dims.y {
        let yi = dim_to_i32(y, "y")?;
        for z in 0..block_array.dims.z {
            let zi = dim_to_i32(z, "z")?;
            for x in 0..block_array.dims.x {
                let xi = dim_to_i32(x, "x")?;
                let i = block_array
                    .dims
                    .index(x, y, z)
                    .expect("voxel coordinate in dims by construction");
                let palette_index = block_array.voxels[i];
                let mut entry = Compound::new();
                entry.insert("state", Tag::Int(i32::from(palette_index.0)));
                entry.insert("pos", Tag::List(List::of_ints([xi, yi, zi])));
                entries.push(entry);
            }
        }
    }
    Ok(List::of_compounds(entries))
}
