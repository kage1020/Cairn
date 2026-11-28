//! Reads a `.schem` of Sponge Schematic version 2 or 3, held to the layout its `Version` gives it
//! (`spec/ecosystem-interop` "Sponge Schematic"). The block data is held as bytes until the fields
//! that decode it have been read, in whatever order the file stores them.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::io::Read;

use cairn_lang_core::block_array::{
    BlockState, Dims, MAX_STRUCTURE_VOLUME, PALETTE_CAPACITY, PaletteIndex,
};
use cairn_lang_nbt::tag::{Compound, Tag};
use cairn_lang_nbt::{Event, NbtReadError, NbtReader};
use indexmap::IndexMap;
use thiserror::Error;

use super::STATE_SEPARATORS;
use crate::read_stream::{Held, lists_compounds, next_inside};
use crate::volume::MAX_HELD_NBT;
use crate::volume::{BlockVolume, StoredBlockEntity};

/// The versions read: 2, the first to carry `DataVersion`, and 3.
const VERSIONS: std::ops::RangeInclusive<i32> = 2..=3;

/// The most bytes a voxel's varint takes for an index below [`PALETTE_CAPACITY`].
const MAX_VARINT_LEN: usize = 3;

/// The most bytes a varint of an `i32` takes.
const MAX_INT_VARINT_LEN: u32 = 5;

/// What block data holding a varint past an `i32` is refused as.
const INT_VARINTS: &str = "an array of varints, each an int of at most five bytes";

/// A `.schem` as read.
#[derive(Debug, Clone, PartialEq)]
pub struct SpongeSchematic {
    /// `Version`, the schematic format's.
    pub version: i32,
    /// `DataVersion`: the Java `DataVersion` the file was saved for.
    pub data_version: i32,
    /// `Metadata`, each field as stored, or empty when the file has none.
    pub metadata: Compound,
    /// `Offset` as stored, or the origin when the file has none: the vector a paste moves the
    /// schematic by in version 3, and in a version 2 file whose `Metadata` holds that vector as
    /// `WEOffsetX`, `WEOffsetY` and `WEOffsetZ`, the minimum corner of the region it was copied from.
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
    /// The bytes are not one gzip-wrapped NBT root this reader holds.
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
    /// A field whose NBT, read whole, would take what the reader holds of the file past its limit.
    #[error("`{path}` holds more NBT than the {limit} bytes a reader holds whole of one file")]
    HeldTooMuch {
        /// The field, as `Schematic.Entities[0]`.
        path: String,
        /// The most memory, in bytes, a reader holds whole of one file.
        limit: usize,
    },
    /// A field away from where the file's `Version` keeps it.
    #[error("`{path}` is not where a version {version} `.schem` keeps it, which is `{belongs}`")]
    Misplaced {
        /// The field, as stored, such as `Schematic.Palette`.
        path: String,
        /// The file's `Version`.
        version: i32,
        /// Where that version keeps it, such as `Schematic.Blocks.Palette`.
        belongs: String,
    },
    /// A field, or a `Palette` key, stored a second time.
    #[error("`{path}` is stored twice, where a compound holds each name once")]
    Repeated {
        /// The field.
        path: String,
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
    /// Two `Palette` keys that spell one block state.
    #[error(
        "`{path}` spells one block state as both `{first}` and `{second}`, where each state takes \
         one index"
    )]
    DuplicateState {
        /// The palette's field.
        path: &'static str,
        /// The key stored first.
        first: String,
        /// The key stored after it.
        second: String,
    },
    /// Two `Palette` keys that map to one index.
    #[error(
        "`{path}` maps both `{first}` and `{second}` to index {index}, where an index names one \
         block state"
    )]
    SharedIndex {
        /// The palette's field.
        path: &'static str,
        /// The index.
        index: i32,
        /// The key stored first.
        first: String,
        /// The key stored after it.
        second: String,
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
        index: i32,
    },
    /// A block entity at the voxel of an earlier one.
    #[error(
        "`{path}` is block entity {first}'s position too, where a voxel holds one block entity"
    )]
    DuplicatePosition {
        /// The later block entity's `Pos`, as `Schematic.Blocks.BlockEntities[3].Pos`.
        path: String,
        /// The earlier block entity's index.
        first: usize,
    },
}

