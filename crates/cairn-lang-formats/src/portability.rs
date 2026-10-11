//! Per-edition palette-entry portability counters.
//!
//! Backs the "edition portability" axis of `cairn info` — the "Which version
//! is it for?" question that `spec/versioning-editions` gives three answers
//! to. The counters run over a real lowered [`BlockArrayIr`] rather than over
//! the source, so each figure counts palette entries the matching
//! `cairn compile --edition X` emits, one for one.
//!
//! The two sides can still spell an entry differently. `info` pins no
//! target and so takes each material's default mapping, while a build takes
//! the target's, and a rename inside an edition's range makes those two ids
//! different strings for the same block. What must not differ is how many
//! entries there are, and `tests/portability/shipped_examples.rs` holds every
//! shipped example to that against every supported target.
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
//! question is "some version", not "every version". A rename inside an
//! edition's own range leaves each spelling valid for part of it — Bedrock
//! has `stonebrick` up to 1.21.0 and `stone_bricks` from 1.21.40 — and
//! neither spelling is absent from Bedrock. Whether the version actually
//! being built has the block is the narrower question, and
//! `cairn compile --target` is the command that knows which version that
//! is; it asks it as `E_UNKNOWN_ID`.
//!
//! A pack carrying no `blocks` component cannot refute an id, so this axis
//! stays silent for such a pack rather than reporting every entry as
//! unsupported — the same line the lowering pass draws.
//!
//! # Do the states survive?
//!
//! Java is the base edition per `spec/versioning-editions` "Backend = data
//! tables" (Java as the base, Bedrock as overriding diffs), so a palette
//! entry Java declares always counts as portable there — the Java writer
//! emits `properties` verbatim under the vanilla `.nbt` schema. Bedrock
//! funnels every entry through [`crate::bedrock_state::translate_states`] —
//! the same function the `.mcstructure` writer consumes when it emits bytes,
//! so this half of the classification cannot drift from what a build actually
//! does — and folds the outcome into `{portable, degraded, unsupported}`:
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
//! The translator's other two refusals are not answers about portability
//! at all, and their own docs say so. `UnknownStairState` is a value
//! outside the Java domain — "the registry pack should reject these one
//! layer up" — and `UnknownStairKey` a key the backend does not read.
//! Neither names anything the edition lacks; both name a blockstate that
//! should never have reached the translator.
//!
//! Counting one as `unsupported` would publish a validation gap as an
//! ordinary portability figure, indistinguishable from a stair whose
//! corner shape Bedrock simply has no state for. So the fold refuses
//! instead: [`portability_for_bedrock`] answers with [`InvalidPalette`],
//! naming every entry that leaked, and the caller reports the pack bug
//! rather than a count taken over a palette that should not exist. The
//! whole palette is walked before refusing, so one run names every leak
//! rather than the first.
//!
//! The counting granularity is per palette entry, matching the atomic unit
//! `translate_states` already exposes. A member whose lowering interns
//! several distinct palette entries (a gable roof with per-corner stairs)
//! contributes one row per interned entry, so the reported figures track
//! what the `.mcstructure` writer actually emits rather than a coarser
//! member-level abstraction.
//!
//! The [`BlockState::AIR_ID`] slot at palette index 0 is skipped — every
//! palette carries it by construction (`Palette::new_with_air`), and it
//! is not a member-authored intent that could be "unsupported".

use cairn_lang_core::block_array::{BlockArrayIr, BlockState};
use cairn_lang_core::resolve::{DegradedEntry, UnsupportedEntry, UnsupportedReason};
use cairn_lang_core::suggest::nearest_namespaced_id;

use thiserror::Error;

use crate::bedrock_state::{
    BedrockStateError, StateTranslation, join_properties, translate_states,
};
use crate::registry::{AliasIndex, BlocksIndex};

/// One edition's portability answer: the counts, and the entries behind
/// the `unsupported` one.
///
/// Returned in place of a bare [`PortabilityCounts`] rather than beside it
/// from a second entry point: the classification decides the category and
/// the reason in the same step, and two functions answering the same
/// question is the shape that lets them drift.
///
/// The fields are private and the count is only ever raised beside a push,
/// so the figure and the list describe the same entries for every value of
/// this type that can exist — including one built outside the crate. Public
/// fields would have made that a convention of this module rather than a
/// property of the type: `PortabilityReport { counts: ..unsupported: 7,
/// unsupported: vec![] }` is a legal literal, and this crate is published.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortabilityReport {
    counts: PortabilityCounts,
    degraded: Vec<DegradedEntry>,
    unsupported: Vec<UnsupportedEntry>,
}

