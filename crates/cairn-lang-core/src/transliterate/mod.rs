//! L1 transliteration, the compiler's half of the reverse direction (`spec/ecosystem-interop`
//! "Three tiers of transliteration"): the blocks a structure file stores, written as raw members
//! that compile back to the same blocks, with no name given to anything.
//!
//! Each grid is cut into boxes of one block, boxes repeated at a fixed step fold into a
//! `raw_repeat`, and every connection state the compile derives back is left out. The written
//! source is compiled against the stamped target and compared cell by cell; a cell the compile
//! builds otherwise gets its connection states written, until every cell agrees.

mod boxes;
mod emit;
mod names;
mod oracle;

use std::fmt;

use indexmap::IndexMap;

use crate::block_array::{BlockState, Dims, PaletteIndex, TargetRegistry};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::edition::Edition;

pub(crate) use emit::spelled;
pub use names::unit_names;

/// The widest and deepest footprint one written body takes; a grid past it on `x` or `z` is
/// written as a `site` of chunks (`spec/ecosystem-interop` "Import stamping and pitfalls").
pub const CHUNK_FOOTPRINT: u32 = 48;

/// The blocks of one structure, as a file stores them.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockGrid {
    dims: Dims,
    palette: Vec<GridEntry>,
    voxels: Vec<PaletteIndex>,
}

impl BlockGrid {
    /// The grid of `dims` whose `voxels`, laid out as [`Dims::index`] lays them, index `palette`.
    ///
    /// # Errors
    ///
    /// [`BlockGridError`] when the voxels do not fill the extents or one names no palette entry.
    pub fn new(
        dims: Dims,
        palette: Vec<GridEntry>,
        voxels: Vec<PaletteIndex>,
    ) -> Result<Self, BlockGridError> {
        if voxels.len() != dims.volume() {
            return Err(BlockGridError::VoxelCount {
                dims,
                voxels: voxels.len(),
            });
        }
        if let Some(slot) = voxels
            .iter()
            .find(|slot| usize::from(slot.0) >= palette.len())
        {
            return Err(BlockGridError::PastPalette {
                index: slot.0,
                len: palette.len(),
            });
        }
        Ok(Self {
            dims,
            palette,
            voxels,
        })
    }

    /// The grid's extents.
    #[must_use]
    pub fn dims(&self) -> Dims {
        self.dims
    }

    /// The blocks the voxels index.
    #[must_use]
    pub fn palette(&self) -> &[GridEntry] {
        &self.palette
    }

    /// One index into [`Self::palette`] per voxel.
    #[must_use]
    pub fn voxels(&self) -> &[PaletteIndex] {
        &self.voxels
    }
}

/// Why parts make no [`BlockGrid`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockGridError {
    /// Voxels that do not fill the extents.
    VoxelCount {
        /// The extents.
        dims: Dims,
        /// How many voxels were given.
        voxels: usize,
    },
    /// A voxel naming an entry past the palette.
    PastPalette {
        /// The index it names.
        index: u16,
        /// The palette's length.
        len: usize,
    },
}

impl fmt::Display for BlockGridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VoxelCount { dims, voxels } => write!(
                f,
                "{voxels} voxels are given for {}x{}x{} cells",
                dims.x, dims.y, dims.z
            ),
            Self::PastPalette { index, len } => {
                write!(f, "a voxel names palette entry {index} of {len}")
            }
        }
    }
}

impl std::error::Error for BlockGridError {}

/// One block of a [`BlockGrid`], in the vocabulary a raw member's `mat=` writes.
#[derive(Debug, Clone, PartialEq)]
pub struct GridEntry {
    /// The block state, with every property the file says.
    pub state: BlockState,
    /// The value the target builds for a property a line leaves out; a property with none is
    /// built only as the line writes it.
    pub defaults: IndexMap<String, String>,
    /// The properties the target builds the same whatever their value.
    pub unread: Vec<String>,
}

impl GridEntry {
    /// An entry whose target builds exactly the properties a line writes, as Java's does.
    #[must_use]
    pub fn exact(state: BlockState) -> Self {
        Self {
            state,
            defaults: IndexMap::new(),
            unread: Vec::new(),
        }
    }

    /// Whether `built` builds the same block as this entry at the target.
    fn is_built_as(&self, built: &BlockState) -> bool {
        built.id == self.state.id
            && self
                .compared_keys(built)
                .all(|key| self.read(&self.state, key) == self.read(built, key))
    }

