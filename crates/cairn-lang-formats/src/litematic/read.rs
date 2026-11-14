//! Reads a `.litematic` one region at a time, so only one region's `BlockStates` is in memory
//! as NBT at once.

use std::io::Read;

use cairn_lang_core::block_array::{Dims, MAX_STRUCTURE_VOLUME, PALETTE_CAPACITY, PaletteIndex};
use cairn_lang_nbt::tag::{Compound, Tag};
use cairn_lang_nbt::{Event, NbtReadError, NbtReader};
use thiserror::Error;

use super::{bits_per_entry, long_count};
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
    pub regions: Vec<ReadRegion>,
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
pub struct ReadRegion {
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
    /// The bytes are not gzip-wrapped NBT.
    #[error("{0}")]
    Nbt(#[from] NbtReadError),
    /// A field is absent, or not the shape Litematica writes.
    #[error("`{path}` is missing or is not {expected}")]
    Malformed {
        /// The field, as `Regions.<name>.Size`.
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
    /// `BlockStates` holds fewer longs than its region's voxels need.
    #[error("region `{region}`'s BlockStates has {len} longs; its voxels need {needed}")]
    BlockStatesTooShort {
        /// The region's name.
        region: String,
        /// The longs stored.
        len: usize,
        /// The longs needed.
        needed: usize,
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
    let gz = flate2::read::GzDecoder::new(reader);
    let mut nbt = NbtReader::java(gz).with_max_len(MAX_STRUCTURE_VOLUME);
    nbt.next_event()?;
    let (mut data_version, mut version, mut sub_version) = (None, None, None);
    let mut metadata = None;
    let mut regions = Vec::new();
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
                let Tag::Compound(compound) = nbt.read_rest()? else {
                    unreachable!("the rest of a compound reads as a compound");
                };
                metadata = Some(read_metadata(&compound));
            }
            Event::Compound { name: Some(name) } if name == "Regions" => {
                read_regions(&mut nbt, &mut regions)?;
            }
            Event::Compound { .. } | Event::List { .. } => nbt.skip_rest()?,
            Event::Value { .. } | Event::End => {}
        }
    }
    let version = version.ok_or_else(|| malformed("Version", "an int"))?;
    if !VERSIONS.contains(&version) {
        return Err(LitematicReadError::UnsupportedVersion { version });
    }
    Ok(LitematicSchematic {
        data_version: data_version.ok_or_else(|| malformed("MinecraftDataVersion", "an int"))?,
        version,
        sub_version,
        metadata: metadata.ok_or_else(|| malformed("Metadata", "a compound"))?,
        regions,
    })
}

fn malformed(path: impl Into<String>, expected: &'static str) -> LitematicReadError {
    LitematicReadError::Malformed {
        path: path.into(),
        expected,
    }
}

/// Each region of the open `Regions` compound, read and decoded one at a time.
fn read_regions<R: Read>(
    nbt: &mut NbtReader<R>,
    regions: &mut Vec<ReadRegion>,
) -> Result<(), LitematicReadError> {
    loop {
        match nbt.next_event()? {
            Some(Event::Compound { name: Some(name) }) => {
                let Tag::Compound(region) = nbt.read_rest()? else {
                    unreachable!("the rest of a compound reads as a compound");
                };
                regions.push(read_region(name, &region)?);
            }
            // Litematica passes over an entry of `Regions` that is not a compound.
            Some(Event::List { .. }) => nbt.skip_rest()?,
            Some(Event::Value { .. } | Event::Compound { name: None }) => {}
            Some(Event::End) | None => return Ok(()),
        }
    }
}

fn int(compound: &Compound, key: &str) -> Option<i32> {
    match compound.entries.get(key) {
        Some(Tag::Int(value)) => Some(*value),
        _ => None,
    }
}

