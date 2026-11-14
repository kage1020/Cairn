//! The library half of `cairn import`: a structure file decoded into the block grids it stores,
//! stamped with the shipped target they are written for, then transliterated into a `.crn` of
//! raw members (`spec/ecosystem-interop` "Reverse direction: the compiler transliterates, an LLM
//! lifts").

mod grid;
mod stamp;

use std::fmt;
use std::io::Read;
use std::path::Path;

use cairn_lang_core::Edition;
use cairn_lang_core::transliterate::{
    GridUnit, Stamp, TransliterateError, Transliteration, transliterate, unit_names,
};
use thiserror::Error;

use crate::bedrock_structure::{BedrockVolume, McStructureReadError, read_mcstructure};
use crate::java_structure::{StructureReadError, read_structure};
use crate::litematic::{LitematicReadError, read_litematic};
use crate::registry::{PackView, builtin_pack};
use crate::schem::{SchemReadError, read_schem};
use crate::volume::StoredBlockEntity;
use cairn_lang_core::block_array::TargetRegistry as _;

pub use stamp::{ImportStamp, StampFit, UnshippedVersion};

/// A structure file format an import reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// A Java structure file, `.nbt`.
    Nbt,
    /// A Litematica schematic, `.litematic`.
    Litematic,
    /// A Sponge Schematic, `.schem`.
    Schem,
    /// A Bedrock structure, `.mcstructure`.
    Mcstructure,
}

impl ImportFormat {
    /// Every format, in the order the CLI lists them.
    pub const ALL: [Self; 4] = [Self::Nbt, Self::Litematic, Self::Schem, Self::Mcstructure];

    /// The format `path`'s extension names, in any case.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|format| format.extension() == extension)
    }

    /// The extension, without its dot.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Nbt => "nbt",
            Self::Litematic => "litematic",
            Self::Schem => "schem",
            Self::Mcstructure => "mcstructure",
        }
    }

    /// The edition whose blocks the format stores.
    #[must_use]
    pub fn edition(self) -> Edition {
        match self {
            Self::Nbt | Self::Litematic | Self::Schem => Edition::Java,
            Self::Mcstructure => Edition::Bedrock,
        }
    }

    /// The format as a sentence names it.
    fn label(self) -> &'static str {
        match self {
            Self::Nbt => "a Java structure file",
            Self::Litematic => "a Litematica schematic",
            Self::Schem => "a Sponge Schematic",
            Self::Mcstructure => "a Bedrock structure",
        }
    }
}

/// Something of the file a raw transliteration does not carry, which `cairn import` reports as
/// `W_SEMANTIC_LOSS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticLoss {
    /// The body it concerns, `None` for the whole file.
    pub unit: Option<String>,
    /// What is lost, as a clause.
    pub what: String,
}

impl fmt::Display for SemanticLoss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.unit {
            Some(unit) => write!(f, "`{unit}`: {}", self.what),
            None => f.write_str(&self.what),
        }
    }
}

/// A structure file as the block grids it stores, before anything is written.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedStructure {
    /// The file's format.
    pub format: ImportFormat,
    /// The file's name, as the written header cites it.
    pub file_name: String,
    /// The target its grids are written for.
    pub stamp: ImportStamp,
    /// One grid per structure the file holds: a Litematica region each, else the one.
    pub units: Vec<GridUnit>,
    /// What a raw transliteration of the grids leaves behind, the loss of intent first.
    pub losses: Vec<SemanticLoss>,
}

impl DecodedStructure {
    /// The registry view of the stamped target, which checks the written blocks.
    #[must_use]
    pub fn registry(&self) -> PackView<'_> {
        builtin_pack(self.stamp.edition).view(Some(&self.stamp.mc_version))
    }

    /// The comment lines the written source opens with: where it came from and why it is
    /// stamped as it is.
    #[must_use]
    pub fn header(&self) -> Vec<String> {
        vec![
            format!(
                "imported from `{}`, {} saved at {}",
                self.file_name,
                self.format.label(),
                self.stamp.saved_label()
            ),
            format!(
                "stamped {} {}: {}",
                self.stamp.edition,
                self.stamp.mc_version,
                self.stamp.reason()
            ),
            "raw transliteration: blocks and their regularity, not the author's intent \
             (W_SEMANTIC_LOSS)"
                .to_owned(),
        ]
    }
}