fn held_too_much(path: impl Into<String>) -> SchemReadError {
    SchemReadError::HeldTooMuch {
        path: path.into(),
        limit: MAX_HELD_NBT,
    }
}

fn malformed(path: impl Into<String>, expected: &'static str) -> SchemReadError {
    SchemReadError::Malformed {
        path: path.into(),
        expected,
    }
}

/// Where a version keeps its fields.
#[derive(Clone, Copy)]
enum Layout {
    /// Version 2: every field in the root.
    V2,
    /// Version 3: the fields in the root's `Schematic`, the blocks' in its `Blocks`.
    V3,
}

/// A field the reader reads, which each version keeps at one path.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Field {
    Schematic,
    Blocks,
    Version,
    DataVersion,
    Metadata,
    Width,
    Height,
    Length,
    Offset,
    Palette,
    BlockData,
    BlockEntities,
    Entities,
}

impl Field {
    /// The field an entry `name` of `container` is, or `None` for a name the reader skips.
    fn named(name: &str, container: Container) -> Option<Self> {
        if container.holds_blocks() {
            return match name {
                "Palette" => Some(Self::Palette),
                "Data" => Some(Self::BlockData),
                "BlockEntities" => Some(Self::BlockEntities),
                _ => None,
            };
        }
        Some(match name {
            "Schematic" if container == Container::Root => Self::Schematic,
            "Blocks" => Self::Blocks,
            "Version" => Self::Version,
            "DataVersion" => Self::DataVersion,
            "Metadata" => Self::Metadata,
            "Width" => Self::Width,
            "Height" => Self::Height,
            "Length" => Self::Length,
            "Offset" => Self::Offset,
            "Palette" => Self::Palette,
            "BlockData" => Self::BlockData,
            "BlockEntities" => Self::BlockEntities,
            "Entities" => Self::Entities,
            _ => return None,
        })
    }

    /// Where `layout` keeps the field, or `None` for a compound it has no place for.
    fn path(self, layout: Layout) -> Option<&'static str> {
        let [v2, v3] = match self {
            Self::Schematic => [None, Some("Schematic")],
            Self::Blocks => [None, Some("Schematic.Blocks")],
            Self::Version => [Some("Version"), Some("Schematic.Version")],
            Self::DataVersion => [Some("DataVersion"), Some("Schematic.DataVersion")],
            Self::Metadata => [Some("Metadata"), Some("Schematic.Metadata")],
            Self::Width => [Some("Width"), Some("Schematic.Width")],
            Self::Height => [Some("Height"), Some("Schematic.Height")],
            Self::Length => [Some("Length"), Some("Schematic.Length")],
            Self::Offset => [Some("Offset"), Some("Schematic.Offset")],
            Self::Palette => [Some("Palette"), Some("Schematic.Blocks.Palette")],
            Self::BlockData => [Some("BlockData"), Some("Schematic.Blocks.Data")],
            Self::BlockEntities => [
                Some("BlockEntities"),
                Some("Schematic.Blocks.BlockEntities"),
            ],
            Self::Entities => [Some("Entities"), Some("Schematic.Entities")],
        };
        match layout {
            Layout::V2 => v2,
            Layout::V3 => v3,
        }
    }

    /// Where `layout` keeps a field every version has.
    fn place(self, layout: Layout) -> &'static str {
        self.path(layout)
            .expect("every version keeps each field but `Schematic` and `Blocks`")
    }

    /// The type the specification gives the field.
    fn expected(self) -> &'static str {
        match self {
            Self::Schematic | Self::Blocks | Self::Metadata => "a compound",
            Self::Version | Self::DataVersion => "an int",
            Self::Width | Self::Height | Self::Length => "a short",
            Self::Offset => "an int array of three",
            Self::Palette => "a compound of block states to indices",
            Self::BlockData => "a byte array",
            Self::BlockEntities | Self::Entities => "a list of compounds",
        }
    }
}

