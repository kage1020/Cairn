//! `BlockArray` → Bedrock `.mcstructure` NBT tag tree.
//!
//! The `.mcstructure` file is uncompressed little-endian NBT with an
//! unnamed root [`Compound`] of:
//!
//! ```text
//! format_version:         Int (always 1)
//! size:                   List<Int>[3]                     (x, y, z)
//! structure:              Compound
//!   block_indices:        List<List<Int>>[2]               (block layer, waterlog layer)
//!   entities:             List<empty>
//!   palette:              Compound
//!     default:            Compound
//!       block_palette:    List<Compound>  (each: { name, states, version })
//!       block_position_data: Compound
//! structure_world_origin: List<Int>[3]
//! ```
//!
//! Layout reference: wiki.bedrock.dev's "mcstructure" page. Each
//! `block_indices` layer is a flat array in `(x, y, z)` nesting with **z
//! fastest** — `index = (x * size_y + y) * size_z + z` — which differs
//! from the `(y, z, x)` order the Java `blocks` list uses. The second
//! layer carries co-located blocks (waterlogging); Cairn's lowering never
//! authors those today, so it is `-1`-filled ("no block here").
//!
//! Blockstate properties are translated per edition by
//! [`crate::bedrock_state`]: the stair family's `facing` / `half` become
//! Bedrock's `weirdo_direction` / `upside_down_bit`, and intent Bedrock
//! cannot express (stair `shape`) is dropped with a degradation note rather
//! than silently (spec versioning-editions §10.3 / §10.4 / §10.7). A block
//! with properties outside a mapped family is still a hard error.

use cairn_lang_core::block_array::BlockArray;
pub use cairn_lang_nbt::Compound;
use cairn_lang_nbt::tag::{List, Tag};
use cairn_lang_nbt::{NbtIoError, write_bedrock_uncompressed};
use thiserror::Error;

use crate::bedrock_state::{BedrockStateError, translate_states};
use crate::data_version::BedrockTarget;
use crate::java_structure::is_concrete_id;

/// A palette entry whose intent was degraded to fit Bedrock — surfaced by the
/// caller as `W_INTENT_DEGRADED`. The serialiser has no source span (a
/// [`cairn_lang_core::block_array::BlockState`] carries none), so the note is
/// keyed by the palette entry's id and left for the CLI to attribute to the
/// enclosing structure scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityNote {
    /// Concrete id of the palette entry whose intent degraded.
    pub id: String,
    /// Human-readable degradation message.
    pub message: String,
}

