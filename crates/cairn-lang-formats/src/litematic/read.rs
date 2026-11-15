//! Reads a `.litematic` one field at a time, so only the fields it returns are held, and only one
//! region's `BlockStates` is in memory as NBT at once.

use std::io::Read;

use cairn_lang_core::block_array::{
    BlockState, Dims, MAX_STRUCTURE_VOLUME, PALETTE_CAPACITY, PaletteIndex,
};
use cairn_lang_nbt::tag::{Compound, Tag};
use cairn_lang_nbt::{Event, NbtReadError, NbtReader};
use thiserror::Error;

use super::{bits_per_entry, long_count};
use crate::read_stream::{lists_compounds, next_inside, rest_of_compound};
use crate::volume::{BlockVolume, StoredBlockEntity, block_state_from_nbt};

/// The `Version`s read: 5, the first whose palette is post-flattening, to 7.
const VERSIONS: std::ops::RangeInclusive<i32> = 5..=7;

/// A `.litematic` as read.
#[derive(Debug, Clone, PartialEq)]
pub struct LitematicSchematic {
    /// `MinecraftDataVersion`: the Java `DataVersion` the file was saved for.
    pub data_version: i32,
    /// `Version`, the schematic format's.
    pub version: i32,
    /// `SubVersion`, which files from before it existed lack.
    pub sub_version: Option<i32>,
    /// `Metadata`.
    pub metadata: LitematicMetadata,
    /// `Regions`, in the file's order.
    pub regions: Vec<LitematicReadRegion>,
}

/// The `Metadata` fields a reader can use, each as stored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LitematicMetadata {
    /// `Name`.
    pub name: Option<String>,
    /// `Author`.
    pub author: Option<String>,
    /// `Description`.
    pub description: Option<String>,
    /// `RegionCount`.
    pub region_count: Option<i32>,
    /// `TotalVolume`.
    pub total_volume: Option<i32>,
    /// `TotalBlocks`, the voxels that are not air.
    pub total_blocks: Option<i32>,
    /// `EnclosingSize`, the extent of the box around every region.
    pub enclosing_size: Option<[i32; 3]>,
}

/// One region of a [`LitematicSchematic`].
#[derive(Debug, Clone, PartialEq)]
pub struct LitematicReadRegion {
    /// The region's name.
    pub name: String,
    /// Where the region's minimum corner sits in the schematic's frame, whichever corner its
    /// `Position` and `Size` name.
    pub min_corner: (i32, i32, i32),
    /// Its voxels, from the minimum corner, with `TileEntities` as its block entities.
    pub volume: BlockVolume,
    /// `Entities`, each as stored.
    pub entities: Vec<Compound>,
}

