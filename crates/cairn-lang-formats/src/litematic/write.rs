//! A [`BlockArrayIr`]'s structures as `.litematic` files: each struct in a file of its own, and
//! each site's placements and walkways in one file, a region each at its site position.

use std::io::Write;

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr};
use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{CompoundStream, NbtIoError, check_string, stream_java_uncompressed};
use indexmap::IndexMap;
use thiserror::Error;

use super::{AIR_IDS, SUB_VERSION, bits_per_entry, long_count, schematic_version};
use crate::data_version::JavaTarget;
use crate::java_structure::{
    JavaStructureError, entity_nbt, palette_entry, placed_block_entities, prepare_structure,
};
use crate::site_frame::site_origin;

/// The keys every Litematica palette entry names its block and properties by.
const PALETTE_KEYS: (&str, &str) = ("Name", "Properties");

/// What one `.litematic` file holds: a struct's array, or a site's placements and walkways.
#[derive(Debug, Clone)]
pub struct LitematicFile<'a> {
    /// The IR scope the file stands for: a struct's key, or `site::<name>` for a site.
    pub scope: String,
    /// The file name without its extension, which `Metadata.Name` repeats.
    pub stem: String,
    /// The regions, in the IR's order.
    pub regions: Vec<LitematicRegion<'a>>,
}

/// One region of a [`LitematicFile`]: a block array and where its minimum corner sits.
#[derive(Clone)]
pub struct LitematicRegion<'a> {
    /// The [`BlockArrayIr::structures`] key the region is written from.
    pub scope: &'a str,
    /// The region's name, the stem the structure's `.nbt` file would have.
    pub name: String,
    /// Where the region's minimum corner sits in the file's frame.
    pub position: (i32, i32, i32),
    /// The region's voxels.
    pub block_array: &'a BlockArray,
}

/// Leaves out the grid, which can hold millions of voxels.
impl std::fmt::Debug for LitematicRegion<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LitematicRegion")
            .field("scope", &self.scope)
            .field("name", &self.name)
            .field("position", &self.position)
            .field("dims", &self.block_array.dims)
            .finish_non_exhaustive()
    }
}

/// The `.litematic` files `ir` writes to, each where its first structure is in the IR.
#[must_use]
pub fn litematic_files(ir: &BlockArrayIr) -> Vec<LitematicFile<'_>> {
    let mut files: IndexMap<String, LitematicFile<'_>> = IndexMap::new();
    for (scope, block_array) in &ir.structures {
        let name = ir.artifact_stem(scope);
        let (file_scope, stem, position) = match site_origin(ir, scope) {
            Some((site, origin)) => (format!("site::{site}"), site.to_string(), origin),
            None => (scope.clone(), name.clone(), (0, 0, 0)),
        };
        files
            .entry(file_scope)
            .or_insert_with_key(|file_scope| LitematicFile {
                scope: file_scope.clone(),
                stem,
                regions: Vec::new(),
            })
            .regions
            .push(LitematicRegion {
                scope,
                name,
                position,
                block_array,
            });
    }
    files.into_values().collect()
}

/// Why a [`LitematicFile`] cannot be written.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LitematicError {
    /// A region's array fails a check the `.nbt` writer applies to it too.
    #[error("region `{region}`: {source}")]
    Region {
        /// The region's name.
        region: String,
        /// The `.nbt` writer's refusal.
        source: JavaStructureError,
    },
    /// The file's or a region's name is not a string the NBT encoder writes.
    #[error("the name `{name}` cannot be written as NBT: {source}")]
    UnencodableName {
        /// The name.
        name: String,
        /// The encoder's refusal.
        source: NbtIoError,
    },
    /// A region's far corner lies past the `i32` a Litematica position holds.
    #[error("region `{region}` reaches {value} on {axis}, past the i32 a position holds")]
    CornerOverflow {
        /// The region's name.
        region: String,
        /// `"x"`, `"y"` or `"z"`.
        axis: &'static str,
        /// The far corner's coordinate on that axis.
        value: i64,
    },
    /// A `Metadata` count or extent past `i32`.
    #[error("Metadata.{field} would be {value}, past the i32 it is stored as")]
    MetadataOverflow {
        /// `"RegionCount"`, `"TotalVolume"`, `"TotalBlocks"`, or an `"EnclosingSize"` axis.
        field: &'static str,
        /// The value.
        value: i64,
    },
}