    /// Each property `built` holds at another value than this entry, as `(key, this entry's
    /// value, built's value)`, a property one side leaves out read as the target's default.
    pub(crate) fn differing_states(
        &self,
        built: &BlockState,
    ) -> Vec<(String, Option<String>, Option<String>)> {
        let mut out: Vec<(String, Option<String>, Option<String>)> = Vec::new();
        for key in self.compared_keys(built) {
            let (expected, got) = (self.read(&self.state, key), self.read(built, key));
            if expected != got && !out.iter().any(|(seen, _, _)| seen == key) {
                out.push((key.clone(), expected.cloned(), got.cloned()));
            }
        }
        out
    }

    /// Every property either side writes or the target gives a default for, but those the target
    /// does not read; a key both sides name comes up more than once.
    fn compared_keys<'a>(&'a self, built: &'a BlockState) -> impl Iterator<Item = &'a String> {
        built
            .properties
            .keys()
            .chain(self.state.properties.keys())
            .chain(self.defaults.keys())
            .filter(|key| !self.unread.contains(key))
    }

    /// The value `state` builds for `key`: its own, else the target's default.
    fn read<'a>(&'a self, state: &'a BlockState, key: &str) -> Option<&'a String> {
        state.properties.get(key).or_else(|| self.defaults.get(key))
    }
}

/// One grid to write, under the name its body takes and with the comment lines written above it.
#[derive(Debug, Clone, PartialEq)]
pub struct GridUnit {
    /// The body's name, an identifier [`unit_names`] made.
    pub name: String,
    /// The blocks.
    pub grid: BlockGrid,
    /// Comment lines written above the body, each without its `#`.
    pub comment: Vec<String>,
}

/// The target a transliteration is written for, which its header stamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    /// The edition the file's blocks are spelled in.
    pub edition: Edition,
    /// The version its blocks are checked against, which `@requires` and `@intended_targets`
    /// name.
    pub mc_version: String,
    /// Comment lines written at the top of the file, each without its `#`.
    pub comment: Vec<String>,
}

/// A written `.crn` source and what it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Transliteration {
    /// The source text.
    pub source: String,
    /// Each unit's body, in the order given: a `struct`, or a `site` of chunks.
    pub bodies: Vec<WrittenBody>,
    /// The warnings compiling [`Self::source`] at the stamped target raises.
    pub warnings: Vec<Diagnostic>,
}

/// How one [`GridUnit`] was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenBody {
    /// The unit's name.
    pub name: String,
    /// The `def` of each chunk the `site` places, empty for a unit written as one `struct`.
    pub chunks: Vec<String>,
    /// The member lines written, `raw_repeat` lines and the lines under them included.
    pub lines: usize,
}

/// Why a set of grids has no transliteration.
#[derive(Debug, Clone, PartialEq)]
pub enum TransliterateError {
    /// A unit holds no block but air, so nothing would size its body.
    Empty {
        /// The unit's name.
        unit: String,
    },
    /// A unit holds a block whose id or state no `mat=` value can spell.
    Unspellable {
        /// The unit's name.
        unit: String,
        /// The block, as `id[key=value,...]`.
        block: String,
        /// The part no `mat=` value can spell.
        part: UnspellablePart,
    },
    /// The written source does not build the bodies it writes, a fault of the writer.
    Miswritten {
        /// What it builds otherwise.
        reason: String,
    },
    /// Compiling the written source at the stamped target raises an error, such as a block the
    /// target does not declare.
    Refused {
        /// The stamped target, as `java 1.21.4`.
        target: String,
        /// The written source.
        source: String,
        /// The errors, with spans into [`Self::Refused::source`].
        errors: Vec<Diagnostic>,
    },
    /// A cell the compile builds as another block though every state of it is written.
    Unmatched {
        /// The unit's name.
        unit: String,
        /// The cell.
        cell: (u32, u32, u32),
        /// The block the grid holds there.
        expected: String,
        /// The block the compile built there.
        built: String,
    },
}