/// Why bytes could not be read as a `.litematic`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LitematicReadError {
    /// The bytes are not one gzip-wrapped NBT root.
    #[error("{0}")]
    Nbt(#[from] NbtReadError),
    /// A field is absent, or not the shape Litematica writes.
    #[error("`{path}` is missing or is not {expected}")]
    Malformed {
        /// The field, as `Regions.<name>.Size` or `Regions.<name>.TileEntities[2].x`.
        path: String,
        /// What Litematica writes there.
        expected: &'static str,
    },
    /// A `Version` outside the ones read.
    #[error(
        "litematic `Version` {version} is not one this reader reads (5 to 7); a file older \
         than 5 predates the flattening"
    )]
    UnsupportedVersion {
        /// The version.
        version: i32,
    },
    /// A region larger than a block array may be.
    #[error("region `{region}` holds {volume} voxels, past the {limit} a block array may hold")]
    RegionTooLarge {
        /// The region's name.
        region: String,
        /// Its voxel count, saturated at `u64::MAX`.
        volume: u64,
        /// The most a block array holds.
        limit: usize,
    },
    /// A palette longer than a block array's.
    #[error("region `{region}` has {len} palette entries, past the {limit} a block array holds")]
    PaletteTooLarge {
        /// The region's name.
        region: String,
        /// The palette's length.
        len: usize,
        /// The most a block array holds.
        limit: usize,
    },
    /// `BlockStates` holds other than the longs its region's voxels fill.
    #[error(
        "region `{region}`'s BlockStates has {len} longs, where its voxels at the {bits} bits \
         its palette takes fill {needed}"
    )]
    BlockStatesLength {
        /// The region's name.
        region: String,
        /// The longs stored.
        len: usize,
        /// The longs the voxels fill.
        needed: usize,
        /// The bits each voxel's index takes.
        bits: u32,
    },
    /// `BlockStates` sets bits past its last voxel's index, which Litematica leaves clear.
    #[error(
        "region `{region}`'s BlockStates sets bits past its last voxel, so it was not packed at \
         the {bits} bits its {len}-entry palette takes"
    )]
    BlockStatesPadding {
        /// The region's name.
        region: String,
        /// The bits each voxel's index takes.
        bits: u32,
        /// The palette's length.
        len: usize,
    },
    /// A packed index past the end of its region's palette.
    #[error("region `{region}` has palette index {index} at voxel {voxel}, past its {len} entries")]
    PaletteIndexOutOfRange {
        /// The region's name.
        region: String,
        /// The voxel, in `(y, z, x)` order.
        voxel: usize,
        /// The index.
        index: u64,
        /// The palette's length.
        len: usize,
    },
}

/// Reads a gzip-wrapped `.litematic`, holding one region's NBT in memory at a time.
///
/// # Errors
///
/// A [`LitematicReadError`] for bytes that are not a `.litematic` this reader reads.
pub fn read_litematic<R: Read>(reader: R) -> Result<LitematicSchematic, LitematicReadError> {
    // As Java's `GZIPInputStream` reads them, members after the first continue the stream.
    let gz = flate2::read::MultiGzDecoder::new(reader);
    let mut nbt = NbtReader::java(gz)
        .with_max_len(MAX_STRUCTURE_VOLUME)
        .with_max_array_len(long_count(
            MAX_STRUCTURE_VOLUME,
            bits_per_entry(PALETTE_CAPACITY),
        ));
    nbt.next_event()?;
    let (mut data_version, mut version, mut sub_version) = (None, None, None);
    let mut metadata = None;
    let mut regions = None;
    while let Some(event) = nbt.next_event()? {
        match event {
            Event::Value {
                name: Some(name),
                tag: Tag::Int(value),
            } => match name.as_str() {
                "MinecraftDataVersion" => data_version = Some(value),
                "Version" => version = Some(value),
                "SubVersion" => sub_version = Some(value),
                _ => {}
            },
            Event::Compound { name: Some(name) } if name == "Metadata" => {
                metadata = Some(read_metadata(&mut nbt)?);
            }
            Event::Compound { name: Some(name) } if name == "Regions" => {
                regions = Some(read_regions(&mut nbt)?);
            }
            Event::Compound { .. } | Event::List { .. } => nbt.skip_rest()?,
            Event::Value { .. } | Event::End => {}
        }
    }
    nbt.finish()?;
    let version = version.ok_or_else(|| malformed("Version", "an int"))?;
    if !VERSIONS.contains(&version) {
        return Err(LitematicReadError::UnsupportedVersion { version });
    }
    let data_version = data_version.ok_or_else(|| malformed("MinecraftDataVersion", "an int"))?;
    let metadata = metadata.ok_or_else(|| malformed("Metadata", "a compound"))?;
    let regions = regions.ok_or_else(|| malformed("Regions", "a compound"))??;
    Ok(LitematicSchematic {
        data_version,
        version,
        sub_version,
        metadata,
        regions,
    })
}

fn malformed(path: impl Into<String>, expected: &'static str) -> LitematicReadError {
    LitematicReadError::Malformed {
        path: path.into(),
        expected,
    }
}

