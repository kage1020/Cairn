//! Reads a `.schem` of Sponge Schematic version 2 or 3, holding its block data as bytes until the
//! fields that decode it have been read, in whatever order the file stores them.

use std::collections::HashMap;
use std::io::Read;

use cairn_lang_core::block_array::{
    BlockState, Dims, MAX_STRUCTURE_VOLUME, PALETTE_CAPACITY, PaletteIndex,
};
use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{Event, NbtReadError, NbtReader};
use indexmap::IndexMap;
use thiserror::Error;

use super::STATE_SEPARATORS;
use crate::volume::{BlockVolume, StoredBlockEntity};

/// The versions read: 2, the first to carry `DataVersion`, and 3.
const VERSIONS: std::ops::RangeInclusive<i32> = 2..=3;

/// The most bytes a voxel's varint takes for an index below [`PALETTE_CAPACITY`].
const MAX_VARINT_LEN: usize = 3;

/// The most bytes a varint of an `i32` takes.
const MAX_INT_VARINT_LEN: u32 = 5;

/// A `.schem` as read.
#[derive(Debug, Clone, PartialEq)]
pub struct SpongeSchematic {
    /// `Version`, the schematic format's.
    pub version: i32,
    /// `DataVersion`: the Java `DataVersion` the file was saved for.
    pub data_version: i32,
    /// `Metadata`, each field as stored, or empty when the file has none.
    pub metadata: Compound,
    /// `Offset`, the vector a paste moves the schematic by, or the origin when the file has none.
    pub offset: (i32, i32, i32),
    /// The voxels, from the schematic's minimum corner, with its block entities.
    pub volume: BlockVolume,
    /// `Entities`, each as stored.
    pub entities: Vec<Compound>,
}

/// Why bytes could not be read as a `.schem`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SchemReadError {
    /// The bytes are not gzip-wrapped NBT.
    #[error("{0}")]
    Nbt(#[from] NbtReadError),
    /// A field is absent, or not the shape the specification gives it.
    #[error("`{path}` is missing or is not {expected}")]
    Malformed {
        /// The field, as `Schematic.Blocks.Palette`.
        path: String,
        /// What the specification puts there.
        expected: &'static str,
    },
    /// A `Version` outside the ones read.
    #[error(
        "Sponge Schematic `Version` {version} is not one this reader reads (2 or 3); version 1 \
         has no `DataVersion`"
    )]
    UnsupportedVersion {
        /// The version.
        version: i32,
    },
    /// Extents larger than a block array may be.
    #[error("the schematic holds {volume} voxels, past the {limit} a block array may hold")]
    TooLarge {
        /// Its voxel count.
        volume: u64,
        /// The most a block array holds.
        limit: usize,
    },
    /// A palette longer than a block array's.
    #[error("the palette has {len} entries, past the {limit} a block array holds")]
    PaletteTooLarge {
        /// The palette's length.
        len: usize,
        /// The most a block array holds.
        limit: usize,
    },
    /// Block data holding a number of entries other than one per voxel.
    #[error("`{path}` holds {found} varints; the schematic's voxels need {needed}")]
    DataLength {
        /// The block data's field.
        path: &'static str,
        /// The varints it holds.
        found: usize,
        /// The voxels.
        needed: usize,
    },
    /// A voxel whose index no palette entry maps to.
    #[error("voxel {voxel} has palette index {index}, which no palette entry maps to")]
    UnknownPaletteIndex {
        /// The voxel, in `(y, z, x)` order.
        voxel: usize,
        /// The index.
        index: u64,
    },
}

fn malformed(path: impl Into<String>, expected: &'static str) -> SchemReadError {
    SchemReadError::Malformed {
        path: path.into(),
        expected,
    }
}

/// The fields read so far, each as stored.
#[derive(Default)]
struct Fields {
    version: Option<i32>,
    data_version: Option<i32>,
    metadata: Option<Compound>,
    extents: [Option<i16>; 3],
    offset: Option<Vec<i32>>,
    palette: Option<Compound>,
    data: Option<Vec<i8>>,
    block_entities: Option<List>,
    entities: Option<List>,
}

