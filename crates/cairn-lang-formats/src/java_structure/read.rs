//! Reads a Java structure file's `blocks` one entry at a time, holding only each entry's position,
//! palette index and `nbt`, in whatever order the file stores its fields.

use std::collections::HashSet;
use std::io::Read;

use cairn_lang_core::block_array::{
    BlockState, Dims, MAX_STRUCTURE_VOLUME, PALETTE_CAPACITY, PaletteIndex,
};
use cairn_lang_nbt::tag::{Compound, Tag};
use cairn_lang_nbt::{Event, NbtReadError, NbtReader};
use thiserror::Error;

use crate::dims::{read_extents, saturating_volume};
use crate::read_stream::Held;
use crate::volume::MAX_HELD_NBT;
use crate::volume::{BlockVolume, StoredBlockEntity, block_state_from_nbt};

/// A Java structure file as read.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredStructure {
    /// `DataVersion`: the Java `DataVersion` the file was saved for.
    pub data_version: i32,
    /// The voxels, indexing `palette` or the first of `palettes`, with each `nbt` as a block
    /// entity. A cell `blocks` lists nothing at indexes `minecraft:air`, which is added to the
    /// palette when the file's has none.
    pub volume: BlockVolume,
    /// The cells `blocks` lists nothing at, which a placement leaves as the world has them.
    pub void_cells: usize,
    /// How many palettes the file holds: `palettes`' length, or 1 for a file with `palette`.
    pub palettes: usize,
    /// `entities`, each as stored.
    pub entities: Vec<Compound>,
}

