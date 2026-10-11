//! Per-edition palette-entry portability counters.
//!
//! Backs the "edition portability" axis of `cairn info` — the "Which version
//! is it for?" question that `spec/versioning-editions` gives three answers
//! to. The counters run over a real lowered [`BlockArrayIr`] rather than over
//! the source, so each figure counts palette entries the matching
//! `cairn compile --edition X` emits, one for one.
//!
//! The two sides can still spell an entry differently: `info` pins no
//! target and takes each material's default mapping, a build takes the
//! target's, and a rename inside an edition's range makes those different
//! strings for the same block. The number of entries must not differ.
//!
//! Two independent questions decide an entry's category, asked in that
//! order.
//!
//! # Does the edition have the block at all?
//!
//! A palette entry can name a block the edition has never had: an authored
//! `@oak_sign` lowers verbatim on either edition, and Bedrock spells that
//! block `standing_sign`. An id no supported version of the edition
//! declares counts as **unsupported**, and the state question below is not
//! asked — there are no states to translate on a block that does not exist.
//!
//! The tables are per version while this axis is per edition, so the
//! question is "some version", not "every version": Bedrock has
//! `stonebrick` up to 1.21.0 and `stone_bricks` from 1.21.40, and neither
//! is absent from Bedrock. Whether the version being built has the block is
//! the narrower question `cairn compile --target` asks as `E_UNKNOWN_ID`.
//!
//! A pack carrying no `blocks` component cannot refute an id, so this axis
//! stays silent for such a pack rather than reporting every entry as
//! unsupported — the same line the lowering pass draws.
//!
//! # Do the states survive?
//!
//! Java is the base edition per `spec/versioning-editions` "Backend = data
//! tables", so a palette entry Java declares always counts as portable
//! there: the Java writer emits `properties` verbatim. Bedrock funnels every
//! entry through [`crate::bedrock_state::translate_states`], the function
//! the `.mcstructure` writer uses, so the classification cannot drift from
//! what a build does, and folds the outcome into
//! `{portable, degraded, unsupported}`:
//!
//! - `Ok(StateTranslation { degraded: [], .. })` → **portable** (the intent
//!   round-trips into Bedrock states with no loss).
//! - `Ok(StateTranslation { degraded: non-empty, .. })` → **degraded** (the
//!   intent compiles but loses detail — e.g. a corner stair `shape` that
//!   Bedrock has no state for; the `.mcstructure` writer surfaces this as
//!   `W_INTENT_DEGRADED`).
//! - `Err(BedrockStateError::UnmappableBlock)` → **unsupported** (the
//!   edition has the block, and this backend has no mapping for the states
//!   the intent put on it, so the `.mcstructure` writer would refuse the
//!   entry).
//!
//! The translator's other two refusals, `UnknownStairState` (a value
//! outside the Java domain) and `UnknownStairKey` (a key the backend does
//! not read), name a blockstate the registry pack should have refused, not
//! anything the edition lacks. Counting one as `unsupported` would publish
//! a validation gap as an ordinary portability figure, so
//! [`portability_for_bedrock`] refuses with [`InvalidPalette`] instead.
//!
//! Counting is per palette entry, the unit `translate_states` takes, so a
//! member that interns several entries (a gable roof with per-corner
//! stairs) contributes one per entry, as the `.mcstructure` writer emits
//! them.
//!
//! The [`BlockState::AIR_ID`] slot at palette index 0 is skipped — every
//! palette carries it by construction (`Palette::new_with_air`), and it
//! is not a member-authored intent that could be "unsupported".

use cairn_lang_core::block_array::{BlockArrayIr, BlockState};
use cairn_lang_core::resolve::{DegradedEntry, UnsupportedEntry, UnsupportedReason};
use cairn_lang_core::suggest::nearest_namespaced_id;

use thiserror::Error;

use crate::bedrock_state::{BedrockStateError, StateTranslation, translate_states};
use crate::registry::{AliasIndex, BlocksIndex};

/// One edition's portability answer: the counts, and the entries behind
/// the `unsupported` one.
///
/// One report rather than counts plus a second entry point for the
/// entries, because the classification decides category and reason in one
/// step.
///
/// The fields are private, and a count is raised only beside a push to its
/// list, so the figure and the list describe the same entries for every
/// value of this type, including one built outside the crate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortabilityReport {
    counts: PortabilityCounts,
    degraded: Vec<DegradedEntry>,
    unsupported: Vec<UnsupportedEntry>,
}