/// Reads a gzip-wrapped `.schem`.
///
/// # Errors
///
/// A [`SchemReadError`] for bytes that are not a `.schem` this reader reads.
pub fn read_schem<R: Read>(reader: R) -> Result<SpongeSchematic, SchemReadError> {
    let gz = flate2::read::GzDecoder::new(reader);
    let max_len = MAX_STRUCTURE_VOLUME.saturating_mul(MAX_VARINT_LEN);
    let mut nbt = NbtReader::java(gz).with_max_len(max_len);
    nbt.next_event()?;
    let mut fields = Fields::default();
    // Version 3 nests its fields in a `Schematic` compound; version 2 keeps them in the root.
    read_entries(&mut nbt, &mut fields, Container::Root)?;
    decode(fields)
}

/// The compound [`read_entries`] is reading the entries of.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    Root,
    Schematic,
    Blocks,
}

fn read_entries<R: Read>(
    nbt: &mut NbtReader<R>,
    fields: &mut Fields,
    container: Container,
) -> Result<(), SchemReadError> {
    while let Some(event) = nbt.next_event()? {
        match event {
            Event::Value {
                name: Some(name),
                tag,
            } => match (name.as_str(), tag) {
                ("Version", Tag::Int(value)) => fields.version = Some(value),
                ("DataVersion", Tag::Int(value)) => fields.data_version = Some(value),
                ("Width", Tag::Short(value)) => fields.extents[0] = Some(value),
                ("Height", Tag::Short(value)) => fields.extents[1] = Some(value),
                ("Length", Tag::Short(value)) => fields.extents[2] = Some(value),
                ("Offset", Tag::IntArray(value)) => fields.offset = Some(value),
                ("BlockData", Tag::ByteArray(value)) if container != Container::Blocks => {
                    fields.data = Some(value);
                }
                ("Data", Tag::ByteArray(value)) if container == Container::Blocks => {
                    fields.data = Some(value);
                }
                _ => {}
            },
            Event::Compound { name: Some(name) } => match name.as_str() {
                "Schematic" if container == Container::Root => {
                    read_entries(nbt, fields, Container::Schematic)?;
                }
                "Blocks" if container != Container::Blocks => {
                    read_entries(nbt, fields, Container::Blocks)?;
                }
                "Metadata" if container != Container::Blocks => {
                    fields.metadata = Some(rest_compound(nbt)?);
                }
                "Palette" => fields.palette = Some(rest_compound(nbt)?),
                _ => nbt.skip_rest()?,
            },
            Event::List {
                name: Some(name), ..
            } => match name.as_str() {
                "BlockEntities" => fields.block_entities = Some(rest_list(nbt)?),
                "Entities" if container != Container::Blocks => {
                    fields.entities = Some(rest_list(nbt)?);
                }
                _ => nbt.skip_rest()?,
            },
            Event::Compound { name: None } | Event::List { name: None, .. } => nbt.skip_rest()?,
            Event::Value { name: None, .. } => {}
            Event::End => return Ok(()),
        }
    }
    Ok(())
}

fn rest_compound<R: Read>(nbt: &mut NbtReader<R>) -> Result<Compound, SchemReadError> {
    let Tag::Compound(compound) = nbt.read_rest()? else {
        unreachable!("the rest of a compound reads as a compound");
    };
    Ok(compound)
}

fn rest_list<R: Read>(nbt: &mut NbtReader<R>) -> Result<List, SchemReadError> {
    let Tag::List(list) = nbt.read_rest()? else {
        unreachable!("the rest of a list reads as a list");
    };
    Ok(list)
}

/// The field paths of one version's layout.
struct Paths {
    prefix: &'static str,
    palette: &'static str,
    data: &'static str,
    block_entities: &'static str,
}