/// A [`LitematicFile`] checked to write for its target, so writing it can fail only on I/O.
#[derive(Debug)]
pub struct Litematic<'a> {
    file: LitematicFile<'a>,
    data_version: i32,
    region_count: i32,
    enclosing_size: [i32; 3],
    total_volume: i32,
    total_blocks: i32,
}

/// Checks that `file` writes as a `.litematic` for `target`, without building anything.
///
/// # Errors
///
/// The first fault found, region by region.
pub fn prepare_litematic<'a>(
    file: LitematicFile<'a>,
    target: &JavaTarget,
) -> Result<Litematic<'a>, LitematicError> {
    let unencodable = |name: &str, source| LitematicError::UnencodableName {
        name: name.to_owned(),
        source,
    };
    check_string(&file.stem).map_err(|source| unencodable(&file.stem, source))?;
    let mut min = [i64::MAX; 3];
    let mut max = [i64::MIN; 3];
    let (mut total_volume, mut total_blocks) = (0_i64, 0_i64);
    for region in &file.regions {
        check_string(&region.name).map_err(|source| unencodable(&region.name, source))?;
        prepare_structure(region.block_array, target).map_err(|source| LitematicError::Region {
            region: region.name.clone(),
            source,
        })?;
        let dims = region.block_array.dims;
        let corner = [region.position.0, region.position.1, region.position.2];
        for (axis, ((&start, len), name)) in corner
            .iter()
            .zip([dims.x, dims.y, dims.z])
            .zip(["x", "y", "z"])
            .enumerate()
        {
            let far = i64::from(start) + i64::from(len) - 1;
            if i32::try_from(far).is_err() {
                return Err(LitematicError::CornerOverflow {
                    region: region.name.clone(),
                    axis: name,
                    value: far,
                });
            }
            min[axis] = min[axis].min(i64::from(start));
            max[axis] = max[axis].max(far);
        }
        let volume = i64::try_from(dims.volume()).unwrap_or(i64::MAX);
        total_volume = total_volume.saturating_add(volume);
        total_blocks = total_blocks.saturating_add(count_blocks(region.block_array));
    }
    let mut enclosing_size = [0; 3];
    if !file.regions.is_empty() {
        for (axis, field) in ["EnclosingSize.x", "EnclosingSize.y", "EnclosingSize.z"]
            .into_iter()
            .enumerate()
        {
            let size = max[axis] - min[axis] + 1;
            enclosing_size[axis] = metadata_int(field, size)?;
        }
    }
    let region_count = i64::try_from(file.regions.len()).unwrap_or(i64::MAX);
    Ok(Litematic {
        data_version: target.data_version,
        region_count: metadata_int("RegionCount", region_count)?,
        enclosing_size,
        total_volume: metadata_int("TotalVolume", total_volume)?,
        total_blocks: metadata_int("TotalBlocks", total_blocks)?,
        file,
    })
}

fn metadata_int(field: &'static str, value: i64) -> Result<i32, LitematicError> {
    i32::try_from(value).map_err(|_| LitematicError::MetadataOverflow { field, value })
}

/// The voxels of `block_array` that Litematica counts as blocks: those that are not air.
fn count_blocks(block_array: &BlockArray) -> i64 {
    let counted: Vec<bool> = block_array
        .palette
        .entries
        .iter()
        .map(|state| !AIR_IDS.contains(&state.id.as_str()))
        .collect();
    let blocks = block_array
        .voxels
        .iter()
        .filter(|index| counted[usize::from(index.0)])
        .count();
    i64::try_from(blocks).unwrap_or(i64::MAX)
}