/// Both of a [`PortabilityReport`]'s lists, moved out together.
///
/// Named fields rather than a pair, because a tuple would make order the
/// only thing telling two lists of palette entries apart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortabilityEntries {
    /// The entries [`PortabilityCounts::degraded`] counts, in palette
    /// order.
    pub degraded: Vec<DegradedEntry>,
    /// The entries [`PortabilityCounts::unsupported`] counts, in palette
    /// order.
    pub unsupported: Vec<UnsupportedEntry>,
}

/// Palette-entry counts per portability category.
///
/// Fields are `u32` to match the `EditionPortability` shape in
/// `cairn-lang-core::resolve`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PortabilityCounts {
    /// Palette entries whose intent translates losslessly into this
    /// edition's on-disk shape.
    pub portable: u32,
    /// Palette entries that translate but lose detail (the `.mcstructure`
    /// writer would emit `W_INTENT_DEGRADED` for these).
    pub degraded: u32,
    /// Palette entries with no representation on this edition: a block no
    /// supported version of the edition declares, or a stateful family
    /// this backend has no Bedrock mapping for.
    pub unsupported: u32,
}

/// A palette carrying blockstates the registry pack was expected to
/// refuse, and every entry that proves it.
///
/// Returned instead of counts because "how much of this ports to Bedrock?"
/// presumes a palette a build could have produced, and a count over this
/// one would read as ordinary portability.
///
/// The leaks are the translator's own errors, unreworded, so a caller
/// prints the sentence a build would have printed. All of them are carried,
/// not the first: they are symptoms of one pack bug.
///
/// The [`Display`](std::fmt::Display) impl only satisfies
/// [`std::error::Error`]; a caller renders the list with its own framing.
#[derive(Debug, Error)]
#[error(
    "the palette carries blockstates the registry pack was expected to refuse ({} of them)",
    .leaks.len()
)]
pub struct InvalidPalette {
    leaks: Vec<BedrockStateError>,
}

impl InvalidPalette {
    /// The refusals, in palette order.
    #[must_use]
    pub fn leaks(&self) -> &[BedrockStateError] {
        &self.leaks
    }
}

impl PortabilityReport {
    /// How many entries fall in each category.
    #[must_use]
    pub fn counts(&self) -> PortabilityCounts {
        self.counts
    }

    /// The entries [`PortabilityCounts::unsupported`] counts, in palette
    /// order.
    #[must_use]
    pub fn unsupported(&self) -> &[UnsupportedEntry] {
        &self.unsupported
    }

    /// The entries [`PortabilityCounts::degraded`] counts, in palette
    /// order.
    #[must_use]
    pub fn degraded(&self) -> &[DegradedEntry] {
        &self.degraded
    }

    /// Take both lists, for a caller that stores them rather than reading
    /// them in place.
    ///
    /// One method rather than an `into_` per list, because each would
    /// consume the report. Read [`Self::counts`] first.
    #[must_use]
    pub fn into_entries(self) -> PortabilityEntries {
        PortabilityEntries {
            degraded: self.degraded,
            unsupported: self.unsupported,
        }
    }

    /// Record an entry that compiles straight through.
    fn count_portable(&mut self) {
        self.counts.portable = self.counts.portable.saturating_add(1);
    }

    /// Record one entry that compiles and loses detail. The only place the
    /// `degraded` count is raised.
    fn push_degraded(&mut self, entry: DegradedEntry) {
        self.counts.degraded = self.counts.degraded.saturating_add(1);
        self.degraded.push(entry);
    }

    /// Record one entry the edition has no form for. The only place the
    /// `unsupported` count is raised.
    fn push_unsupported(&mut self, entry: UnsupportedEntry) {
        self.counts.unsupported = self.counts.unsupported.saturating_add(1);
        self.unsupported.push(entry);
    }
}

impl PortabilityCounts {
    /// Total of all three categories: every non-air palette entry counted.
    #[must_use]
    pub fn total(self) -> u32 {
        self.portable
            .saturating_add(self.degraded)
            .saturating_add(self.unsupported)
    }
}

/// Java is the base edition, so every non-air palette entry Java declares
/// counts as portable and `degraded` never fires.
///
/// `blocks` is the Java pack's id table: an entry naming a block no
/// supported Java version has is `unsupported`, as on Bedrock. A theme
/// resolving a slot to a Bedrock-only spelling puts one in a Java palette;
/// `aliases`, the pack's table of what Java calls that block, makes the row
/// actionable.
#[must_use]
pub fn portability_for_java(
    ir: &BlockArrayIr,
    blocks: &BlocksIndex,
    aliases: &AliasIndex,
) -> PortabilityReport {
    let mut report = PortabilityReport::default();
    for entry in non_air_entries(ir) {
        if absent_from_edition(blocks, entry) {
            report.push_unsupported(absent_entry(blocks, aliases, entry));
            continue;
        }
        report.count_portable();
    }
    report
}