impl Paths {
    fn of(version: i32) -> Self {
        if version >= 3 {
            Self {
                prefix: "Schematic.",
                palette: "Schematic.Blocks.Palette",
                data: "Schematic.Blocks.Data",
                block_entities: "Schematic.Blocks.BlockEntities",
            }
        } else {
            Self {
                prefix: "",
                palette: "Palette",
                data: "BlockData",
                block_entities: "BlockEntities",
            }
        }
    }
}

fn decode(fields: Fields) -> Result<SpongeSchematic, SchemReadError> {
    let version = fields
        .version
        .ok_or_else(|| malformed("Version", "an int"))?;
    if !VERSIONS.contains(&version) {
        return Err(SchemReadError::UnsupportedVersion { version });
    }
    let paths = Paths::of(version);
    let data_version = fields
        .data_version
        .ok_or_else(|| malformed(format!("{}DataVersion", paths.prefix), "an int"))?;
    let mut extents = [0_u32; 3];
    for ((slot, extent), field) in extents
        .iter_mut()
        .zip(fields.extents)
        .zip(["Width", "Height", "Length"])
    {
        let extent =
            extent.ok_or_else(|| malformed(format!("{}{field}", paths.prefix), "a short"))?;
        *slot = u32::from(extent.cast_unsigned());
    }
    let [x, y, z] = extents;
    let dims = Dims { x, y, z };
    let volume = dims
        .checked_volume()
        .filter(|&volume| volume <= MAX_STRUCTURE_VOLUME)
        .ok_or_else(|| SchemReadError::TooLarge {
            volume: u64::from(x) * u64::from(y) * u64::from(z),
            limit: MAX_STRUCTURE_VOLUME,
        })?;
    let offset = match fields.offset.as_deref() {
        None => (0, 0, 0),
        Some(&[x, y, z]) => (x, y, z),
        Some(_) => {
            return Err(malformed(
                format!("{}Offset", paths.prefix),
                "an int array of three",
            ));
        }
    };

    let stored = fields
        .palette
        .ok_or_else(|| malformed(paths.palette, "a compound of block states to indices"))?;
    let (palette, lookup) = read_palette(&stored, paths.palette)?;
    let data = fields
        .data
        .ok_or_else(|| malformed(paths.data, "a byte array"))?;
    let voxels = decode_data(&data, &lookup, volume, paths.data)?;

    let block_entities = match fields.block_entities {
        None => Vec::new(),
        Some(list) => list
            .items
            .iter()
            .map(|item| stored_block_entity(item, version, dims))
            .collect::<Option<_>>()
            .ok_or_else(|| {
                malformed(
                    paths.block_entities,
                    "a list of block entities, each an `Id` and a `Pos` inside the schematic",
                )
            })?,
    };
    let entities = match fields.entities {
        None => Vec::new(),
        Some(list) => list
            .items
            .into_iter()
            .map(|item| match item {
                Tag::Compound(compound) => Some(compound),
                _ => None,
            })
            .collect::<Option<_>>()
            .ok_or_else(|| malformed(format!("{}Entities", paths.prefix), "a list of compounds"))?,
    };
    Ok(SpongeSchematic {
        version,
        data_version,
        metadata: fields.metadata.unwrap_or_default(),
        offset,
        volume: BlockVolume {
            dims,
            palette,
            voxels,
            block_entities,
        },
        entities,
    })
}

/// The palette in index order, and each stored index's slot in it.
fn read_palette(
    stored: &Compound,
    path: &str,
) -> Result<(Vec<BlockState>, HashMap<i32, PaletteIndex>), SchemReadError> {
    if stored.entries.len() > PALETTE_CAPACITY {
        return Err(SchemReadError::PaletteTooLarge {
            len: stored.entries.len(),
            limit: PALETTE_CAPACITY,
        });
    }
    let mut indexed = Vec::with_capacity(stored.entries.len());
    for (spelled, index) in &stored.entries {
        let entry_path = || format!("{path}.{spelled}");
        let Tag::Int(index) = index else {
            return Err(malformed(entry_path(), "an int index"));
        };
        let state = parse_state(spelled).ok_or_else(|| {
            malformed(
                entry_path(),
                "keyed by a block state, `namespace:id[name=value,...]`",
            )
        })?;
        indexed.push((*index, state));
    }
    indexed.sort_by_key(|&(index, _)| index);
    let mut lookup = HashMap::with_capacity(indexed.len());
    let mut palette = Vec::with_capacity(indexed.len());
    for (slot, (index, state)) in indexed.into_iter().enumerate() {
        let slot = u16::try_from(slot).expect("the palette was checked to fit a block array's");
        if lookup.insert(index, PaletteIndex(slot)).is_some() {
            return Err(malformed(path, "a compound that maps each index once"));
        }
        palette.push(state);
    }
    Ok((palette, lookup))
}