/// Errors raised while serialising a [`BlockArray`] to a `.mcstructure`.
#[derive(Debug, Error)]
pub enum BedrockStructureError {
    /// Forwarded I/O / encoding failure from the NBT writer.
    #[error("nbt: {0}")]
    Nbt(#[from] NbtIoError),
    /// A palette entry's `id` lacks a `namespace:identifier` form. Same
    /// contract as the Java backend: abstract material tokens must be
    /// resolved before reaching a serialiser.
    #[error("palette entry `{id}` is abstract; expected `namespace:identifier`")]
    AbstractPaletteEntry {
        /// Offending id verbatim.
        id: String,
    },
    /// A palette entry's blockstate properties could not be translated to
    /// Bedrock's `states` vocabulary (an unmapped block family, or a value
    /// outside the Java domain). The forwarded error carries the
    /// self-correction triple so the lint loop can act on it.
    #[error(transparent)]
    State(#[from] BedrockStateError),
    /// A voxel dimension overflowed the `i32` wire width NBT uses.
    #[error("dimension {axis} = {value} exceeds NBT i32 wire limit")]
    DimensionOverflow {
        /// Which axis overflowed (`"x"` / `"y"` / `"z"`).
        axis: &'static str,
        /// Offending dimension value.
        value: u32,
    },
}

/// Build the unnamed root [`Compound`] for a `.mcstructure` file from a
/// lowered [`BlockArray`], alongside any [`ParityNote`]s raised while
/// translating blockstate properties to Bedrock's `states` vocabulary.
///
/// Pure: no I/O happens here, so the same tree can be serialised twice
/// (hashing, artifact write) without rebuilding.
///
/// # Errors
///
/// Returns [`BedrockStructureError::AbstractPaletteEntry`] for an
/// unresolved abstract token, [`BedrockStructureError::State`] for a palette
/// entry whose properties cannot be mapped to Bedrock (see
/// [`crate::bedrock_state`]), and [`BedrockStructureError::DimensionOverflow`]
/// when a dimension does not fit the wire width.
pub fn build_mcstructure_tag(
    ba: &BlockArray,
    target: &BedrockTarget,
) -> Result<(Compound, Vec<ParityNote>), BedrockStructureError> {
    // Translate every palette entry up front: a mapping failure (abstract
    // token, unmapped stateful block) aborts the whole build before any tree
    // is assembled, mirroring the Java backend's fail-loud contract.
    let mut palette_states: Vec<Compound> = Vec::with_capacity(ba.palette.entries.len());
    let mut notes: Vec<ParityNote> = Vec::new();
    for entry in &ba.palette.entries {
        if !is_concrete_id(&entry.id) {
            return Err(BedrockStructureError::AbstractPaletteEntry {
                id: entry.id.clone(),
            });
        }
        let translated = translate_states(&entry.id, &entry.properties)?;
        for message in translated.degraded {
            notes.push(ParityNote {
                id: entry.id.clone(),
                message,
            });
        }
        palette_states.push(translated.states);
    }

    let size_x = dim_to_i32(ba.dims.x, "x")?;
    let size_y = dim_to_i32(ba.dims.y, "y")?;
    let size_z = dim_to_i32(ba.dims.z, "z")?;

    let mut structure = Compound::new();
    structure.insert("block_indices", Tag::List(block_indices(ba)));
    structure.insert("entities", Tag::List(List::empty()));
    structure.insert(
        "palette",
        Tag::Compound(palette_compound(ba, target, palette_states)),
    );

    let mut root = Compound::new();
    root.insert("format_version", Tag::Int(1));
    root.insert("size", Tag::List(List::of_ints([size_x, size_y, size_z])));
    root.insert("structure", Tag::Compound(structure));
    root.insert(
        "structure_world_origin",
        Tag::List(List::of_ints([0, 0, 0])),
    );
    Ok((root, notes))
}

/// Write an already-built `.mcstructure` root under the empty root name
/// the game expects, as raw (uncompressed) little-endian NBT. Split from
/// [`build_mcstructure_tag`] so a caller can build every tree first and
/// only then start touching the filesystem, mirroring the Java backend's
/// split.
///
/// # Errors
///
/// Propagates I/O failure from `writer` and any encoding error raised by
/// the tag tree.
pub fn write_mcstructure<W: std::io::Write>(
    writer: &mut W,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_bedrock_uncompressed(writer, "", root)
}

fn dim_to_i32(value: u32, axis: &'static str) -> Result<i32, BedrockStructureError> {
    i32::try_from(value).map_err(|_| BedrockStructureError::DimensionOverflow { axis, value })
}

/// The two `block_indices` layers. Layer 0 is the palette index per
/// voxel; layer 1 is the co-located (waterlog) layer, `-1`-filled because
/// Cairn's lowering never authors co-located blocks today.
fn block_indices(ba: &BlockArray) -> List {
    let volume = ba.dims.volume();
    let mut layer0: Vec<Tag> = Vec::with_capacity(volume);
    for x in 0..ba.dims.x {
        for y in 0..ba.dims.y {
            for z in 0..ba.dims.z {
                let i = ba
                    .dims
                    .index(x, y, z)
                    .expect("voxel coordinate in dims by construction");
                layer0.push(Tag::Int(i32::from(ba.voxels[i].0)));
            }
        }
    }
    let layer1: Vec<Tag> = vec![Tag::Int(-1); volume];
    List {
        element_type_id: 9,
        items: vec![
            Tag::List(List {
                element_type_id: 3,
                items: layer0,
            }),
            Tag::List(List {
                element_type_id: 3,
                items: layer1,
            }),
        ],
    }
}

fn palette_compound(
    ba: &BlockArray,
    target: &BedrockTarget,
    palette_states: Vec<Compound>,
) -> Compound {
    let entries: Vec<Compound> = ba
        .palette
        .entries
        .iter()
        .zip(palette_states)
        .map(|(state, states)| {
            let mut c = Compound::new();
            c.insert("name", Tag::String(state.id.clone()));
            // Bedrock `states` translated from the Java properties by
            // `build_mcstructure_tag`; an empty compound for a bare block
            // (the game still expects the key).
            c.insert("states", Tag::Compound(states));
            c.insert("version", Tag::Int(target.block_version));
            c
        })
        .collect();

    let mut default = Compound::new();
    default.insert("block_palette", Tag::List(List::of_compounds(entries)));
    default.insert("block_position_data", Tag::Compound(Compound::new()));

    let mut palette = Compound::new();
    palette.insert("default", Tag::Compound(default));
    palette
}