/// The compound [`read_entries`] is reading the entries of.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    Root,
    Schematic,
    RootBlocks,
    SchematicBlocks,
}

impl Container {
    /// What a path to one of its entries starts with.
    fn prefix(self) -> &'static str {
        match self {
            Self::Root => "",
            Self::Schematic => "Schematic.",
            Self::RootBlocks => "Blocks.",
            Self::SchematicBlocks => "Schematic.Blocks.",
        }
    }

    /// Whether it is a `Blocks` compound, which names the block data `Data`.
    fn holds_blocks(self) -> bool {
        matches!(self, Self::RootBlocks | Self::SchematicBlocks)
    }
}

/// One field as the file stored it.
struct Stored {
    field: Field,
    path: String,
    /// Whether its tag is the type the specification gives it.
    typed: bool,
}

/// The fields read so far: each one stored, in file order, and the value of the first of each
/// that had its type.
#[derive(Default)]
struct Fields {
    stored: Vec<Stored>,
    version: Option<i32>,
    data_version: Option<i32>,
    metadata: Option<Compound>,
    extents: [Option<i16>; 3],
    offset: Option<Vec<i32>>,
    palette: Option<StoredPalette>,
    data: Option<Vec<i8>>,
    block_entities: Option<Result<Vec<ReadBlockEntity>, ListFault>>,
    entities: Option<Result<Vec<Compound>, ListFault>>,
}

impl Fields {
    fn record(&mut self, field: Field, container: Container, name: &str, typed: bool) {
        self.stored.push(Stored {
            field,
            path: format!("{}{name}", container.prefix()),
            typed,
        });
    }

    /// Holds `tag` as `field`'s value when it has the field's type, and returns whether it does.
    fn hold_value(&mut self, field: Field, tag: Tag) -> bool {
        match (field, tag) {
            (Field::Version, Tag::Int(value)) => _ = self.version.get_or_insert(value),
            (Field::DataVersion, Tag::Int(value)) => _ = self.data_version.get_or_insert(value),
            (Field::Width, Tag::Short(value)) => _ = self.extents[0].get_or_insert(value),
            (Field::Height, Tag::Short(value)) => _ = self.extents[1].get_or_insert(value),
            (Field::Length, Tag::Short(value)) => _ = self.extents[2].get_or_insert(value),
            (Field::Offset, Tag::IntArray(value)) => _ = self.offset.get_or_insert(value),
            (Field::BlockData, Tag::ByteArray(value)) => _ = self.data.get_or_insert(value),
            _ => return false,
        }
        true
    }

    /// Whether `field` was stored with a tag of another type.
    fn mistyped(&self, field: Field) -> bool {
        self.stored
            .iter()
            .any(|stored| stored.field == field && !stored.typed)
    }
}

/// A `Palette` as it streamed: each spelling with its index, `None` for a value that is no int,
/// held up to [`PALETTE_CAPACITY`] entries and counted past them.
struct StoredPalette {
    entries: IndexMap<String, Option<i32>>,
    len: usize,
    /// The first key stored a second time.
    repeated: Option<String>,
}

/// A `BlockEntities` item whose `Id` and `Pos` read, with its other fields as stored.
struct ReadBlockEntity {
    pos: (u32, u32, u32),
    id: String,
    rest: Compound,
}