impl fmt::Display for TransliterateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { unit } => write!(
                f,
                "`{unit}` holds no block but air, so there is nothing to write"
            ),
            Self::Unspellable { unit, block, part } => write!(
                f,
                "`{unit}` holds `{block}`, whose {part} no `mat=` can spell: a block is \
                 `namespace:path[property=value]`, each name ASCII letters, digits and `_` that \
                 does not start with a digit, and a value such a name or digits alone. Fix: \
                 replace the block in the structure with one the target declares, and save the \
                 file again"
            ),
            Self::Miswritten { reason } => write!(
                f,
                "the written source {reason}; the transliteration is refused rather than \
                 written wrong, and the file is worth reporting"
            ),
            Self::Refused {
                target,
                source,
                errors,
            } => {
                write!(
                    f,
                    "the written source does not compile at the stamped target, {target}:"
                )?;
                for error in errors {
                    let before = source.get(..error.span.start).unwrap_or("");
                    let start = before.rfind('\n').map_or(0, |i| i + 1);
                    let line = source[start..].lines().next().unwrap_or("").trim();
                    write!(
                        f,
                        "\n  error[{}]: {} (at `{line}`)",
                        error.code.as_str(),
                        error.primary
                    )?;
                }
                refusal_fixes(f, target, errors)
            }
            Self::Unmatched {
                unit,
                cell: (x, y, z),
                expected,
                built,
            } => write!(
                f,
                "`{unit}` cell {x},{y},{z} holds `{expected}`, and the written raw members build \
                 `{built}` there, so no raw member writes it; the transliteration is refused \
                 rather than written wrong, and the file is worth reporting"
            ),
        }
    }
}

impl std::error::Error for TransliterateError {}

/// The part of a block that no `mat=` value can spell.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnspellablePart {
    /// The id.
    Id,
    /// The name of a property.
    Property(String),
    /// The value of the property it names.
    Value(String),
}

impl fmt::Display for UnspellablePart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id => f.write_str("id"),
            Self::Property(key) => write!(f, "property `{key}`"),
            Self::Value(key) => write!(f, "value of `{key}`"),
        }
    }
}

/// One `Fix:` line per kind of error among `errors` the stamped `target` raises.
fn refusal_fixes(f: &mut fmt::Formatter<'_>, target: &str, errors: &[Diagnostic]) -> fmt::Result {
    let raised = |codes: &[DiagnosticCode]| errors.iter().any(|error| codes.contains(&error.code));
    let blocks = [DiagnosticCode::UnknownId];
    let states = [DiagnosticCode::StateDomain, DiagnosticCode::StateUnmapped];
    if raised(&blocks) {
        write!(
            f,
            "\nFix: open and save the file in {target}, which renames a block an older release \
             spelled otherwise, or replace each block no {target} declares"
        )?;
    }
    if raised(&states) {
        write!(
            f,
            "\nFix: open and save the file in {target}, which rewrites a state an older release \
             stored otherwise, or replace each state {target} does not build"
        )?;
    }
    if errors
        .iter()
        .any(|error| !blocks.contains(&error.code) && !states.contains(&error.code))
    {
        write!(
            f,
            "\nFix: none; a source the transliteration writes compiles at any target that \
             builds its blocks, so the file is worth reporting"
        )?;
    }
    Ok(())
}

/// Rounds of restoring connection states before every one is written.
const ROUNDS: usize = 8;

/// Write `units` as one `.crn` source for `stamp`'s target, checked by compiling it against
/// `registry` and comparing every cell.
///
/// # Errors
///
/// [`TransliterateError`] when a unit holds nothing or a block no `mat=` spells, the target
/// refuses its blocks, or the written source does not build every cell as the grid holds it.
pub fn transliterate(
    units: &[GridUnit],
    stamp: &Stamp,
    registry: &dyn TargetRegistry,
) -> Result<Transliteration, TransliterateError> {
    let mut plans = units
        .iter()
        .map(|unit| emit::UnitPlan::new(unit, registry))
        .collect::<Result<Vec<_>, _>>()?;
    for round in 0..=ROUNDS {
        let (source, bodies) = emit::source(&plans, stamp);
        let built = oracle::compile(&source, stamp, registry)?;
        let mut agreed = true;
        for plan in &mut plans {
            let unmatched = plan.unmatched(&built.ir)?;
            if unmatched.is_empty() {
                continue;
            }
            agreed = false;
            if round == ROUNDS {
                let (chunk, cell, built) = &unmatched[0];
                return Err(plan.mismatch(*chunk, *cell, built));
            }
            plan.write_connections(unmatched.iter().map(|(chunk, cell, _)| (*chunk, *cell)));
            if round + 1 == ROUNDS {
                plan.write_every_connection();
            }
        }
        if agreed {
            return Ok(Transliteration {
                source,
                bodies,
                warnings: built.warnings,
            });
        }
    }
    unreachable!("the last round either agrees or returns its mismatch")
}

#[cfg(test)]
mod tests;