/// Why bytes could not be read as a Java structure file.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StructureReadError {
    /// The bytes are not gzip-wrapped NBT.
    #[error("{0}")]
    Nbt(#[from] NbtReadError),
    /// A field is absent, or not the shape the game saves.
    #[error("`{path}` is missing or is not {expected}")]
    Malformed {
        /// The field, as `blocks[3].pos`.
        path: String,
        /// What the game saves there.
        expected: &'static str,
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
    /// A field whose NBT, read whole, would take what the reader holds of the file past its limit.
    #[error("`{path}` holds more NBT than the {limit} bytes a reader holds whole of one file")]
    HeldTooMuch {
        /// The field, as `entities`.
        path: String,
        /// The most memory, in bytes, a reader holds whole of one file.
        limit: usize,
    },
    /// A `blocks` entry whose `state` no palette entry has.
    #[error("`blocks[{block}]` has state {index}, past the palette's {len} entries")]
    PaletteIndexOutOfRange {
        /// The entry's place in `blocks`.
        block: usize,
        /// Its `state`.
        index: i32,
        /// The palette's length.
        len: usize,
    },
    /// A `blocks` entry at a position outside `size`.
    #[error("`blocks[{block}]` is at {x},{y},{z}, outside the structure's {size}")]
    OutsideSize {
        /// The entry's place in `blocks`.
        block: usize,
        /// Its x.
        x: i32,
        /// Its y.
        y: i32,
        /// Its z.
        z: i32,
        /// The extents, as `3x4x5`.
        size: String,
    },
    /// A field of the root stored a second time.
    #[error("`{path}` is stored twice, where a compound holds each name once")]
    StoredTwice {
        /// The field.
        path: String,
    },
    /// Two `blocks` entries at one position.
    #[error("`blocks[{block}]` is at {x},{y},{z}, where an earlier entry already is")]
    Repeated {
        /// The later entry's place in `blocks`.
        block: usize,
        /// Its x.
        x: i32,
        /// Its y.
        y: i32,
        /// Its z.
        z: i32,
    },
}

fn malformed(path: impl Into<String>, expected: &'static str) -> StructureReadError {
    StructureReadError::Malformed {
        path: path.into(),
        expected,
    }
}

fn held_too_much(path: impl Into<String>) -> StructureReadError {
    StructureReadError::HeldTooMuch {
        path: path.into(),
        limit: MAX_HELD_NBT,
    }
}

/// One `blocks` entry, without its NBT.
struct Listed {
    pos: [i32; 3],
    state: i32,
}

/// The fields read so far.
#[derive(Default)]
struct Fields {
    data_version: Option<i32>,
    size: Option<Vec<i32>>,
    palette: Option<Vec<BlockState>>,
    /// The first palette of `palettes`, and how many it holds.
    palettes: Option<(Option<Vec<BlockState>>, usize)>,
    blocks: Option<Vec<Listed>>,
    block_entities: Vec<(usize, Compound)>,
    entities: Vec<Compound>,
}

/// Reads a gzip-wrapped Java structure file.
///
/// # Errors
///
/// A [`StructureReadError`] for bytes that are not a structure file this reader reads.
pub fn read_structure<R: Read>(reader: R) -> Result<StoredStructure, StructureReadError> {
    // As Java's `GZIPInputStream` reads them, members after the first continue the stream.
    let gz = flate2::read::MultiGzDecoder::new(reader);
    let mut nbt = NbtReader::java(gz).with_max_len(MAX_STRUCTURE_VOLUME);
    nbt.next_event()?;
    let mut fields = Fields::default();
    let mut held = Held::new();
    let mut stored: HashSet<String> = HashSet::new();
    while let Some(event) = nbt.next_event()? {
        let name = match &event {
            Event::Compound { name } | Event::List { name, .. } | Event::Value { name, .. } => {
                name.as_deref()
            }
            Event::End => None,
        };
        if let Some(name) = name
            && !stored.insert(name.to_owned())
        {
            return Err(StructureReadError::StoredTwice {
                path: name.to_owned(),
            });
        }
        match event {
            Event::Value {
                name: Some(name),
                tag: Tag::Int(value),
            } if name == "DataVersion" => fields.data_version = Some(value),
            Event::List {
                name: Some(name),
                len,
                ..
            } => match name.as_str() {
                "size" => {
                    let (Tag::List(list), _) =
                        held.read(&mut nbt)?.ok_or_else(|| held_too_much("size"))?
                    else {
                        unreachable!("the rest of a list reads as a list");
                    };
                    let ints = list.items.iter().map(|item| match item {
                        Tag::Int(value) => Some(*value),
                        _ => None,
                    });
                    fields.size = ints.collect();
                }
                "palette" => {
                    fields.palette = Some(read_palette(&mut nbt, &mut held, "palette", len)?);
                }
                "palettes" => fields.palettes = Some(read_palettes(&mut nbt, &mut held)?),
                "blocks" => read_blocks(&mut nbt, &mut held, &mut fields)?,
                "entities" => fields.entities = compounds(&mut nbt, &mut held, "entities")?,
                _ => nbt.skip_rest()?,
            },
            Event::Compound { .. } | Event::List { .. } => nbt.skip_rest()?,
            Event::Value { .. } | Event::End => {}
        }
    }
    nbt.finish()?;
    decode(fields)
}

/// The compounds of the open list `path`, held in `held`.
fn compounds<R: Read>(
    nbt: &mut NbtReader<R>,
    held: &mut Held,
    path: &str,
) -> Result<Vec<Compound>, StructureReadError> {
    let Tag::List(list) = held.rest(nbt)?.ok_or_else(|| held_too_much(path))? else {
        unreachable!("the rest of a list reads as a list");
    };
    list.items
        .into_iter()
        .map(|item| match item {
            Tag::Compound(compound) => Ok(compound),
            _ => Err(malformed(path, "a list of compounds")),
        })
        .collect()
}

/// The block states of the open palette list `path`, of `len` entries.
fn read_palette<R: Read>(
    nbt: &mut NbtReader<R>,
    held: &mut Held,
    path: &str,
    len: usize,
) -> Result<Vec<BlockState>, StructureReadError> {
    if len > PALETTE_CAPACITY {
        return Err(StructureReadError::PaletteTooLarge {
            len,
            limit: PALETTE_CAPACITY,
        });
    }
    compounds(nbt, held, path)?
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            block_state_from_nbt(entry).map_err(|fault| {
                malformed(format!("{path}[{index}].{}", fault.field), fault.expected)
            })
        })
        .collect()
}