/// Both of a [`PortabilityReport`]'s lists, moved out together.
///
/// Named fields rather than a pair, because the two are the same shape at
/// a glance — two lists of palette entries — and a tuple would make their
/// order the only thing telling them apart.
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
/// Fields are `u32` to match [`crate::registry`]'s wire types and the
/// `EditionPortability` shape in `cairn-lang-core::resolve`; the palette
/// sizes involved (single-digit counts on cottage-scale examples) are
/// nowhere near overflow.
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
/// Returned instead of counts because a figure taken over such a palette
/// is not an answer to the question the row asks. "How much of this ports
/// to Bedrock?" presumes the palette is one a build could have produced;
/// an entry the translator refuses for a state value outside the Java
/// domain, or for a key it does not read, says the palette is not. The
/// count would still be printable, and it would read as ordinary
/// portability — which is the one thing the reader must not conclude.
///
/// The leaks are the translator's own errors, unreworded, so a caller
/// prints the same sentence a build would have printed for the same entry.
/// Every one of them is carried rather than the first: they are all one
/// bug's symptoms, and a reader who fixes the pack wants the whole list.
///
/// The [`Display`](std::fmt::Display) impl below is what [`std::error::Error`]
/// requires and not the message a reader sees: the answer is the list, and
/// a caller renders it with its own framing. Nothing should grow a second
/// wording here to keep in step with that one.
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
    /// One method rather than an `into_` per list, because each of them
    /// would consume the report and a caller needs both. The counts are
    /// [`Copy`], so `counts()` is still there to be read first.
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

    /// Record one entry that compiles and loses detail, raising the count
    /// with it.
    ///
    /// The single place `degraded` is incremented, for the reason
    /// [`Self::push_unsupported`] is the single place the other is: the
    /// figure and the names are two views of one push rather than two
    /// tallies that have to agree.
    fn push_degraded(&mut self, entry: DegradedEntry) {
        self.counts.degraded = self.counts.degraded.saturating_add(1);
        self.degraded.push(entry);
    }

    /// Record one entry the edition has no form for, raising the count
    /// with it.
    ///
    /// The single place `unsupported` is incremented, which is what makes
    /// the figure and the names two views of one push rather than two
    /// tallies that have to agree.
    fn push_unsupported(&mut self, entry: UnsupportedEntry) {
        self.counts.unsupported = self.counts.unsupported.saturating_add(1);
        self.unsupported.push(entry);
    }
}

impl PortabilityCounts {
    /// Total of all three categories. Equivalent to the count of non-air
    /// palette entries reached by the underlying computation — a
    /// sanity-check callers can use to confirm no entry was silently
    /// dropped in the fold.
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
/// supported Java version has is `unsupported` here for the same reason it
/// would be on Bedrock. That is not hypothetical — a theme resolving a slot
/// to a Bedrock-only spelling puts one straight into a Java palette, and
/// `aliases` is the same pack's table of what Java calls that block, which
/// is what makes such a row actionable rather than a dead end.
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
/// blockstate no validated pack can produce — a value outside the Java
/// domain, or a key the backend does not read. Neither is a fact about
/// Bedrock, so neither has a category here, and a report built over the
/// rest of the palette would present a validation gap as a portability
/// figure. The whole palette is still walked, so the refusal names every
/// such entry rather than the first.
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
        // Matched variant by variant rather than through a wildcard: the
        // three refusals do not describe the same kind of failure, and a
        // fourth added later must be classified here rather than joining
        // whichever bucket a `_` arm points at.
        match translate_states(&entry.id, &entry.properties) {
            // Destructured without `..` for the reason the wildcard is
            // refused above: reading `degraded` alone would make these two
            // arms an effective wildcard over `StateTranslation`, and a
            // second kind of loss added to it would land in `portable`
            // silently. `portable` has no list, so the "the figure is the
            // length" invariant that guards the other two categories could
            // not notice. Without `..` the compiler raises it here.
            Ok(StateTranslation {
                states: _,
                degraded,
            }) if degraded.is_empty() => {
                report.count_portable();
            }
            // The translator's own answer, carried rather than recomputed:
            // which property it could not write is a rule that lives in
            // `bedrock_state`, and re-deriving it here would be a second
            // copy of that rule to fall out of step.
            Ok(StateTranslation {
                states: _,
                degraded,
            }) => {
                report.push_degraded(DegradedEntry {
                    id: entry.id.clone(),
                    states: join_properties(&entry.properties),
                    dropped: degraded,
                });
            }
            // The edition has the block and this backend has no mapping
            // for its states yet — `UnmappableBlock`'s own doc says "which
            // does not exist for this block yet", so the missing mapping
            // is the fact, and it is one a build would hit. Nothing is
            // reworded on the way through: both lists come off the error,
            // which threads them from the translator's own constants, so a
            // family added there reaches this row without a second edit.
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
            // The two the pack was supposed to have refused one layer up.
            // Collected rather than counted, and collected rather than
            // returned on the spot, so the refusal below carries the whole
            // list.
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
/// Both are the functions `E_UNKNOWN_ID` uses — the alias groups and
/// `cairn_lang_core::suggest::nearest_namespaced_id` — so the two commands
/// read a rename and a typo the same way. What differs is the scope: a
/// pinned build asks one version's table, and this axis asks of the
/// edition, so every version's ids are candidates here and an alias is kept
/// when *some* version declares it. The two can therefore answer
/// differently for the same id, and each is right about its own question.
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
/// A walkway strip is lowered into its own `BlockArray` stored under the
/// same `structures` map (keyed `walkway::SITE::FROM__TO`), so a
/// `connect ... path=@ID` is counted here like any member's material — and
/// it needs to be, because that material resolves through the registry
/// exactly the way a `mat_slot=` one does.
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