/// The first part of a list that is not the shape the specification gives it.
struct ListFault {
    /// The item and its field, or `None` for the list itself.
    item: Option<(usize, &'static str)>,
    expected: &'static str,
}

impl ListFault {
    /// The refusal of the list at `path`.
    fn at(&self, path: &str) -> SchemReadError {
        match self.item {
            None => malformed(path, self.expected),
            Some((index, field)) => malformed(format!("{path}[{index}].{field}"), self.expected),
        }
    }
}

/// Reads a gzip-wrapped `.schem`.
///
/// # Errors
///
/// A [`SchemReadError`] for bytes that are not a `.schem` this reader reads.
pub fn read_schem<R: Read>(reader: R) -> Result<SpongeSchematic, SchemReadError> {
    // As Java's `GZIPInputStream` reads them, members after the first continue the stream.
    let gz = flate2::read::MultiGzDecoder::new(reader);
    let mut nbt = NbtReader::java(gz)
        .with_max_len(MAX_STRUCTURE_VOLUME)
        .with_max_array_len(MAX_STRUCTURE_VOLUME.saturating_mul(MAX_VARINT_LEN));
    nbt.next_event()?;
    let mut fields = Fields::default();
    read_entries(&mut nbt, &mut Held::new(), &mut fields, Container::Root)?;
    nbt.finish()?;
    decode(fields)
}

/// Reads the entries of the open `container`, recording each field either version names.
fn read_entries<R: Read>(
    nbt: &mut NbtReader<R>,
    held: &mut Held,
    fields: &mut Fields,
    container: Container,
) -> Result<(), SchemReadError> {
    loop {
        match next_inside(nbt)? {
            Event::Value {
                name: Some(name),
                tag,
            } => {
                if let Some(field) = Field::named(&name, container) {
                    let typed = fields.hold_value(field, tag);
                    fields.record(field, container, &name, typed);
                }
            }
            Event::Compound { name: Some(name) } => {
                let Some(field) = Field::named(&name, container) else {
                    nbt.skip_rest()?;
                    continue;
                };
                let typed = matches!(
                    field,
                    Field::Schematic | Field::Blocks | Field::Metadata | Field::Palette
                );
                fields.record(field, container, &name, typed);
                match field {
                    Field::Schematic => read_entries(nbt, held, fields, Container::Schematic)?,
                    Field::Blocks if container == Container::Root => {
                        read_entries(nbt, held, fields, Container::RootBlocks)?;
                    }
                    Field::Blocks => {
                        read_entries(nbt, held, fields, Container::SchematicBlocks)?;
                    }
                    Field::Metadata if fields.metadata.is_none() => {
                        let metadata = held.rest_of_compound(nbt)?;
                        fields.metadata = Some(metadata.ok_or_else(|| {
                            held_too_much(format!("{}{name}", container.prefix()))
                        })?);
                    }
                    Field::Palette if fields.palette.is_none() => {
                        fields.palette = Some(read_palette_entries(nbt)?);
                    }
                    _ => nbt.skip_rest()?,
                }
            }
            Event::List {
                name: Some(name),
                element_type_id,
                ..
            } => {
                let Some(field) = Field::named(&name, container) else {
                    nbt.skip_rest()?;
                    continue;
                };
                let typed = matches!(field, Field::BlockEntities | Field::Entities);
                fields.record(field, container, &name, typed);
                let path = format!("{}{name}", container.prefix());
                match field {
                    Field::BlockEntities if fields.block_entities.is_none() => {
                        fields.block_entities = Some(each_compound(
                            nbt,
                            held,
                            &path,
                            element_type_id,
                            read_block_entity,
                        )?);
                    }
                    Field::Entities if fields.entities.is_none() => {
                        fields.entities =
                            Some(each_compound(nbt, held, &path, element_type_id, Ok)?);
                    }
                    _ => nbt.skip_rest()?,
                }
            }
            Event::End => return Ok(()),
            event => unreachable!("a compound's entries are named, got {event:?}"),
        }
    }
}

/// The entries of the open `Palette` compound, none held past [`PALETTE_CAPACITY`].
fn read_palette_entries<R: Read>(nbt: &mut NbtReader<R>) -> Result<StoredPalette, NbtReadError> {
    let mut palette = StoredPalette {
        entries: IndexMap::new(),
        len: 0,
        repeated: None,
    };
    loop {
        let (name, index) = match next_inside(nbt)? {
            Event::Value {
                name: Some(name),
                tag,
            } => {
                let index = match tag {
                    Tag::Int(index) => Some(index),
                    _ => None,
                };
                (name, index)
            }
            Event::Compound { name: Some(name) }
            | Event::List {
                name: Some(name), ..
            } => {
                nbt.skip_rest()?;
                (name, None)
            }
            Event::End => return Ok(palette),
            event => unreachable!("a compound's entries are named, got {event:?}"),
        };
        palette.len += 1;
        if palette.len <= PALETTE_CAPACITY {
            match palette.entries.entry(name) {
                indexmap::map::Entry::Occupied(first) => {
                    palette.repeated.get_or_insert_with(|| first.key().clone());
                }
                indexmap::map::Entry::Vacant(slot) => _ = slot.insert(index),
            }
        }
    }
}

/// The items of the open list `path`, each a compound kept in `held` that `item` reads, or the
/// first one it refuses, with the rest of the list skipped.
fn each_compound<R: Read, T>(
    nbt: &mut NbtReader<R>,
    held: &mut Held,
    path: &str,
    element_type_id: u8,
    item: impl Fn(Compound) -> Result<T, (&'static str, &'static str)>,
) -> Result<Result<Vec<T>, ListFault>, SchemReadError> {
    if !lists_compounds(element_type_id) {
        nbt.skip_rest()?;
        return Ok(Err(ListFault {
            item: None,
            expected: "a list of compounds",
        }));
    }
    let mut items = Vec::new();
    loop {
        match next_inside(nbt)? {
            Event::Compound { name: None } => match item(
                held.rest_of_compound(nbt)?
                    .ok_or_else(|| held_too_much(format!("{path}[{}]", items.len())))?,
            ) {
                Ok(read) => items.push(read),
                Err((field, expected)) => {
                    let fault = ListFault {
                        item: Some((items.len(), field)),
                        expected,
                    };
                    nbt.skip_rest()?;
                    return Ok(Err(fault));
                }
            },
            Event::End => return Ok(Ok(items)),
            event => unreachable!("a list of compounds holds only compounds, got {event:?}"),
        }
    }
}

/// A `BlockEntities` item's `Id` and `Pos`, or the field of it that is not the shape it must be.
fn read_block_entity(
    mut stored: Compound,
) -> Result<ReadBlockEntity, (&'static str, &'static str)> {
    let Some(Tag::String(id)) = stored.entries.shift_remove("Id") else {
        return Err(("Id", "a block entity id string"));
    };
    let Some(Tag::IntArray(pos)) = stored.entries.shift_remove("Pos") else {
        return Err(("Pos", "an int array of three"));
    };
    let &[x, y, z] = pos.as_slice() else {
        return Err(("Pos", "an int array of three"));
    };
    let [Ok(x), Ok(y), Ok(z)] = [x, y, z].map(u32::try_from) else {
        return Err(("Pos", "three ints, each 0 or more"));
    };
    Ok(ReadBlockEntity {
        pos: (x, y, z),
        id,
        rest: stored,
    })
}

/// The file's `Version`: the first stored, wherever it is.
fn read_version(fields: &Fields) -> Result<i32, SchemReadError> {
    let Some(first) = fields
        .stored
        .iter()
        .find(|stored| stored.field == Field::Version)
    else {
        let nested = fields
            .stored
            .iter()
            .any(|stored| stored.field == Field::Schematic);
        let path = if nested {
            "Schematic.Version"
        } else {
            "Version"
        };
        return Err(malformed(path, Field::Version.expected()));
    };
    let version = fields
        .version
        .filter(|_| first.typed)
        .ok_or_else(|| malformed(first.path.clone(), Field::Version.expected()))?;
    if !VERSIONS.contains(&version) {
        return Err(SchemReadError::UnsupportedVersion { version });
    }
    Ok(version)
}

/// Refuses a field away from where `layout` keeps it or stored twice, and a `Schematic` or
/// `Blocks` that is not a compound, in the order the file stores them.
fn check_layout(stored: &[Stored], version: i32, layout: Layout) -> Result<(), SchemReadError> {
    let mut placed = HashSet::with_capacity(stored.len());
    for stored in stored {
        let Some(belongs) = stored.field.path(layout) else {
            continue;
        };
        if stored.path != belongs {
            return Err(SchemReadError::Misplaced {
                path: stored.path.clone(),
                version,
                belongs: belongs.to_owned(),
            });
        }
        if !placed.insert(stored.field) {
            return Err(SchemReadError::Repeated {
                path: stored.path.clone(),
            });
        }
        if !stored.typed && matches!(stored.field, Field::Schematic | Field::Blocks) {
            return Err(malformed(belongs, stored.field.expected()));
        }
    }
    Ok(())
}

fn decode(mut fields: Fields) -> Result<SpongeSchematic, SchemReadError> {
    let version = read_version(&fields)?;
    let layout = if version >= 3 { Layout::V3 } else { Layout::V2 };
    check_layout(&fields.stored, version, layout)?;
    // Each field is now stored at most once, where `layout` keeps it, so a value missing from its
    // slot was either left out or of another type.
    let refused = |field: Field| malformed(field.place(layout), field.expected());

    let data_version = fields
        .data_version
        .ok_or_else(|| refused(Field::DataVersion))?;
    let metadata = match fields.metadata.take() {
        Some(metadata) => metadata,
        None if fields.mistyped(Field::Metadata) => return Err(refused(Field::Metadata)),
        None => Compound::new(),
    };
    let mut extents = [0_u32; 3];
    for ((slot, extent), field) in
        extents
            .iter_mut()
            .zip(fields.extents)
            .zip([Field::Width, Field::Height, Field::Length])
    {
        *slot = u32::from(extent.ok_or_else(|| refused(field))?.cast_unsigned());
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
        None if !fields.mistyped(Field::Offset) => (0, 0, 0),
        Some(&[x, y, z]) => (x, y, z),
        _ => return Err(refused(Field::Offset)),
    };

    let stored = fields
        .palette
        .take()
        .ok_or_else(|| refused(Field::Palette))?;
    let (palette, lookup) = read_palette(stored, Field::Palette.place(layout))?;
    let data = fields
        .data
        .take()
        .ok_or_else(|| refused(Field::BlockData))?;
    let voxels = decode_data(&data, &lookup, volume, Field::BlockData.place(layout))?;

    let block_entities = match fields.block_entities.take() {
        None if fields.mistyped(Field::BlockEntities) => return Err(refused(Field::BlockEntities)),
        None => Vec::new(),
        Some(read) => {
            let path = Field::BlockEntities.place(layout);
            let read = read.map_err(|fault| fault.at(path))?;
            stored_block_entities(read, version, layout, dims, path)?
        }
    };
    let entities = match fields.entities.take() {
        None if fields.mistyped(Field::Entities) => return Err(refused(Field::Entities)),
        None => Vec::new(),
        Some(read) => read.map_err(|fault| fault.at(Field::Entities.place(layout)))?,
    };
    let volume = BlockVolume::new(dims, palette, voxels, block_entities)
        .expect("each part of the schematic is checked as it is decoded");
    Ok(SpongeSchematic {
        version,
        data_version,
        metadata,
        offset,
        volume,
        entities,
    })
}

/// The palette in index order, and each stored index's slot in it.
fn read_palette(
    stored: StoredPalette,
    path: &'static str,
) -> Result<(Vec<BlockState>, HashMap<i32, PaletteIndex>), SchemReadError> {
    if stored.len > PALETTE_CAPACITY {
        return Err(SchemReadError::PaletteTooLarge {
            len: stored.len,
            limit: PALETTE_CAPACITY,
        });
    }
    if let Some(key) = stored.repeated {
        return Err(SchemReadError::Repeated {
            path: format!("{path}.{key}"),
        });
    }
    let mut indexed = Vec::with_capacity(stored.entries.len());
    let mut spelled_by: HashMap<_, String> = HashMap::with_capacity(stored.entries.len());
    for (spelled, index) in stored.entries {
        let index = match index {
            None => return Err(malformed(format!("{path}.{spelled}"), "an int index")),
            Some(index) if index < 0 => {
                return Err(malformed(
                    format!("{path}.{spelled}"),
                    "an int index, 0 or more",
                ));
            }
            Some(index) => index,
        };
        let Some(state) = parse_state(&spelled) else {
            return Err(malformed(
                format!("{path}.{spelled}"),
                "keyed by a block state, `namespace:id[name=value,...]`",
            ));
        };
        let (id, properties) = state.canonical_key();
        let canonical: (String, Vec<(String, String)>) =
            (id.to_owned(), properties.into_iter().map(owned).collect());
        match spelled_by.entry(canonical) {
            Entry::Occupied(first) => {
                return Err(SchemReadError::DuplicateState {
                    path,
                    first: first.get().clone(),
                    second: spelled,
                });
            }
            Entry::Vacant(slot) => _ = slot.insert(spelled.clone()),
        }
        indexed.push((index, spelled, state));
    }
    indexed.sort_by_key(|&(index, ..)| index);
    if let Some(pair) = indexed.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(SchemReadError::SharedIndex {
            path,
            index: pair[0].0,
            first: pair[0].1.clone(),
            second: pair[1].1.clone(),
        });
    }
    let mut lookup = HashMap::with_capacity(indexed.len());
    let mut palette = Vec::with_capacity(indexed.len());
    for (slot, (index, _, state)) in indexed.into_iter().enumerate() {
        let slot = u16::try_from(slot).expect("the palette was checked to fit a block array's");
        lookup.insert(index, PaletteIndex(slot));
        palette.push(state);
    }
    Ok((palette, lookup))
}

