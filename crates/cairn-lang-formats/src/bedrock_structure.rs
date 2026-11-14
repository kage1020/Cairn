//! A [`BlockArray`] as a Bedrock `.mcstructure`. Each `block_indices` layer runs z fastest,
//! `(x * size_y + y) * size_z + z`, unlike Java's `blocks`; the second layer holds water where a
//! waterlogged entry stands and `-1` everywhere else. A block entity is written under
//! `block_position_data`, keyed by its voxel's place in that order: each of the IR's, and one for
//! each block whose Java states Bedrock keeps in its block entity, as a chest's pairing.

use std::collections::HashMap;

use cairn_lang_core::block_array::{
    BlockArray, BlockEntityData, BlockState, DYE_COLORS, PaletteIndex, SignText,
};
use cairn_lang_core::resolve::DroppedIntent;
pub use cairn_lang_nbt::Compound;
use cairn_lang_nbt::tag::{List, Tag};
use cairn_lang_nbt::{
    CompoundStream, NbtIoError, check_string, stream_bedrock_uncompressed,
    write_bedrock_uncompressed,
};
use thiserror::Error;

use crate::bedrock_state::{BedrockStateError, StateEntity, translate_states};
use crate::block_id::is_concrete_id;
use crate::data_version::BedrockTarget;
use crate::dims::{VolumeError, dims_to_i32, list_volume};
use crate::registry::VersionStates;

/// One intent a palette entry dropped to fit Bedrock, reported as `W_INTENT_DEGRADED`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParityNote {
    /// The entry's block id.
    pub id: String,
    /// The entry's `key=value` pairs, comma-joined, the spelling of
    /// [`DegradedEntry::states`](cairn_lang_core::resolve::DegradedEntry::states).
    pub states: String,
    /// The intent dropped, worded by its `reason`.
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
        /// `"block_indices"` or `"block_palette"`.
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
    /// A block entity Bedrock has no counterpart for in this writer.
    #[error(
        "block entity `{id}` has no Bedrock form this writer knows; it writes `{known}` only. Fix: \
         compile with `--edition java`"
    )]
    UnmappedBlockEntity {
        /// The block entity's id.
        id: String,
        /// The ids it maps, comma-joined.
        known: String,
    },
    /// A bed whose colour is no dye's.
    #[error("the bed at {pos:?} is `{color}`, which is not a dye's colour. Valid: {valid}")]
    UnknownBedColor {
        /// The bed's voxel.
        pos: (u32, u32, u32),
        /// The colour.
        color: String,
        /// The dyes' colours, comma-joined.
        valid: String,
    },
    /// A sign's text the NBT encoder cannot write.
    #[error(
        "the sign at {pos:?} has the text `{text}`, which cannot be written as NBT: {source}. \
         Fix: write the sign's text in printable ASCII"
    )]
    UnencodableSignText {
        /// The sign's voxel.
        pos: (u32, u32, u32),
        /// The face's text, its lines joined by newlines.
        text: String,
        /// The encoder's refusal.
        source: NbtIoError,
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
    slots: Vec<i32>,
    /// Each palette entry's water layer slot, `-1` for one that is not waterlogged.
    water: Vec<i32>,
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

/// Checks that `block_array` writes as a `.mcstructure` for `target`, its palette translated
/// through `states`, with the [`ParityNote`]s the translation raised, in palette order.
///
/// # Errors
///
/// The first fault found, as any [`BedrockStructureError`] but [`BedrockStructureError::Nbt`].
///
/// # Panics
///
/// When `states` is another version's map than `target`'s.
pub fn prepare_mcstructure<'a>(
    block_array: &'a BlockArray,
    target: &BedrockTarget,
    states: &VersionStates<'_>,
) -> Result<(McStructure<'a>, Vec<ParityNote>), BedrockStructureError> {
    assert_eq!(
        states.mc_version(),
        target.mc_version,
        "the state map must be the target version's"
    );
    let entries = &block_array.palette.entries;
    let mut palette = BlockPalette::with_capacity(entries.len());
    let mut slots: Vec<i32> = Vec::with_capacity(entries.len());
    let mut waterlogged: Vec<bool> = Vec::with_capacity(entries.len());
    let mut held: Vec<Option<(String, StateEntity)>> = Vec::with_capacity(entries.len());
    let mut notes: Vec<ParityNote> = Vec::new();
    for (slot, entry) in entries.iter().enumerate() {
        if !is_concrete_id(&entry.id) {
            return Err(BedrockStructureError::AbstractPaletteEntry {
                id: entry.id.clone(),
            });
        }
        let derived = block_array.derived.get(slot).map_or(&[][..], Vec::as_slice);
        let translated = translate_states(states, entry, derived)?;
        check_palette_strings(&translated.id, &translated.states)?;
        // Named by the canonical entry: several can share one Bedrock state once the intent drops.
        if !translated.degraded.is_empty() {
            let states = entry.properties_text();
            notes.extend(translated.degraded.into_iter().map(|dropped| ParityNote {
                id: entry.id.clone(),
                states: states.clone(),
                dropped,
            }));
        }
        held.push(
            translated
                .entity
                .map(|entity| (translated.id.clone(), entity)),
        );
        let compound = palette_entry(&translated.id, translated.states, target.block_version);
        slots.push(palette.slot(compound)?);
        waterlogged.push(translated.waterlogged);
    }
    let water = if waterlogged.contains(&true) {
        let water = translate_states(states, &BlockState::bare(WATER_ID), &[])?;
        let slot = palette.slot(palette_entry(&water.id, water.states, target.block_version))?;
        waterlogged
            .iter()
            .map(|&wet| if wet { slot } else { -1 })
            .collect()
    } else {
        vec![-1; waterlogged.len()]
    };
    let BlockPalette {
        entries: block_palette,
        ..
    } = palette;
    let block_palette_len = block_palette.len();
    let size = dims_to_i32(&block_array.dims)?;
    let volume = list_volume(block_array)?;
    if let Some((index, len)) = block_array.first_index_outside_palette() {
        return Err(BedrockStructureError::PaletteIndexOutOfRange { index, len });
    }
    let position_data = position_data(block_array, &held)?;

    let prepared = McStructure {
        block_array,
        size,
        volume,
        palette: palette_compound(block_palette, position_data),
        block_palette_len,
        slots,
        water,
    };
    Ok((prepared, notes))
}

