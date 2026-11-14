//! Reads a `.mcstructure`, holding each `block_indices` layer as a list of indices, in whatever
//! order the file stores its fields.

use std::io::Read;

use cairn_lang_core::block_array::{Dims, MAX_STRUCTURE_VOLUME, PALETTE_CAPACITY, PaletteIndex};
use cairn_lang_nbt::tag::{Compound, Tag};
use cairn_lang_nbt::{Event, NbtReadError, NbtReader};
use thiserror::Error;

use crate::dims::{read_extents, saturating_volume};
use crate::volume::StoredBlockEntity;

/// The `format_version` read, the only one the game saves.
const FORMAT_VERSION: i32 = 1;

/// A `.mcstructure` as read.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredMcStructure {
    /// `structure_world_origin`: where the structure was saved from in its world.
    pub world_origin: (i32, i32, i32),
    /// The voxels, with `block_position_data`'s block entities.
    pub volume: BedrockVolume,
    /// `structure.entities`, each as stored.
    pub entities: Vec<Compound>,
}

/// The voxels of a `.mcstructure`, in the `(y, z, x)` order [`Dims::index`] lays out.
#[derive(Debug, Clone, PartialEq)]
pub struct BedrockVolume {
    /// Voxel extents.
    pub dims: Dims,
    /// `palette.default.block_palette`, in the file's order.
    pub palette: Vec<BedrockBlock>,
    /// The first `block_indices` layer, `None` where it holds `-1`: a structure void, which a
    /// placement leaves as the world has it.
    pub blocks: Vec<Option<PaletteIndex>>,
    /// The second layer, the water a waterlogged block stands in, `None` where it holds `-1`.
    pub liquids: Vec<Option<PaletteIndex>>,
    /// The block entities, each at the voxel its `block_position_data` key names.
    pub block_entities: Vec<StoredBlockEntity>,
}

/// One `block_palette` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct BedrockBlock {
    /// `name`, the Bedrock id.
    pub name: String,
    /// `states`, each typed as stored.
    pub states: Compound,
    /// `version`, the block-state version it was saved at, which some tools leave out.
    pub version: Option<i32>,
}

/// Why bytes could not be read as a `.mcstructure`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum McStructureReadError {
    /// The bytes are not little-endian NBT.
    #[error("{0}")]
    Nbt(#[from] NbtReadError),
    /// A field is absent, or not the shape the game saves.
    #[error("`{path}` is missing or is not {expected}")]
    Malformed {
        /// The field, as `structure.palette.default.block_palette`.
        path: String,
        /// What the game saves there.
        expected: &'static str,
    },
    /// A `format_version` other than the one the game saves.
    #[error("`.mcstructure` `format_version` {version} is not one this reader reads (1)")]
    UnsupportedFormatVersion {
        /// The version.
        version: i32,
    },
    /// Extents larger than a block array may be.
    #[error("the structure holds {volume} voxels, past the {limit} a block array may hold")]
    TooLarge {
        /// Its voxel count, saturated at `u64::MAX`.
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
    /// A layer holding other than one index per voxel.
    #[error("`block_indices[{layer}]` holds {found} indices; the structure's voxels need {needed}")]
    LayerLength {
        /// The layer, 0 or 1.
        layer: usize,
        /// The indices it holds.
        found: usize,
        /// The voxels.
        needed: usize,
    },
    /// An index past the end of the palette.
    #[error(
        "`block_indices[{layer}]` has palette index {index} at voxel {voxel}, past the palette's \
         {len} entries"
    )]
    PaletteIndexOutOfRange {
        /// The layer, 0 or 1.
        layer: usize,
        /// The voxel, in the file's z-fastest order.
        voxel: usize,
        /// The index.
        index: i32,
        /// The palette's length.
        len: usize,
    },
}

fn malformed(path: impl Into<String>, expected: &'static str) -> McStructureReadError {
    McStructureReadError::Malformed {
        path: path.into(),
        expected,
    }
}

/// The fields read so far, each layer in the file's z-fastest order.
#[derive(Default)]
struct Fields {
    format_version: Option<i32>,
    size: Option<Vec<i32>>,
    world_origin: Option<Vec<i32>>,
    layers: Option<Vec<Vec<i32>>>,
    palette: Option<Compound>,
    entities: Vec<Compound>,
}

/// Reads an uncompressed little-endian `.mcstructure`.
///
/// # Errors
///
/// A [`McStructureReadError`] for bytes that are not a `.mcstructure` this reader reads.
pub fn read_mcstructure<R: Read>(reader: R) -> Result<StoredMcStructure, McStructureReadError> {
    let mut nbt = NbtReader::bedrock(reader).with_max_len(MAX_STRUCTURE_VOLUME);
    nbt.next_event()?;
    let mut fields = Fields::default();
    while let Some(event) = nbt.next_event()? {
        match event {
            Event::Value {
                name: Some(name),
                tag: Tag::Int(value),
            } if name == "format_version" => fields.format_version = Some(value),
            Event::List {
                name: Some(name), ..
            } if name == "size" || name == "structure_world_origin" => {
                let ints = ints(&mut nbt)?;
                if name == "size" {
                    fields.size = ints;
                } else {
                    fields.world_origin = ints;
                }
            }
            Event::Compound { name: Some(name) } if name == "structure" => {
                read_structure(&mut nbt, &mut fields)?;
            }
            Event::Compound { .. } | Event::List { .. } => nbt.skip_rest()?,
            Event::Value { .. } | Event::End => {}
        }
    }
    decode(fields)
}