/// The fields of the open `Metadata` compound that [`LitematicMetadata`] keeps.
fn read_metadata<R: Read>(nbt: &mut NbtReader<R>) -> Result<LitematicMetadata, NbtReadError> {
    let mut metadata = LitematicMetadata::default();
    loop {
        match next_inside(nbt)? {
            Event::End => return Ok(metadata),
            Event::Value {
                name: Some(name),
                tag,
            } => match (name.as_str(), tag) {
                ("Name", Tag::String(value)) => metadata.name = Some(value),
                ("Author", Tag::String(value)) => metadata.author = Some(value),
                ("Description", Tag::String(value)) => metadata.description = Some(value),
                ("RegionCount", Tag::Int(value)) => metadata.region_count = Some(value),
                ("TotalVolume", Tag::Int(value)) => metadata.total_volume = Some(value),
                ("TotalBlocks", Tag::Int(value)) => metadata.total_blocks = Some(value),
                _ => {}
            },
            Event::Compound { name: Some(name) } if name == "EnclosingSize" => {
                metadata.enclosing_size = vec3(&rest_of_compound(nbt)?);
            }
            Event::Compound { .. } | Event::List { .. } => nbt.skip_rest()?,
            Event::Value { name: None, .. } => {}
        }
    }
}

/// The regions of the open `Regions` compound, each read and decoded in turn. A region that does
/// not decode stands in for them all, the rest of `Regions` skipped, so a `Version` after it is
/// still weighed first.
fn read_regions<R: Read>(
    nbt: &mut NbtReader<R>,
) -> Result<Result<Vec<LitematicReadRegion>, LitematicReadError>, NbtReadError> {
    let depth = nbt.depth();
    let mut regions = Vec::new();
    loop {
        match next_inside(nbt)? {
            Event::Compound { name: Some(name) } => match read_region(nbt, name) {
                Ok(region) => regions.push(region),
                Err(LitematicReadError::Nbt(err)) => return Err(err),
                Err(fault) => {
                    while nbt.depth() >= depth {
                        nbt.skip_rest()?;
                    }
                    return Ok(Err(fault));
                }
            },
            // Litematica passes over an entry of `Regions` that is not a compound.
            Event::List { .. } => nbt.skip_rest()?,
            Event::Value { .. } | Event::Compound { name: None } => {}
            Event::End => return Ok(Ok(regions)),
        }
    }
}

fn int(compound: &Compound, key: &str) -> Option<i32> {
    match compound.entries.get(key) {
        Some(Tag::Int(value)) => Some(*value),
        _ => None,
    }
}

fn vec3(xyz: &Compound) -> Option<[i32; 3]> {
    Some([int(xyz, "x")?, int(xyz, "y")?, int(xyz, "z")?])
}

/// The fields of one region, in whatever order the file holds them.
#[derive(Default)]
struct RegionFields {
    position: Option<[i32; 3]>,
    size: Option<[i32; 3]>,
    palette: Option<Vec<BlockState>>,
    block_states: Option<Vec<i64>>,
    block_entities: Vec<StoredBlockEntity>,
    entities: Vec<Compound>,
}