fn string(compound: &Compound, key: &str) -> Option<String> {
    match compound.entries.get(key) {
        Some(Tag::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn vec3(compound: &Compound, key: &str) -> Option<[i32; 3]> {
    let Some(Tag::Compound(xyz)) = compound.entries.get(key) else {
        return None;
    };
    Some([int(xyz, "x")?, int(xyz, "y")?, int(xyz, "z")?])
}

fn read_metadata(metadata: &Compound) -> LitematicMetadata {
    LitematicMetadata {
        name: string(metadata, "Name"),
        author: string(metadata, "Author"),
        description: string(metadata, "Description"),
        region_count: int(metadata, "RegionCount"),
        total_volume: int(metadata, "TotalVolume"),
        total_blocks: int(metadata, "TotalBlocks"),
        enclosing_size: vec3(metadata, "EnclosingSize"),
    }
}

fn compounds<'c>(
    region: &'c Compound,
    path: &str,
    key: &str,
) -> Result<Vec<&'c Compound>, LitematicReadError> {
    match region.entries.get(key) {
        None => Ok(Vec::new()),
        Some(Tag::List(list)) => list
            .items
            .iter()
            .map(|item| match item {
                Tag::Compound(compound) => Ok(compound),
                _ => Err(malformed(format!("{path}.{key}"), "a list of compounds")),
            })
            .collect(),
        Some(_) => Err(malformed(format!("{path}.{key}"), "a list of compounds")),
    }
}

fn read_region(name: String, region: &Compound) -> Result<ReadRegion, LitematicReadError> {
    let path = format!("Regions.{name}");
    let xyz = "a compound of x, y and z ints";
    let position =
        vec3(region, "Position").ok_or_else(|| malformed(format!("{path}.Position"), xyz))?;
    let size = vec3(region, "Size")
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

    let palette = compounds(region, &path, "BlockStatePalette")?
        .into_iter()
        .map(block_state_from_nbt)
        .collect::<Option<Vec<_>>>()
        .filter(|palette| !palette.is_empty())
        .ok_or_else(|| {
            malformed(
                format!("{path}.BlockStatePalette"),
                "a non-empty list of block states, each a `Name` and its `Properties`",
            )
        })?;
    if palette.len() > PALETTE_CAPACITY {
        return Err(LitematicReadError::PaletteTooLarge {
            region: name,
            len: palette.len(),
            limit: PALETTE_CAPACITY,
        });
    }
    let Some(Tag::LongArray(longs)) = region.entries.get("BlockStates") else {
        return Err(malformed(format!("{path}.BlockStates"), "a long array"));
    };
    let voxels = unpack(longs, volume, palette.len()).map_err(|fault| match fault {
        Unpacked::TooShort { needed } => LitematicReadError::BlockStatesTooShort {
            region: name.clone(),
            len: longs.len(),
            needed,
        },
        Unpacked::OutOfRange { voxel, index } => LitematicReadError::PaletteIndexOutOfRange {
            region: name.clone(),
            voxel,
            index,
            len: palette.len(),
        },
    })?;

    let block_entities = compounds(region, &path, "TileEntities")?
        .into_iter()
        .map(|nbt| {
            stored_block_entity(nbt, dims).ok_or_else(|| {
                malformed(
                    format!("{path}.TileEntities"),
                    "a list of block entities, each an `id` and `x`, `y`, `z` inside the region",
                )
            })
        })
        .collect::<Result<_, _>>()?;
    let entities = compounds(region, &path, "Entities")?
        .into_iter()
        .cloned()
        .collect();
    Ok(ReadRegion {
        name,
        min_corner: (min_corner[0], min_corner[1], min_corner[2]),
        volume: BlockVolume {
            dims,
            palette,
            voxels,
            block_entities,
        },
        entities,
    })
}

/// A `TileEntities` entry, its position checked to lie inside `dims`.
fn stored_block_entity(nbt: &Compound, dims: Dims) -> Option<StoredBlockEntity> {
    let id = string(nbt, "id")?;
    let [x, y, z] = ["x", "y", "z"].map(|axis| int(nbt, axis).and_then(|v| u32::try_from(v).ok()));
    let pos = (x?, y?, z?);
    dims.index(pos.0, pos.1, pos.2)?;
    let mut data = nbt.clone();
    for key in ["id", "x", "y", "z"] {
        data.entries.shift_remove(key);
    }
    Some(StoredBlockEntity { pos, id, data })
}

/// Why packed `BlockStates` do not decode.
enum Unpacked {
    TooShort { needed: usize },
    OutOfRange { voxel: usize, index: u64 },
}

/// The `volume` palette indices `longs` packs for a palette of `palette_len` entries.
fn unpack(longs: &[i64], volume: usize, palette_len: usize) -> Result<Vec<PaletteIndex>, Unpacked> {
    let bits = bits_per_entry(palette_len);
    let needed = long_count(volume, bits);
    if longs.len() < needed {
        return Err(Unpacked::TooShort { needed });
    }
    let mask = (1_u64 << bits) - 1;
    let width = usize::try_from(bits).expect("at most 32 bits");
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