/// The open `palettes` list: the first palette read, and how many there are, the others skipped.
fn read_palettes<R: Read>(
    nbt: &mut NbtReader<R>,
    held: &mut Held,
) -> Result<(Option<Vec<BlockState>>, usize), StructureReadError> {
    let (mut first, mut count) = (None, 0);
    loop {
        match nbt.next_event()? {
            Some(Event::List { len, .. }) => {
                if count == 0 {
                    first = Some(read_palette(nbt, held, "palettes[0]", len)?);
                } else {
                    nbt.skip_rest()?;
                }
                count += 1;
            }
            Some(Event::End) | None => return Ok((first, count)),
            Some(Event::Compound { .. } | Event::Value { .. }) => {
                return Err(malformed("palettes", "a list of palettes"));
            }
        }
    }
}

/// The open `blocks` list, an entry at a time.
fn read_blocks<R: Read>(
    nbt: &mut NbtReader<R>,
    held: &mut Held,
    fields: &mut Fields,
) -> Result<(), StructureReadError> {
    let mut blocks = Vec::new();
    loop {
        match nbt.next_event()? {
            Some(Event::Compound { name: None }) => {
                let at = blocks.len();
                let path = || format!("blocks[{at}]");
                let (Tag::Compound(mut entry), taken) =
                    held.read(nbt)?.ok_or_else(|| held_too_much(path()))?
                else {
                    unreachable!("the rest of a compound reads as a compound");
                };
                let Some(&Tag::Int(state)) = entry.entries.get("state") else {
                    return Err(malformed(format!("{}.state", path()), "an int"));
                };
                let pos = match entry.entries.get("pos") {
                    Some(Tag::List(list)) => match list.items.as_slice() {
                        [Tag::Int(x), Tag::Int(y), Tag::Int(z)] => Some([*x, *y, *z]),
                        _ => None,
                    },
                    _ => None,
                }
                .ok_or_else(|| malformed(format!("{}.pos", path()), "a list of three ints"))?;
                match entry.entries.swap_remove("nbt") {
                    None => {}
                    Some(Tag::Compound(data)) => {
                        held.keep(taken);
                        fields.block_entities.push((at, data));
                    }
                    Some(_) => return Err(malformed(format!("{}.nbt", path()), "a compound")),
                }
                blocks.push(Listed { pos, state });
            }
            Some(Event::End) | None => break,
            Some(Event::List { .. } | Event::Value { .. } | Event::Compound { .. }) => {
                return Err(malformed("blocks", "a list of compounds"));
            }
        }
    }
    fields.blocks = Some(blocks);
    Ok(())
}