/// The open region `name`, its fields read one at a time and every other entry skipped.
fn read_region<R: Read>(
    nbt: &mut NbtReader<R>,
    name: String,
) -> Result<LitematicReadRegion, LitematicReadError> {
    let path = format!("Regions.{name}");
    let mut fields = RegionFields::default();
    loop {
        match next_inside(nbt)? {
            Event::End => break,
            Event::Compound { name: Some(key) } if key == "Position" => {
                fields.position = vec3(&rest_of_compound(nbt)?);
            }
            Event::Compound { name: Some(key) } if key == "Size" => {
                fields.size = vec3(&rest_of_compound(nbt)?);
            }
            Event::List {
                name: Some(key),
                element_type_id,
                len,
            } => match key.as_str() {
                "BlockStatePalette" => {
                    if len > PALETTE_CAPACITY {
                        return Err(LitematicReadError::PaletteTooLarge {
                            region: name,
                            len,
                            limit: PALETTE_CAPACITY,
                        });
                    }
                    let mut palette = Vec::with_capacity(len);
                    let list = format!("{path}.BlockStatePalette");
                    each_compound(nbt, &list, element_type_id, |index, entry| {
                        let state = block_state_from_nbt(&entry).map_err(|fault| {
                            malformed(format!("{list}[{index}].{}", fault.field), fault.expected)
                        })?;
                        palette.push(state);
                        Ok(())
                    })?;
                    fields.palette = Some(palette);
                }
                "TileEntities" => {
                    let list = format!("{path}.TileEntities");
                    let mut block_entities = Vec::new();
                    each_compound(nbt, &list, element_type_id, |index, nbt| {
                        block_entities.push(stored_block_entity(nbt, &format!("{list}[{index}]"))?);
                        Ok(())
                    })?;
                    fields.block_entities = block_entities;
                }
                "Entities" => {
                    let mut entities = Vec::new();
                    each_compound(
                        nbt,
                        &format!("{path}.Entities"),
                        element_type_id,
                        |_, nbt| {
                            entities.push(nbt);
                            Ok(())
                        },
                    )?;
                    fields.entities = entities;
                }
                _ => nbt.skip_rest()?,
            },
            Event::Value {
                name: Some(key),
                tag: Tag::LongArray(longs),
            } if key == "BlockStates" => fields.block_states = Some(longs),
            Event::Compound { .. } | Event::List { .. } => nbt.skip_rest()?,
            Event::Value { .. } => {}
        }
    }
    decode_region(name, &path, fields)
}

/// Each compound of the open list `path`, read whole one at a time and handed to `item` with its
/// index.
fn each_compound<R: Read>(
    nbt: &mut NbtReader<R>,
    path: &str,
    element_type_id: u8,
    mut item: impl FnMut(usize, Compound) -> Result<(), LitematicReadError>,
) -> Result<(), LitematicReadError> {
    if !lists_compounds(element_type_id) {
        return Err(malformed(path, "a list of compounds"));
    }
    for index in 0.. {
        match next_inside(nbt)? {
            Event::Compound { name: None } => item(index, rest_of_compound(nbt)?)?,
            Event::End => break,
            event => unreachable!("a list of compounds holds only compounds, got {event:?}"),
        }
    }
    Ok(())
}

/// A `TileEntities` entry at `path`, its position checked to be a voxel coordinate.
fn stored_block_entity(
    mut nbt: Compound,
    path: &str,
) -> Result<StoredBlockEntity, LitematicReadError> {
    let Some(Tag::String(id)) = nbt.entries.shift_remove("id") else {
        return Err(malformed(format!("{path}.id"), "a block entity id string"));
    };
    let mut pos = [0; 3];
    for (axis, key) in pos.iter_mut().zip(["x", "y", "z"]) {
        let Some(Tag::Int(value)) = nbt.entries.shift_remove(key) else {
            return Err(malformed(format!("{path}.{key}"), "an int"));
        };
        *axis = u32::try_from(value)
            .map_err(|_| malformed(format!("{path}.{key}"), "an int, 0 or more"))?;
    }
    Ok(StoredBlockEntity {
        pos: (pos[0], pos[1], pos[2]),
        id,
        data: nbt,
    })
}