/// `spelled`, as `namespace:id[name=value,...]` spells a state, with `minecraft` for a namespace
/// it leaves out.
fn parse_state(spelled: &str) -> Option<BlockState> {
    let (id, body) = match spelled.split_once('[') {
        None => (spelled, ""),
        Some((id, rest)) => (id, rest.strip_suffix(']')?),
    };
    if id.is_empty() {
        return None;
    }
    let id = if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    };
    let mut properties = IndexMap::new();
    if !body.is_empty() {
        for pair in body.split(',') {
            let (name, value) = pair.split_once('=')?;
            if name.is_empty()
                || name.contains(STATE_SEPARATORS)
                || value.contains(STATE_SEPARATORS)
            {
                return None;
            }
            if properties
                .insert(name.to_owned(), value.to_owned())
                .is_some()
            {
                return None;
            }
        }
    }
    Some(BlockState { id, properties })
}

/// The `volume` voxels `data` encodes, each a varint the palette maps to a slot.
fn decode_data(
    data: &[i8],
    lookup: &HashMap<i32, PaletteIndex>,
    volume: usize,
    path: &'static str,
) -> Result<Vec<PaletteIndex>, SchemReadError> {
    if data.last().is_some_and(|byte| *byte < 0) {
        return Err(malformed(path, "an array of whole varints"));
    }
    let found = data.iter().filter(|byte| **byte >= 0).count();
    if found != volume {
        return Err(SchemReadError::DataLength {
            path,
            found,
            needed: volume,
        });
    }
    let mut voxels = Vec::with_capacity(volume);
    let (mut value, mut shift) = (0_u64, 0_u32);
    for &byte in data {
        let byte = byte.cast_unsigned();
        if shift == 7 * MAX_INT_VARINT_LEN {
            return Err(malformed(path, "an array of varints of at most five bytes"));
        }
        value |= u64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            let voxel = voxels.len();
            let slot = i32::try_from(value)
                .ok()
                .and_then(|index| lookup.get(&index))
                .ok_or(SchemReadError::UnknownPaletteIndex {
                    voxel,
                    index: value,
                })?;
            voxels.push(*slot);
            (value, shift) = (0, 0);
        }
    }
    Ok(voxels)
}

/// A `BlockEntities` item: an `Id` and `Pos`, its fields in `Data` from version 3 and beside
/// them before it.
fn stored_block_entity(item: &Tag, version: i32, dims: Dims) -> Option<StoredBlockEntity> {
    let Tag::Compound(stored) = item else {
        return None;
    };
    let Some(Tag::String(id)) = stored.entries.get("Id") else {
        return None;
    };
    let Some(Tag::IntArray(pos)) = stored.entries.get("Pos") else {
        return None;
    };
    let [x, y, z] = pos.as_slice() else {
        return None;
    };
    let pos = (
        u32::try_from(*x).ok()?,
        u32::try_from(*y).ok()?,
        u32::try_from(*z).ok()?,
    );
    dims.index(pos.0, pos.1, pos.2)?;
    let data = if version >= 3 {
        match stored.entries.get("Data") {
            None => Compound::new(),
            Some(Tag::Compound(data)) => data.clone(),
            Some(_) => return None,
        }
    } else {
        let mut data = stored.clone();
        data.entries.shift_remove("Id");
        data.entries.shift_remove("Pos");
        data
    };
    Some(StoredBlockEntity {
        pos,
        id: id.clone(),
        data,
    })
}