impl Litematic<'_> {
    /// The file name without its extension.
    #[must_use]
    pub fn stem(&self) -> &str {
        &self.file.stem
    }

    /// Gzip-writes the schematic, packing each region's `BlockStates` from the grid as it goes.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::Io`] only, since [`prepare_litematic`] ruled out every encoding error.
    pub fn write_gzip<W: Write>(&self, writer: &mut W) -> Result<(), NbtIoError> {
        let mut gz = flate2::write::GzEncoder::new(writer, flate2::Compression::default());
        stream_java_uncompressed(&mut gz, "", |root| self.write_root(root))?;
        gz.finish()?;
        Ok(())
    }

    /// The root's entries, in the order Litematica writes them.
    fn write_root<W: Write>(&self, root: &mut CompoundStream<'_, W>) -> Result<(), NbtIoError> {
        root.tag("MinecraftDataVersion", &Tag::Int(self.data_version))?;
        root.tag("Version", &Tag::Int(schematic_version(self.data_version)))?;
        root.tag("SubVersion", &Tag::Int(SUB_VERSION))?;
        root.compound_tag("Metadata", &self.metadata())?;
        root.compound("Regions", |regions| {
            for region in &self.file.regions {
                regions.compound(&region.name, |body| write_region(body, region))?;
            }
            Ok(())
        })
    }

    /// `Metadata`, without the times and preview image that would make a build's bytes vary.
    fn metadata(&self) -> Compound {
        let mut metadata = Compound::new();
        metadata.insert("Name", Tag::String(self.file.stem.clone()));
        metadata.insert("Author", Tag::String("Cairn".to_owned()));
        metadata.insert("Description", Tag::String(String::new()));
        metadata.insert("RegionCount", Tag::Int(self.region_count));
        metadata.insert("TotalVolume", Tag::Int(self.total_volume));
        metadata.insert("TotalBlocks", Tag::Int(self.total_blocks));
        metadata.insert("EnclosingSize", Tag::Compound(vec3(self.enclosing_size)));
        metadata
    }
}

/// One region's entries, in the order Litematica writes them.
fn write_region<W: Write>(
    body: &mut CompoundStream<'_, W>,
    region: &LitematicRegion<'_>,
) -> Result<(), NbtIoError> {
    let block_array = region.block_array;
    let entries = &block_array.palette.entries;
    let palette: Vec<Compound> = entries
        .iter()
        .map(|state| palette_entry(state, PALETTE_KEYS))
        .collect();
    body.tag("BlockStatePalette", &Tag::List(List::of_compounds(palette)))?;
    let bits = bits_per_entry(entries.len());
    let indices = block_array.voxels.iter().map(|index| index.0);
    body.long_array(
        "BlockStates",
        long_count(block_array.voxels.len(), bits),
        pack(indices, bits),
    )?;
    let tile_entities: Vec<Compound> = placed_block_entities(block_array)
        .into_iter()
        .map(|entity| {
            let mut nbt = entity_nbt(entity);
            let (x, y, z) = entity.pos;
            for (axis, value) in [("x", x), ("y", y), ("z", z)] {
                let value = i32::try_from(value).expect("dims checked to fit i32");
                nbt.insert(axis, Tag::Int(value));
            }
            nbt
        })
        .collect();
    body.tag(
        "TileEntities",
        &Tag::List(List::of_compounds(tile_entities)),
    )?;
    for empty in ["PendingBlockTicks", "PendingFluidTicks", "Entities"] {
        body.tag(empty, &Tag::List(List::empty()))?;
    }
    let (x, y, z) = region.position;
    body.compound_tag("Position", &vec3([x, y, z]))?;
    let dims = block_array.dims;
    let size = [dims.x, dims.y, dims.z].map(|d| i32::try_from(d).expect("dims checked to fit i32"));
    body.compound_tag("Size", &vec3(size))
}

/// `{x, y, z}`, as Litematica stores a position or a size.
fn vec3([x, y, z]: [i32; 3]) -> Compound {
    let mut compound = Compound::new();
    compound.insert("x", Tag::Int(x));
    compound.insert("y", Tag::Int(y));
    compound.insert("z", Tag::Int(z));
    compound
}

/// `indices` packed `bits` at a time from each long's low end, an index that does not fit what
/// is left of a long continuing in the next one.
fn pack(indices: impl Iterator<Item = u16>, bits: u32) -> impl Iterator<Item = i64> {
    let mut indices = indices.fuse();
    let (mut pending, mut filled) = (0_u128, 0_u32);
    std::iter::from_fn(move || {
        while filled < 64 {
            match indices.next() {
                Some(index) => {
                    pending |= u128::from(index) << filled;
                    filled += bits;
                }
                None if filled == 0 => return None,
                None => break,
            }
        }
        let long = u64::try_from(pending & u128::from(u64::MAX)).expect("masked to 64 bits");
        pending >>= 64;
        filled = filled.saturating_sub(64);
        Some(long.cast_signed())
    })
}