/// Feed every non-air palette entry Bedrock declares through
/// [`translate_states`] and fold the outcome into per-category counts.
///
/// An entry Bedrock does not declare is `unsupported` without reaching
/// `translate_states`, which answers about states and would report a
/// stateless unknown id as a clean translation.
///
/// # Errors
///
/// [`InvalidPalette`] when an entry reaches the translator carrying a
/// blockstate no validated pack can produce: a value outside the Java
/// domain, or a key the backend does not read. The whole palette is still
/// walked, so the refusal names every such entry.
pub fn portability_for_bedrock(
    ir: &BlockArrayIr,
    blocks: &BlocksIndex,
    aliases: &AliasIndex,
) -> Result<PortabilityReport, InvalidPalette> {
    let mut report = PortabilityReport::default();
    let mut leaks = Vec::new();
    for entry in non_air_entries(ir) {
        if absent_from_edition(blocks, entry) {
            report.push_unsupported(absent_entry(blocks, aliases, entry));
            continue;
        }
        // No wildcard arm: a new refusal must be classified here.
        match translate_states(entry) {
            // Destructured without `..` for the same reason: a new field
            // of `StateTranslation` must be classified here rather than
            // land in `portable` silently.
            Ok(StateTranslation {
                states: _,
                degraded,
            }) if degraded.is_empty() => {
                report.count_portable();
            }
            // Carried rather than re-derived, so the rule stays in
            // `bedrock_state`.
            Ok(StateTranslation {
                states: _,
                degraded,
            }) => {
                report.push_degraded(DegradedEntry {
                    id: entry.id.clone(),
                    states: entry.properties_text(),
                    dropped: degraded,
                });
            }
            // The edition has the block but this backend cannot map its
            // states, which a build would hit too. Both lists come off the
            // error, so a newly mapped family reaches this row unedited.
            Err(BedrockStateError::UnmappableBlock {
                properties, mapped, ..
            }) => {
                report.push_unsupported(UnsupportedEntry {
                    id: entry.id.clone(),
                    reason: UnsupportedReason::StatesUnmapped {
                        states: properties,
                        mapped: mapped.to_owned(),
                    },
                });
            }
            // The two the pack should have refused; collected so the
            // refusal carries them all.
            Err(
                err @ (BedrockStateError::UnknownStairState { .. }
                | BedrockStateError::UnknownStairKey { .. }),
            ) => {
                leaks.push(err);
            }
        }
    }
    if leaks.is_empty() {
        Ok(report)
    } else {
        Err(InvalidPalette { leaks })
    }
}

/// The entry for a block the edition's tables refute, carrying both
/// answers the pack can give about it: what this edition calls the same
/// block, and the nearest id it declares when one is close enough to be a
/// typo.
///
/// Both come from the functions `E_UNKNOWN_ID` uses (the alias groups and
/// `cairn_lang_core::suggest::nearest_namespaced_id`), so the two commands
/// read a rename and a typo the same way. The scope differs: a pinned build
/// asks one version's table and this axis the whole edition, so an alias is
/// kept when *some* version declares it, and the two can answer differently
/// for the same id.
fn absent_entry(
    blocks: &BlocksIndex,
    aliases: &AliasIndex,
    state: &BlockState,
) -> UnsupportedEntry {
    UnsupportedEntry {
        id: state.id.clone(),
        reason: UnsupportedReason::AbsentFromEdition {
            suggestion: nearest_namespaced_id(&state.id, blocks.declared_ids()),
            aliases: aliases
                .spellings_of(&state.id)
                .iter()
                .filter(|spelling| blocks.declared_by_some_version(spelling) == Some(true))
                .cloned()
                .collect(),
        },
    }
}

/// Every authored palette entry across the IR's structures, in palette
/// order.
///
/// A walkway strip is its own `BlockArray` in the same `structures` map
/// (keyed `walkway::SITE::FROM__TO`), so a `connect ... path=@ID` material
/// is counted like a member's, as it resolves through the registry the same
/// way.
fn non_air_entries(ir: &BlockArrayIr) -> impl Iterator<Item = &BlockState> {
    ir.structures
        .values()
        .flat_map(|ba| ba.palette.entries.iter())
        .filter(|entry| !is_air(entry))
}

/// Whether the edition's tables prove no supported version has this block.
///
/// The single place the "cannot refute" rule lives, so neither counter can
/// read a pack with no `blocks` component as one where every id is absent.
fn absent_from_edition(blocks: &BlocksIndex, state: &BlockState) -> bool {
    blocks.declared_by_some_version(&state.id) == Some(false)
}

fn is_air(state: &BlockState) -> bool {
    state.id == BlockState::AIR_ID
}