/// The block a waterlogged cell's second layer holds.
const WATER_ID: &str = "minecraft:water";

/// Each block entity id the writer maps, with the id Bedrock saves it under.
const BLOCK_ENTITY_IDS: [(&str, &str); 1] = [("minecraft:comparator", "Comparator")];

/// The opaque black ARGB a sign's text is written in.
const SIGN_TEXT_BLACK: i32 = 0xFF00_0000_u32.cast_signed();

/// `block_position_data`: each block entity's `block_entity_data`, keyed by its voxel's index in
/// `block_indices`, its position the voxel's own; `held` is what each palette entry's block entity
/// holds of its states, with its Bedrock id.
fn position_data(
    block_array: &BlockArray,
    held: &[Option<(String, StateEntity)>],
) -> Result<Compound, BedrockStructureError> {
    let dims = block_array.dims;
    let index = |(x, y, z): (u32, u32, u32)| {
        (u64::from(x) * u64::from(dims.y) + u64::from(y)) * u64::from(dims.z) + u64::from(z)
    };
    let mut data = Compound::new();
    for entity in &block_array.block_entities {
        let nbt = member_entity(&entity.data, entity.pos)?;
        data.insert(index(entity.pos).to_string(), placed(nbt, entity.pos)?);
    }
    if held.iter().all(Option::is_none) {
        return Ok(data);
    }
    let held_at = |x: i64, y: u32, z: i64| {
        let (x, z) = (u32::try_from(x).ok()?, u32::try_from(z).ok()?);
        let voxel = block_array.voxels[dims.index(x, y, z)?];
        held.get(usize::from(voxel.0))?.as_ref()
    };
    for x in 0..dims.x {
        for y in 0..dims.y {
            for z in 0..dims.z {
                let Some((id, entity)) = held_at(i64::from(x), y, i64::from(z)) else {
                    continue;
                };
                let nbt = match *entity {
                    StateEntity::ChestHalf { toward, lead } => {
                        let (px, pz) = (
                            i64::from(x) + i64::from(toward.0),
                            i64::from(z) + i64::from(toward.1),
                        );
                        // A half whose other half is not beside it stands single.
                        let paired = held_at(px, y, pz).is_some_and(|(other, half)| {
                            other == id
                                && *half
                                    == StateEntity::ChestHalf {
                                        toward: (-toward.0, -toward.1),
                                        lead: !lead,
                                    }
                        });
                        if !paired {
                            continue;
                        }
                        let mut nbt = Compound::new();
                        nbt.insert("id", Tag::String("Chest".to_owned()));
                        nbt.insert("pairlead", Tag::Byte(i8::from(lead)));
                        for (axis, value) in [("pairx", px), ("pairz", pz)] {
                            let value = i32::try_from(value)
                                .expect("a voxel of the grid, whose dimensions fit `i32`");
                            nbt.insert(axis, Tag::Int(value));
                        }
                        nbt
                    }
                    StateEntity::Note(note) => {
                        let mut nbt = Compound::new();
                        nbt.insert("id", Tag::String("Music".to_owned()));
                        nbt.insert("note", Tag::Byte(note));
                        nbt
                    }
                };
                data.insert(index((x, y, z)).to_string(), placed(nbt, (x, y, z))?);
            }
        }
    }
    Ok(data)
}