/// The ints of the open list, or `None` when an item is not one.
fn ints<R: Read>(nbt: &mut NbtReader<R>) -> Result<Option<Vec<i32>>, McStructureReadError> {
    let Tag::List(list) = nbt.read_rest()? else {
        unreachable!("the rest of a list reads as a list");
    };
    Ok(list
        .items
        .iter()
        .map(|item| match item {
            Tag::Int(value) => Some(*value),
            _ => None,
        })
        .collect())
}

/// The entries of the open `structure` compound.
fn read_structure<R: Read>(
    nbt: &mut NbtReader<R>,
    fields: &mut Fields,
) -> Result<(), McStructureReadError> {
    loop {
        match nbt.next_event()? {
            Some(Event::List {
                name: Some(name), ..
            }) => match name.as_str() {
                "block_indices" => fields.layers = Some(read_layers(nbt)?),
                "entities" => {
                    let Tag::List(list) = nbt.read_rest()? else {
                        unreachable!("the rest of a list reads as a list");
                    };
                    fields.entities = list
                        .items
                        .into_iter()
                        .map(|item| match item {
                            Tag::Compound(compound) => Some(compound),
                            _ => None,
                        })
                        .collect::<Option<_>>()
                        .ok_or_else(|| malformed("structure.entities", "a list of compounds"))?;
                }
                _ => nbt.skip_rest()?,
            },
            Some(Event::Compound { name: Some(name) }) if name == "palette" => {
                let Tag::Compound(palette) = nbt.read_rest()? else {
                    unreachable!("the rest of a compound reads as a compound");
                };
                fields.palette = Some(palette);
            }
            Some(Event::Compound { .. } | Event::List { .. }) => nbt.skip_rest()?,
            Some(Event::Value { .. }) => {}
            Some(Event::End) | None => return Ok(()),
        }
    }
}

/// The open `block_indices` list, an index at a time.
fn read_layers<R: Read>(nbt: &mut NbtReader<R>) -> Result<Vec<Vec<i32>>, McStructureReadError> {
    let shape = || malformed("structure.block_indices", "a list of two lists of ints");
    let mut layers = Vec::with_capacity(2);
    loop {
        match nbt.next_event()? {
            Some(Event::List { len, .. }) => {
                let mut layer = Vec::with_capacity(len.min(1 << 16));
                loop {
                    match nbt.next_event()? {
                        Some(Event::Value {
                            tag: Tag::Int(index),
                            ..
                        }) => layer.push(index),
                        Some(Event::End) => break,
                        _ => return Err(shape()),
                    }
                }
                layers.push(layer);
            }
            Some(Event::End) | None => return Ok(layers),
            Some(Event::Compound { .. } | Event::Value { .. }) => return Err(shape()),
        }
    }
}

fn decode(fields: Fields) -> Result<StoredMcStructure, McStructureReadError> {
    let version = fields
        .format_version
        .ok_or_else(|| malformed("format_version", "an int"))?;
    if version != FORMAT_VERSION {
        return Err(McStructureReadError::UnsupportedFormatVersion { version });
    }
    let dims = read_extents(fields.size.as_deref())
        .ok_or_else(|| malformed("size", "a list of three ints, none negative"))?;
    let volume = dims
        .checked_volume()
        .filter(|&volume| volume <= MAX_STRUCTURE_VOLUME)
        .ok_or_else(|| McStructureReadError::TooLarge {
            volume: saturating_volume(dims),
            limit: MAX_STRUCTURE_VOLUME,
        })?;
    let world_origin = match fields.world_origin.as_deref() {
        None => (0, 0, 0),
        Some(&[x, y, z]) => (x, y, z),
        Some(_) => {
            return Err(malformed("structure_world_origin", "a list of three ints"));
        }
    };
    let palette_path = "structure.palette.default";
    let Some(Tag::Compound(default)) = fields
        .palette
        .as_ref()
        .and_then(|p| p.entries.get("default"))
    else {
        return Err(malformed(palette_path, "a compound"));
    };
    let palette = read_palette(default)?;
    let layers = fields
        .layers
        .ok_or_else(|| malformed("structure.block_indices", "a list of two lists of ints"))?;
    let [blocks, liquids] = <[Vec<i32>; 2]>::try_from(layers)
        .map_err(|_| malformed("structure.block_indices", "a list of two lists of ints"))?;
    let blocks = reorder(dims, volume, 0, &blocks, palette.len())?;
    let liquids = reorder(dims, volume, 1, &liquids, palette.len())?;
    let block_entities = match default.entries.get("block_position_data") {
        None => Vec::new(),
        Some(Tag::Compound(data)) => position_data(data, dims)?,
        Some(_) => {
            return Err(malformed(
                format!("{palette_path}.block_position_data"),
                "a compound",
            ));
        }
    };
    Ok(StoredMcStructure {
        world_origin,
        volume: BedrockVolume {
            dims,
            palette,
            blocks,
            liquids,
            block_entities,
        },
        entities: fields.entities,
    })
}