/// The region `fields` hold, checked to be one Litematica writes.
fn decode_region(
    name: String,
    path: &str,
    fields: RegionFields,
) -> Result<LitematicReadRegion, LitematicReadError> {
    let position = fields
        .position
        .ok_or_else(|| malformed(format!("{path}.Position"), "a compound of x, y and z ints"))?;
    let size = fields
        .size
        .filter(|size| size.iter().all(|&axis| axis != 0))
        .ok_or_else(|| {
            malformed(
                format!("{path}.Size"),
                "a compound of non-zero x, y and z ints",
            )
        })?;
    let mut min_corner = [0; 3];
    for axis in 0..3 {
        // A negative size runs from `Position` back towards lower coordinates.
        let min =
            i64::from(position[axis]) + i64::from(size[axis].min(0)) + i64::from(size[axis] < 0);
        min_corner[axis] = i32::try_from(min)
            .map_err(|_| malformed(format!("{path}.Position"), "a position inside i32"))?;
    }
    let [x, y, z] = size.map(i32::unsigned_abs);
    let dims = Dims { x, y, z };
    let volume = dims
        .checked_volume()
        .filter(|&volume| volume <= MAX_STRUCTURE_VOLUME)
        .ok_or_else(|| LitematicReadError::RegionTooLarge {
            region: name.clone(),
            volume: u64::from(x)
                .saturating_mul(u64::from(y))
                .saturating_mul(u64::from(z)),
            limit: MAX_STRUCTURE_VOLUME,
        })?;
    let palette = fields
        .palette
        .filter(|palette| !palette.is_empty())
        .ok_or_else(|| {
            malformed(
                format!("{path}.BlockStatePalette"),
                "a non-empty list of block states",
            )
        })?;
    let longs = fields
        .block_states
        .ok_or_else(|| malformed(format!("{path}.BlockStates"), "a long array"))?;
    let bits = bits_per_entry(palette.len());
    let voxels = unpack(&longs, volume, palette.len()).map_err(|fault| match fault {
        Unpacked::Length { needed } => LitematicReadError::BlockStatesLength {
            region: name.clone(),
            len: longs.len(),
            needed,
            bits,
        },
        Unpacked::Padding => LitematicReadError::BlockStatesPadding {
            region: name.clone(),
            bits,
            len: palette.len(),
        },
        Unpacked::OutOfRange { voxel, index } => LitematicReadError::PaletteIndexOutOfRange {
            region: name.clone(),
            voxel,
            index,
            len: palette.len(),
        },
    })?;
    for (index, entity) in fields.block_entities.iter().enumerate() {
        let (pos, extents) = ([entity.pos.0, entity.pos.1, entity.pos.2], [x, y, z]);
        if let Some(axis) = (0..3).find(|&axis| pos[axis] >= extents[axis]) {
            return Err(malformed(
                format!("{path}.TileEntities[{index}].{}", ["x", "y", "z"][axis]),
                "an int below the region's `Size` on that axis",
            ));
        }
    }
    let volume = BlockVolume::new(dims, palette, voxels, fields.block_entities)
        .expect("each part of a region is checked as it is decoded");
    Ok(LitematicReadRegion {
        name,
        min_corner: (min_corner[0], min_corner[1], min_corner[2]),
        volume,
        entities: fields.entities,
    })
}

/// Why packed `BlockStates` do not decode.
enum Unpacked {
    Length { needed: usize },
    Padding,
    OutOfRange { voxel: usize, index: u64 },
}

/// The `volume` palette indices `longs` packs for a palette of `palette_len` entries, which
/// Litematica writes in exactly the longs they fill, with the bits past the last index clear.
fn unpack(longs: &[i64], volume: usize, palette_len: usize) -> Result<Vec<PaletteIndex>, Unpacked> {
    let bits = bits_per_entry(palette_len);
    let needed = long_count(volume, bits);
    if longs.len() != needed {
        return Err(Unpacked::Length { needed });
    }
    let width = usize::try_from(bits).expect("at most 32 bits");
    let used = (volume * width) % 64;
    if used != 0 && longs[needed - 1].cast_unsigned() >> used != 0 {
        return Err(Unpacked::Padding);
    }
    let mask = (1_u64 << bits) - 1;
    let mut voxels = Vec::with_capacity(volume);
    for voxel in 0..volume {
        let offset = voxel * width;
        let (start, shift) = (offset / 64, offset % 64);
        let mut index = longs[start].cast_unsigned() >> shift;
        if shift + width > 64 {
            index |= longs[start + 1].cast_unsigned() << (64 - shift);
        }
        let index = index & mask;
        match u16::try_from(index) {
            Ok(slot) if usize::from(slot) < palette_len => voxels.push(PaletteIndex(slot)),
            _ => return Err(Unpacked::OutOfRange { voxel, index }),
        }
    }
    Ok(voxels)
}