/// One of the IR's block entities as Bedrock saves it, without its position.
fn member_entity(
    data: &BlockEntityData,
    pos: (u32, u32, u32),
) -> Result<Compound, BedrockStructureError> {
    let mut nbt = Compound::new();
    match data {
        BlockEntityData::Fields { id: java, fields } => {
            let Some(&(_, id)) = BLOCK_ENTITY_IDS.iter().find(|(known, _)| known == java) else {
                return Err(BedrockStructureError::UnmappedBlockEntity {
                    id: java.clone(),
                    known: BLOCK_ENTITY_IDS.map(|(known, _)| known).join(", "),
                });
            };
            nbt.insert("id", Tag::String(id.to_owned()));
            for (field, value) in fields {
                nbt.insert(field.clone(), Tag::Int(*value));
            }
        }
        BlockEntityData::Sign { front, back } => {
            nbt.insert("id", Tag::String("Sign".to_owned()));
            nbt.insert("FrontText", Tag::Compound(sign_face(front, pos)?));
            nbt.insert("BackText", Tag::Compound(sign_face(back, pos)?));
            nbt.insert("IsWaxed", Tag::Byte(0));
        }
        BlockEntityData::Bed { color } => {
            // A bed of no colour is saved as the game saves one, white.
            let color = color.as_deref().unwrap_or(DYE_COLORS[0]);
            let Some(number) = DYE_COLORS.iter().position(|dye| *dye == color) else {
                return Err(BedrockStructureError::UnknownBedColor {
                    pos,
                    color: color.to_owned(),
                    valid: DYE_COLORS.join(", "),
                });
            };
            nbt.insert("id", Tag::String("Bed".to_owned()));
            nbt.insert(
                "color",
                Tag::Byte(i8::try_from(number).expect("sixteen dyes fit a byte")),
            );
        }
    }
    Ok(nbt)
}

/// One face of a sign as Bedrock saves it: its lines as one text, black and glowing or not.
fn sign_face(text: &SignText, pos: (u32, u32, u32)) -> Result<Compound, BedrockStructureError> {
    let joined = text.lines.join("\n");
    check_string(&joined).map_err(|source| BedrockStructureError::UnencodableSignText {
        pos,
        text: joined.clone(),
        source,
    })?;
    let mut face = Compound::new();
    face.insert("Text", Tag::String(joined));
    face.insert("TextOwner", Tag::String(String::new()));
    face.insert("SignTextColor", Tag::Int(SIGN_TEXT_BLACK));
    face.insert("IgnoreLighting", Tag::Byte(i8::from(text.glowing)));
    face.insert("HideGlowOutline", Tag::Byte(0));
    face.insert("PersistFormatting", Tag::Byte(1));
    Ok(face)
}