fn decode(fields: Fields) -> Result<StoredStructure, StructureReadError> {
    let data_version = fields
        .data_version
        .ok_or_else(|| malformed("DataVersion", "an int"))?;
    let size = read_extents(fields.size.as_deref())
        .ok_or_else(|| malformed("size", "a list of three ints, none negative"))?;
    let volume = size
        .checked_volume()
        .filter(|&volume| volume <= MAX_STRUCTURE_VOLUME)
        .ok_or_else(|| StructureReadError::TooLarge {
            volume: saturating_volume(size),
            limit: MAX_STRUCTURE_VOLUME,
        })?;
    // As the game loads a structure, `palettes` wins over `palette` when both are stored.
    let (mut palette, palettes) = match fields.palettes {
        Some((Some(first), count)) => (first, count),
        Some((None, _)) => return Err(malformed("palettes[0]", "a list of block states")),
        None => (
            fields
                .palette
                .ok_or_else(|| malformed("palette", "a list of block states"))?,
            1,
        ),
    };
    let blocks = fields
        .blocks
        .ok_or_else(|| malformed("blocks", "a list of compounds"))?;
    let Placed { slots, cells } = place(blocks, size, volume, palette.len())?;
    let void_cells = slots.iter().filter(|slot| slot.is_none()).count();
    let air = void_air(&mut palette, void_cells)?;
    let voxels = slots.into_iter().map(|slot| slot.unwrap_or(air)).collect();
    let block_entities = fields
        .block_entities
        .into_iter()
        .map(|(block, mut data)| {
            let Some(Tag::String(id)) = data.entries.shift_remove("id") else {
                return Err(malformed(
                    format!("blocks[{block}].nbt.id"),
                    "the block entity's id",
                ));
            };
            Ok(StoredBlockEntity {
                pos: cells[block],
                id,
                data,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(StoredStructure {
        data_version,
        volume: BlockVolume::new(size, palette, voxels, block_entities)
            .expect("each part of the structure is checked as it is decoded"),
        void_cells,
        palettes,
        entities: fields.entities,
    })
}

/// The `blocks` entries placed in the structure's cells.
struct Placed {
    /// Each cell's palette index, `None` where `blocks` lists nothing.
    slots: Vec<Option<PaletteIndex>>,
    /// Each entry's cell, in `blocks` order.
    cells: Vec<(u32, u32, u32)>,
}

fn place(
    blocks: Vec<Listed>,
    size: Dims,
    volume: usize,
    palette_len: usize,
) -> Result<Placed, StructureReadError> {
    let mut slots: Vec<Option<PaletteIndex>> = vec![None; volume];
    let mut cells = Vec::with_capacity(blocks.len());
    for (
        block,
        Listed {
            pos: [x, y, z],
            state,
        },
    ) in blocks.into_iter().enumerate()
    {
        let index = usize::try_from(state)
            .ok()
            .filter(|&index| index < palette_len)
            .ok_or(StructureReadError::PaletteIndexOutOfRange {
                block,
                index: state,
                len: palette_len,
            })?;
        let at = match [x, y, z].map(u32::try_from) {
            [Ok(cx), Ok(cy), Ok(cz)] => size.index(cx, cy, cz).map(|i| (i, (cx, cy, cz))),
            _ => None,
        };
        let Some((i, cell)) = at else {
            return Err(StructureReadError::OutsideSize {
                block,
                x,
                y,
                z,
                size: format!("{}x{}x{}", size.x, size.y, size.z),
            });
        };
        if slots[i].is_some() {
            return Err(StructureReadError::Repeated { block, x, y, z });
        }
        let index = u16::try_from(index).expect("the palette was checked to fit a block array's");
        slots[i] = Some(PaletteIndex(index));
        cells.push(cell);
    }
    Ok(Placed { slots, cells })
}

/// The palette index of air for the `void_cells` cells `blocks` lists nothing at, added to
/// `palette` when it has none.
fn void_air(
    palette: &mut Vec<BlockState>,
    void_cells: usize,
) -> Result<PaletteIndex, StructureReadError> {
    if void_cells == 0 {
        return Ok(PaletteIndex::AIR);
    }
    let fits = "the palette was checked to fit a block array's";
    if let Some(at) = palette
        .iter()
        .position(|state| state.id == BlockState::AIR_ID)
    {
        return Ok(PaletteIndex(u16::try_from(at).expect(fits)));
    }
    if palette.len() == PALETTE_CAPACITY {
        return Err(StructureReadError::PaletteTooLarge {
            len: palette.len() + 1,
            limit: PALETTE_CAPACITY,
        });
    }
    palette.push(BlockState::air());
    Ok(PaletteIndex(u16::try_from(palette.len() - 1).expect(fits)))
}