fn owned((name, value): (&str, &str)) -> (String, String) {
    (name.to_owned(), value.to_owned())
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

/// The `volume` voxels `data` encodes, each a varint holding an `i32` the palette maps to a slot.
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
            return Err(malformed(path, INT_VARINTS));
        }
        value |= u64::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            let index = u32::try_from(value)
                .map_err(|_| malformed(path, INT_VARINTS))?
                .cast_signed();
            let voxel = voxels.len();
            let slot = lookup
                .get(&index)
                .ok_or(SchemReadError::UnknownPaletteIndex { voxel, index })?;
            voxels.push(*slot);
            (value, shift) = (0, 0);
        }
    }
    Ok(voxels)
}

/// The `BlockEntities` items as the volume holds them: each at a voxel of `dims` no earlier one
/// takes, its fields in `Data` from version 3 and beside its `Id` and `Pos` before it.
fn stored_block_entities(
    read: Vec<ReadBlockEntity>,
    version: i32,
    layout: Layout,
    dims: Dims,
    path: &str,
) -> Result<Vec<StoredBlockEntity>, SchemReadError> {
    let mut first_at = HashMap::with_capacity(read.len());
    let mut block_entities = Vec::with_capacity(read.len());
    for (index, read) in read.into_iter().enumerate() {
        let item = format!("{path}[{index}]");
        let (x, y, z) = read.pos;
        let Some(voxel) = dims.index(x, y, z) else {
            return Err(malformed(
                format!("{item}.Pos"),
                "a voxel inside the schematic's `Width`, `Height` and `Length`",
            ));
        };
        if let Some(&first) = first_at.get(&voxel) {
            return Err(SchemReadError::DuplicatePosition {
                path: format!("{item}.Pos"),
                first,
            });
        }
        first_at.insert(voxel, index);
        let data = match layout {
            Layout::V2 => read.rest,
            Layout::V3 => kept_in_data(read.rest, version, &item)?,
        };
        block_entities.push(StoredBlockEntity {
            pos: read.pos,
            id: read.id,
            data,
        });
    }
    Ok(block_entities)
}

/// A version 3 block entity's `Data`, from the fields its `item` holds beside `Id` and `Pos`.
fn kept_in_data(mut rest: Compound, version: i32, item: &str) -> Result<Compound, SchemReadError> {
    let data = match rest.entries.shift_remove("Data") {
        None => Compound::new(),
        Some(Tag::Compound(data)) => data,
        Some(_) => return Err(malformed(format!("{item}.Data"), "a compound")),
    };
    if let Some(key) = rest.entries.keys().next() {
        return Err(SchemReadError::Misplaced {
            path: format!("{item}.{key}"),
            version,
            belongs: format!("{item}.Data.{key}"),
        });
    }
    Ok(data)
}