fn read_palette(default: &Compound) -> Result<Vec<BedrockBlock>, McStructureReadError> {
    let path = "structure.palette.default.block_palette";
    let expected = "a list of blocks, each a `name` and its `states`";
    let Some(Tag::List(list)) = default.entries.get("block_palette") else {
        return Err(malformed(path, expected));
    };
    if list.items.len() > PALETTE_CAPACITY {
        return Err(McStructureReadError::PaletteTooLarge {
            len: list.items.len(),
            limit: PALETTE_CAPACITY,
        });
    }
    list.items
        .iter()
        .map(|item| {
            let Tag::Compound(entry) = item else {
                return None;
            };
            let Some(Tag::String(name)) = entry.entries.get("name") else {
                return None;
            };
            let states = match entry.entries.get("states") {
                None => Compound::new(),
                Some(Tag::Compound(states)) => states.clone(),
                Some(_) => return None,
            };
            let version = match entry.entries.get("version") {
                None => None,
                Some(Tag::Int(version)) => Some(*version),
                Some(_) => return None,
            };
            Some(BedrockBlock {
                name: name.clone(),
                states,
                version,
            })
        })
        .collect::<Option<_>>()
        .ok_or_else(|| malformed(path, expected))
}

/// `layer`, stored z fastest, in the `(y, z, x)` order, each `-1` as `None`.
fn reorder(
    dims: Dims,
    volume: usize,
    layer: usize,
    stored: &[i32],
    len: usize,
) -> Result<Vec<Option<PaletteIndex>>, McStructureReadError> {
    if stored.len() != volume {
        return Err(McStructureReadError::LayerLength {
            layer,
            found: stored.len(),
            needed: volume,
        });
    }
    let mut out = vec![None; volume];
    let mut voxel = 0;
    for x in 0..dims.x {
        for y in 0..dims.y {
            for z in 0..dims.z {
                let index = stored[voxel];
                if index != -1 {
                    let slot = usize::try_from(index)
                        .ok()
                        .filter(|&slot| slot < len)
                        .and_then(|slot| u16::try_from(slot).ok())
                        .ok_or(McStructureReadError::PaletteIndexOutOfRange {
                            layer,
                            voxel,
                            index,
                            len,
                        })?;
                    let at = dims.index(x, y, z).expect("a voxel of the structure");
                    out[at] = Some(PaletteIndex(slot));
                }
                voxel += 1;
            }
        }
    }
    Ok(out)
}

/// Each `block_entity_data` of `block_position_data`, at the voxel its key names.
fn position_data(
    data: &Compound,
    dims: Dims,
) -> Result<Vec<StoredBlockEntity>, McStructureReadError> {
    let path = "structure.palette.default.block_position_data";
    let mut out = Vec::new();
    for (key, entry) in &data.entries {
        let position = key
            .parse::<u64>()
            .ok()
            .and_then(|index| {
                let (y_count, z_count) = (u64::from(dims.y), u64::from(dims.z));
                let z = index % z_count.max(1);
                let y = index / z_count.max(1) % y_count.max(1);
                let x = index / z_count.max(1) / y_count.max(1);
                let cell = (
                    u32::try_from(x).ok()?,
                    u32::try_from(y).ok()?,
                    u32::try_from(z).ok()?,
                );
                dims.index(cell.0, cell.1, cell.2)?;
                Some(cell)
            })
            .ok_or_else(|| malformed(format!("{path}.{key}"), "keyed by a voxel's index"))?;
        let Tag::Compound(entry) = entry else {
            return Err(malformed(format!("{path}.{key}"), "a compound"));
        };
        let mut block = match entry.entries.get("block_entity_data") {
            None => continue,
            Some(Tag::Compound(block)) => block.clone(),
            Some(_) => {
                return Err(malformed(
                    format!("{path}.{key}.block_entity_data"),
                    "a compound",
                ));
            }
        };
        let Some(Tag::String(id)) = block.entries.shift_remove("id") else {
            return Err(malformed(
                format!("{path}.{key}.block_entity_data.id"),
                "the block entity's id",
            ));
        };
        for axis in ["x", "y", "z"] {
            block.entries.shift_remove(axis);
        }
        out.push(StoredBlockEntity {
            pos: position,
            id,
            data: block,
        });
    }
    Ok(out)
}