/// `nbt` with the position `pos`, as `block_position_data` holds it.
fn placed(mut nbt: Compound, (x, y, z): (u32, u32, u32)) -> Result<Tag, BedrockStructureError> {
    for (axis, value) in [("x", x), ("y", y), ("z", z)] {
        let value = i32::try_from(value)
            .map_err(|_| BedrockStructureError::DimensionOverflow { axis, value })?;
        nbt.insert(axis, Tag::Int(value));
    }
    let mut block = Compound::new();
    block.insert("block_entity_data", Tag::Compound(nbt));
    Ok(Tag::Compound(block))
}

/// The `block_palette` being built, a set (`spec/compilation` "Within-phase conflicts and the
/// palette"): an entry whose translation folds into an earlier one takes that slot, so air stays
/// at 0.
struct BlockPalette {
    entries: Vec<Compound>,
    slots: HashMap<String, Vec<i32>>,
}

impl BlockPalette {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            slots: HashMap::new(),
        }
    }

    /// The slot of `compound`, added unless an earlier entry is the same block.
    fn slot(&mut self, compound: Compound) -> Result<i32, BedrockStructureError> {
        // Keyed by the `name` each compound carries: entries of two ids never merge.
        let name = match compound.entries.get("name") {
            Some(Tag::String(name)) => name.clone(),
            _ => String::new(),
        };
        let named = self.slots.entry(name).or_default();
        let entries = &self.entries;
        if let Some(slot) = named
            .iter()
            .copied()
            .find(|&slot| usize::try_from(slot).is_ok_and(|slot| entries[slot] == compound))
        {
            return Ok(slot);
        }
        self.entries.push(compound);
        let len = self.entries.len();
        let slot = i32::try_from(len).map_err(|_| BedrockStructureError::ListTooLong {
            list: "block_palette",
            len,
        })? - 1;
        named.push(slot);
        Ok(slot)
    }
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
        self.slots[usize::from(voxel.0)]
    }

    /// The water layer's slot for a voxel, `-1` for a dry one.
    fn water_slot(&self, voxel: PaletteIndex) -> i32 {
        self.water[usize::from(voxel.0)]
    }

    /// Each voxel's index, in the `block_indices` order: z fastest, then y, then x.
    fn voxel_order(&self) -> impl Iterator<Item = PaletteIndex> + '_ {
        let dims = self.block_array.dims;
        (0..dims.x).flat_map(move |x| {
            (0..dims.y).flat_map(move |y| {
                (0..dims.z).map(move |z| {
                    let i = dims
                        .index(x, y, z)
                        .expect("voxel coordinate in dims by construction");
                    self.block_array.voxels[i]
                })
            })
        })
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
        let volume = self.volume;
        structure.list("block_indices", 9, 2, |layers| {
            layers.list(3, volume, |layer0| {
                for voxel in self.voxel_order() {
                    layer0.item(&Tag::Int(self.slot(voxel)))?;
                }
                Ok(())
            })?;
            layers.list(3, volume, |layer1| {
                for voxel in self.voxel_order() {
                    layer1.item(&Tag::Int(self.water_slot(voxel)))?;
                }
                Ok(())
            })
        })
    }

    /// The two `block_indices` layers: each voxel's `block_palette` slot, then its water's.
    fn block_indices(&self) -> List {
        let layer0: Vec<Tag> = self.voxel_order().map(|v| Tag::Int(self.slot(v))).collect();
        let layer1: Vec<Tag> = self
            .voxel_order()
            .map(|v| Tag::Int(self.water_slot(v)))
            .collect();
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
///
/// # Panics
///
/// When `states` is another version's map than `target`'s.
pub fn build_mcstructure_tag(
    block_array: &BlockArray,
    target: &BedrockTarget,
    states: &VersionStates<'_>,
) -> Result<(Compound, Vec<ParityNote>), BedrockStructureError> {
    let (prepared, notes) = prepare_mcstructure(block_array, target, states)?;

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

fn palette_compound(block_palette: Vec<Compound>, position_data: Compound) -> Compound {
    let mut default = Compound::new();
    default.insert(
        "block_palette",
        Tag::List(List::of_compounds(block_palette)),
    );
    default.insert("block_position_data", Tag::Compound(position_data));

    let mut palette = Compound::new();
    palette.insert("default", Tag::Compound(default));
    palette
}