/// A structure file written as raw members.
#[derive(Debug, Clone, PartialEq)]
pub struct RawImport {
    /// The file as decoded.
    pub decoded: DecodedStructure,
    /// The written source, checked against the stamped target.
    pub transliteration: Transliteration,
}

/// Why a structure file has no raw import.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ImportError {
    /// The bytes are not a Java structure file this build reads.
    #[error("{0}")]
    Structure(#[from] StructureReadError),
    /// The bytes are not a `.litematic` this build reads.
    #[error("{0}")]
    Litematic(#[from] LitematicReadError),
    /// The bytes are not a `.schem` this build reads.
    #[error("{0}")]
    Schem(#[from] SchemReadError),
    /// The bytes are not a `.mcstructure` this build reads.
    #[error("{0}")]
    McStructure(#[from] McStructureReadError),
    /// A `.litematic` with no region, so nothing to write.
    #[error("the schematic holds no region, so there is nothing to write")]
    NoRegions,
    /// A file saved past every shipped target.
    #[error(
        "the file was saved at {}, past every {} target this build ships (the newest is {}), so \
         no target can check its blocks. Fix: save it from {} or older, or import it with a \
         newer cairn",
        .0.saved, .0.edition, .0.newest, .0.newest
    )]
    Unshipped(UnshippedVersion),
    /// The grids have no transliteration.
    #[error("{0}")]
    Transliterate(#[from] TransliterateError),
}

/// Decodes `reader`, a file `file_name` of `format`, into its stamped block grids.
///
/// # Errors
///
/// [`ImportError`] for bytes its reader refuses, no region, or a version past every target.
///
/// # Panics
///
/// When the built-in Bedrock pack lacks a targetable version's state map, which it is checked for.
pub fn decode_structure<R: Read>(
    reader: R,
    format: ImportFormat,
    file_name: &str,
) -> Result<DecodedStructure, ImportError> {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file_name);
    let mut losses = vec![SemanticLoss {
        unit: None,
        what: "an import recovers blocks and their regularity, not what the author meant: every \
               member it writes is a raw block or fill, and none names a wall, a window or a \
               material slot"
            .to_owned(),
    }];
    let (stamp, units) = match format {
        ImportFormat::Nbt => decode_nbt(reader, stem, &mut losses)?,
        ImportFormat::Litematic => decode_litematic(reader, &mut losses)?,
        ImportFormat::Schem => decode_schem(reader, stem, &mut losses)?,
        ImportFormat::Mcstructure => decode_mcstructure(reader, stem, &mut losses)?,
    };
    Ok(DecodedStructure {
        format,
        file_name: file_name.to_owned(),
        stamp,
        units,
        losses,
    })
}

/// The stamp and grid of a Java structure file.
fn decode_nbt<R: Read>(
    reader: R,
    stem: &str,
    losses: &mut Vec<SemanticLoss>,
) -> Result<(ImportStamp, Vec<GridUnit>), ImportError> {
    let read = read_structure(reader)?;
    let stamp = java_stamp(read.data_version)?;
    let name = unit_names([stem]).remove(0);
    lose_block_entities(losses, &name, &read.volume.block_entities);
    lose_entities(losses, &name, &read.entities);
    if read.void_cells > 0 {
        losses.push(void_loss(&name, read.void_cells));
    }
    if read.palettes > 1 {
        losses.push(SemanticLoss {
            unit: Some(name.clone()),
            what: format!(
                "the file holds {} palettes, of which a placement picks one at random; only the \
                 first is written",
                read.palettes
            ),
        });
    }
    let unit = GridUnit {
        name,
        grid: grid::java_grid(&read.volume),
        comment: Vec::new(),
    };
    Ok((stamp, vec![unit]))
}

/// The stamp and grids of a `.litematic`, a grid per region.
fn decode_litematic<R: Read>(
    reader: R,
    losses: &mut Vec<SemanticLoss>,
) -> Result<(ImportStamp, Vec<GridUnit>), ImportError> {
    let read = read_litematic(reader)?;
    if read.regions.is_empty() {
        return Err(ImportError::NoRegions);
    }
    let stamp = java_stamp(read.data_version)?;
    let names = unit_names(read.regions.iter().map(|region| region.name.as_str()));
    let units = read
        .regions
        .iter()
        .zip(names)
        .map(|(region, name)| {
            lose_block_entities(losses, &name, &region.volume.block_entities);
            lose_entities(losses, &name, &region.entities);
            let (x, y, z) = region.min_corner;
            GridUnit {
                name,
                grid: grid::java_grid(&region.volume),
                comment: vec![format!(
                    "region `{}`, its minimum corner at {x},{y},{z} of the schematic",
                    region.name
                )],
            }
        })
        .collect();
    Ok((stamp, units))
}

/// The stamp and grid of a `.schem`.
fn decode_schem<R: Read>(
    reader: R,
    stem: &str,
    losses: &mut Vec<SemanticLoss>,
) -> Result<(ImportStamp, Vec<GridUnit>), ImportError> {
    let read = read_schem(reader)?;
    let stamp = java_stamp(read.data_version)?;
    let name = unit_names([stem]).remove(0);
    lose_block_entities(losses, &name, &read.volume.block_entities);
    lose_entities(losses, &name, &read.entities);
    let (x, y, z) = read.offset;
    let comment = if read.offset == (0, 0, 0) {
        Vec::new()
    } else {
        vec![format!("pasted at an offset of {x},{y},{z}")]
    };
    let unit = GridUnit {
        name,
        grid: grid::java_grid(&read.volume),
        comment,
    };
    Ok((stamp, vec![unit]))
}

/// The stamp and grid of a `.mcstructure`.
fn decode_mcstructure<R: Read>(
    reader: R,
    stem: &str,
    losses: &mut Vec<SemanticLoss>,
) -> Result<(ImportStamp, Vec<GridUnit>), ImportError> {
    let read = read_mcstructure(reader)?;
    let name = unit_names([stem]).remove(0);
    lose_block_entities(losses, &name, &read.volume.block_entities);
    lose_entities(losses, &name, &read.entities);
    let (stamp, grid) = bedrock_stamp(&read.volume)?;
    bedrock_losses(losses, &name, &stamp, &grid);
    let (x, y, z) = read.world_origin;
    let comment = if read.world_origin == (0, 0, 0) {
        Vec::new()
    } else {
        vec![format!("saved from {x},{y},{z} of its world")]
    };
    let unit = GridUnit {
        name,
        grid: grid.grid,
        comment,
    };
    Ok((stamp, vec![unit]))
}

/// Decodes `reader`, a file `file_name` of `format`, and writes it as raw members.
///
/// # Errors
///
/// Those of [`decode_structure`], and [`ImportError::Transliterate`] when the grids have no
/// transliteration at the stamped target.
///
/// # Panics
///
/// Those of [`decode_structure`].
pub fn import_raw<R: Read>(
    reader: R,
    format: ImportFormat,
    file_name: &str,
) -> Result<RawImport, ImportError> {
    let decoded = decode_structure(reader, format, file_name)?;
    let stamp = Stamp {
        edition: decoded.stamp.edition,
        mc_version: decoded.stamp.mc_version.clone(),
        comment: decoded.header(),
    };
    let transliteration = transliterate(&decoded.units, &stamp, &decoded.registry())?;
    Ok(RawImport {
        decoded,
        transliteration,
    })
}

/// The Java target a file saved at `data_version` is stamped with.
fn java_stamp(data_version: i32) -> Result<ImportStamp, ImportError> {
    let (versions, fit) =
        stamp::candidates(Edition::Java, data_version, |row| Some(row.data_version))
            .map_err(ImportError::Unshipped)?;
    Ok(ImportStamp {
        edition: Edition::Java,
        mc_version: versions
            .into_iter()
            .next()
            .expect("a candidate list is never empty"),
        saved: Some(data_version),
        fit,
    })
}

/// The Bedrock target `volume` is stamped with, and its grid there: of the shipped targets its
/// newest block version names, the oldest whose state map spells every block, else the oldest.
fn bedrock_stamp(volume: &BedrockVolume) -> Result<(ImportStamp, grid::BedrockGrid), ImportError> {
    let pack = builtin_pack(Edition::Bedrock);
    let saved = volume
        .palette
        .iter()
        .filter_map(|entry| entry.version)
        .max();
    let (versions, fit) = match saved {
        Some(saved) => stamp::candidates(Edition::Bedrock, saved, |row| row.block_version)
            .map_err(ImportError::Unshipped)?,
        None => (vec![pack.data_versions().latest.clone()], StampFit::Exact),
    };
    let mut first = None;
    for version in versions {
        let states = pack
            .states_at(&version)
            .expect("the loader refuses a pack without a state map for a targetable version");
        let view = pack.view(Some(&version));
        let ids = view.block_ids();
        let declares = |id: &str| ids.is_none_or(|ids| ids.contains(id));
        let grid = grid::bedrock_grid(volume, &states, &declares);
        let stamp = ImportStamp {
            edition: Edition::Bedrock,
            mc_version: version.clone(),
            saved,
            fit,
        };
        if grid.unspelled.is_empty() {
            return Ok((stamp, grid));
        }
        first.get_or_insert((stamp, grid));
    }
    Ok(first.expect("a candidate list is never empty"))
}

/// The losses a Bedrock grid records.
fn bedrock_losses(
    losses: &mut Vec<SemanticLoss>,
    name: &str,
    stamp: &ImportStamp,
    grid: &grid::BedrockGrid,
) {
    let mut lose = |what: String| {
        losses.push(SemanticLoss {
            unit: Some(name.to_owned()),
            what,
        });
    };
    if grid.void_cells > 0 {
        lose(void_loss(name, grid.void_cells).what);
    }
    for (block, cells) in &grid.unspelled {
        lose(format!(
            "`{block}` ({}) has no canonical state at bedrock {}, so it is written by its id \
             alone and builds that block's default states",
            cells_word(*cells),
            stamp.mc_version
        ));
    }
    if grid.dried > 0 {
        lose(format!(
            "water stands at {} whose block cannot hold it as `waterlogged`; the water is not \
             written",
            cells_word(grid.dried)
        ));
    }
    if grid.flooded > 0 {
        lose(format!(
            "the written block stands in water at {} where the file's second layer holds none, \
             since that block always does",
            cells_word(grid.flooded)
        ));
    }
    for (block, cells) in &grid.layered {
        lose(format!(
            "the second layer holds `{block}` at {}, which no raw member writes; it is not written",
            cells_word(*cells)
        ));
    }
}

/// `1 cell` or `n cells`.
fn cells_word(cells: usize) -> String {
    if cells == 1 {
        "1 cell".to_owned()
    } else {
        format!("{cells} cells")
    }
}

/// The loss of `cells` structure voids, written as air.
fn void_loss(name: &str, cells: usize) -> SemanticLoss {
    let verb = if cells == 1 { "holds" } else { "hold" };
    SemanticLoss {
        unit: Some(name.to_owned()),
        what: format!(
            "{} {verb} a structure void, which a placement leaves as the world has it, and the \
             import writes air there, which a placement clears",
            cells_word(cells)
        ),
    }
}

/// The loss of `block_entities`, which no raw member carries.
fn lose_block_entities(
    losses: &mut Vec<SemanticLoss>,
    name: &str,
    block_entities: &[StoredBlockEntity],
) {
    if block_entities.is_empty() {
        return;
    }
    losses.push(SemanticLoss {
        unit: Some(name.to_owned()),
        what: format!(
            "{} not written, since a raw member carries no block entity data, such as a chest's \
             contents or a command block's command: {}",
            counted(
                block_entities.len(),
                "block entity is",
                "block entities are"
            ),
            listed(&grid::block_entity_ids(block_entities))
        ),
    });
}

/// The loss of `entities`, which an import does not write.
fn lose_entities(
    losses: &mut Vec<SemanticLoss>,
    name: &str,
    entities: &[cairn_lang_nbt::tag::Compound],
) {
    if entities.is_empty() {
        return;
    }
    losses.push(SemanticLoss {
        unit: Some(name.to_owned()),
        what: format!(
            "{} not written, since an import writes blocks only: {}",
            counted(entities.len(), "entity is", "entities are"),
            listed(&grid::entity_ids(entities))
        ),
    });
}

/// `1 <one>` or `n <many>`.
fn counted(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Each id after its count, comma-joined.
fn listed(ids: &[(String, usize)]) -> String {
    ids.iter()
        .map(|(id, n)| format!("{n} `{id}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
