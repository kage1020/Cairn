//! Per-edition palette-entry portability counters.
//!
//! Backs the "edition portability" axis of `cairn info`
//! (spec versioning-editions §10.5). The counters run over a real lowered
//! [`BlockArrayIr`] rather than over the source, so each figure counts
//! palette entries the matching `cairn compile --edition X` emits, one for
//! one.
//!
//! The two sides can still spell an entry differently. `info` pins no
//! target and so takes each material's default mapping, while a build takes
//! the target's, and a rename inside an edition's range makes those two ids
//! different strings for the same block. What must not differ is how many
//! entries there are, and `tests/example_portability.rs` holds every
//! shipped example to that against every supported target.
//!
//! Two independent questions decide an entry's category, asked in that
//! order.
//!
//! # Does the edition have the block at all?
//!
//! A palette entry can name a block the edition has never had: an authored
//! `@oak_sign` lowers verbatim on either edition, and Bedrock spells that
//! block `standing_sign`. Until
//! the registry pack grew per-version id tables there was nothing to ask
//! the question with, so a stateless id counted as portable whichever
//! edition it came from. An id no supported version of the edition declares
//! counts as **unsupported**, and the state question below is not asked —
//! there are no states to translate on a block that does not exist.
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
//! Java is the base edition per §10.3 ("Java as the base, Bedrock as
//! overriding diffs"), so a palette entry Java declares always counts as
//! portable there — the Java writer emits `properties` verbatim under the
//! vanilla `.nbt` schema. Bedrock funnels every entry through
//! [`crate::bedrock_state::translate_states`] — the same function the
//! `.mcstructure` writer consumes when it emits bytes, so this half of the
//! classification cannot drift from what a build actually does — and folds
//! the outcome into `{portable, degraded, unsupported}`:
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
use cairn_lang_core::resolve::{UnsupportedEntry, UnsupportedReason};
use cairn_lang_core::suggest::nearest_namespaced_id;

use thiserror::Error;

use crate::bedrock_state::{BedrockStateError, translate_states};
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
    unsupported: Vec<UnsupportedEntry>,
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

    /// Take the entries, for a caller that stores them rather than reading
    /// them in place.
    #[must_use]
    pub fn into_unsupported(self) -> Vec<UnsupportedEntry> {
        self.unsupported
    }

    /// Record an entry that compiles straight through.
    fn count_portable(&mut self) {
        self.counts.portable = self.counts.portable.saturating_add(1);
    }

    /// Record an entry that compiles and loses detail.
    fn count_degraded(&mut self) {
        self.counts.degraded = self.counts.degraded.saturating_add(1);
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
            Ok(t) if t.degraded.is_empty() => {
                report.count_portable();
            }
            Ok(_) => {
                report.count_degraded();
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

#[cfg(test)]
mod tests {
    use super::*;

    use cairn_lang_core::block_array::{
        BlockArray, BlockArrayIr, BlockState, Dims, Palette, PaletteIndex,
    };
    use indexmap::IndexMap;

    use crate::registry::{AliasCatalog, BlocksCatalog};

    /// A pack shipping no `aliases` component — every test that is not
    /// about renames uses this, so a group added to one of them is a
    /// deliberate change to what that test asks.
    fn no_aliases() -> AliasIndex {
        AliasIndex::empty()
    }

    /// The Bedrock report for a palette every test but the leak ones
    /// expects to be valid.
    ///
    /// Named rather than `.expect(..)` at thirty call sites: a leak is not
    /// a variation on a count, so a test asking about counts should not be
    /// able to be read as also asking about that.
    fn bedrock_report(
        ir: &BlockArrayIr,
        blocks: &BlocksIndex,
        aliases: &AliasIndex,
    ) -> PortabilityReport {
        portability_for_bedrock(ir, blocks, aliases)
            .expect("the palette carries no blockstate a pack would have refused")
    }

    /// A table shaped like a real edition's: two versions with a rename
    /// between them, so an id valid for only part of the range is available
    /// to test with alongside ids valid for all of it.
    fn table() -> BlocksIndex {
        let catalog: BlocksCatalog = serde_json::from_str(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": {
                "mc_version": "1.0",
                "blocks": ["oak_planks", "oak_stairs", "oak_door", "stonebrick"]
            },
            "diffs": [
                { "mc_version": "1.1", "inherits": "1.0",
                  "added": ["stone_bricks"], "removed": ["stonebrick"] }
            ]
        }"#,
        )
        .expect("test catalog parses as JSON");
        BlocksIndex::from_catalog(catalog).expect("test catalog folds")
    }

    fn stair_props(facing: &str, half: &str, shape: &str) -> IndexMap<String, String> {
        // Same insertion order the lowering uses so the palette entries
        // resemble what `translate_states` sees at runtime.
        let mut m = IndexMap::new();
        m.insert("facing".to_owned(), facing.to_owned());
        m.insert("half".to_owned(), half.to_owned());
        m.insert("shape".to_owned(), shape.to_owned());
        m
    }

    fn one_block_array(scope: &str, states: Vec<BlockState>) -> BlockArray {
        // Reuse a single 1×1×1 volume — the counters only inspect the
        // palette, so the voxel grid can stay minimal.
        let mut palette = Palette::new_with_air();
        for s in states {
            palette.intern(s);
        }
        BlockArray {
            dims: Dims { x: 1, y: 1, z: 1 },
            palette,
            voxels: vec![PaletteIndex::AIR],
            block_entities: Vec::new(),
            entities: Vec::new(),
            source_scope: scope.to_owned(),
        }
    }

    fn one_state_ir(states: Vec<BlockState>) -> BlockArrayIr {
        let mut structures = IndexMap::new();
        structures.insert(
            "struct::probe".to_owned(),
            one_block_array("struct::probe", states),
        );
        BlockArrayIr {
            structures,
            placements: IndexMap::new(),
            walkways: IndexMap::new(),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn ac11_bedrock_counts_bare_stair_corner_and_stateful_non_stair() {
        // AC11: palette with (bare oak_planks, straight stair, corner stair,
        // door with facing) folds to (portable: 2, degraded: 1, unsupported: 1).
        let ir = one_state_ir(vec![
            BlockState::bare("minecraft:oak_planks"),
            BlockState {
                id: "minecraft:oak_stairs".to_owned(),
                properties: stair_props("north", "bottom", "straight"),
            },
            BlockState {
                id: "minecraft:oak_stairs".to_owned(),
                properties: stair_props("south", "top", "outer_left"),
            },
            BlockState {
                id: "minecraft:oak_door".to_owned(),
                properties: {
                    let mut m = IndexMap::new();
                    m.insert("facing".to_owned(), "north".to_owned());
                    m
                },
            },
        ]);
        let counts = bedrock_report(&ir, &table(), &no_aliases()).counts;
        assert_eq!(
            counts,
            PortabilityCounts {
                portable: 2,
                degraded: 1,
                unsupported: 1,
            },
            "unexpected fold: {counts:?}",
        );
    }

    #[test]
    fn ac12_java_reports_every_non_air_entry_as_portable() {
        // AC12: Java always reports the full non-air palette as portable.
        // The palette mirrors AC11's — the Java axis must ignore the
        // Bedrock-only degradation signal.
        let ir = one_state_ir(vec![
            BlockState::bare("minecraft:oak_planks"),
            BlockState {
                id: "minecraft:oak_stairs".to_owned(),
                properties: stair_props("south", "top", "outer_left"),
            },
            BlockState {
                id: "minecraft:oak_door".to_owned(),
                properties: {
                    let mut m = IndexMap::new();
                    m.insert("facing".to_owned(), "north".to_owned());
                    m
                },
            },
        ]);
        let counts = portability_for_java(&ir, &table(), &no_aliases()).counts;
        assert_eq!(
            counts,
            PortabilityCounts {
                portable: 3,
                degraded: 0,
                unsupported: 0,
            },
        );
    }

    #[test]
    fn air_slot_is_skipped_by_both_editions() {
        // Every palette starts with air at index 0; neither edition should
        // count it as an authored intent.
        let ir = one_state_ir(Vec::new());
        assert_eq!(
            portability_for_java(&ir, &table(), &no_aliases()).counts,
            PortabilityCounts::default()
        );
        assert_eq!(
            bedrock_report(&ir, &table(), &no_aliases()).counts,
            PortabilityCounts::default()
        );
    }

    #[test]
    fn an_id_the_edition_never_declares_is_unsupported_on_both_sides() {
        // A stateless id reaches `translate_states` as an empty property
        // map and comes back clean, so before the tables were consulted
        // this counted as portable — on either edition, since the Java
        // side asked nothing at all.
        let ir = one_state_ir(vec![
            BlockState::bare("minecraft:oak_planks"),
            BlockState::bare("minecraft:totally_not_a_block"),
        ]);
        let blocks = table();
        assert_eq!(
            bedrock_report(&ir, &blocks, &no_aliases()).counts,
            PortabilityCounts {
                portable: 1,
                degraded: 0,
                unsupported: 1,
            },
        );
        assert_eq!(
            portability_for_java(&ir, &blocks, &no_aliases()).counts,
            PortabilityCounts {
                portable: 1,
                degraded: 0,
                unsupported: 1,
            },
        );
    }

    #[test]
    fn an_id_only_part_of_the_range_declares_is_not_unsupported() {
        // Both spellings of the renamed block are valid somewhere in the
        // range, so neither is absent from the edition. Counting them
        // unsupported would mark a build that succeeds on the target the
        // author picked as impossible on the edition entirely.
        let ir = one_state_ir(vec![
            BlockState::bare("minecraft:stonebrick"),
            BlockState::bare("minecraft:stone_bricks"),
        ]);
        let blocks = table();
        assert_eq!(
            bedrock_report(&ir, &blocks, &no_aliases()).counts,
            PortabilityCounts {
                portable: 2,
                degraded: 0,
                unsupported: 0,
            },
        );
        assert_eq!(
            portability_for_java(&ir, &blocks, &no_aliases()).counts,
            PortabilityCounts {
                portable: 2,
                degraded: 0,
                unsupported: 0,
            },
        );
    }

    #[test]
    fn an_absent_id_with_unsupported_states_is_counted_once() {
        // The door's `facing` has no Bedrock mapping, so both questions
        // would answer "unsupported". The entry is one palette row and
        // must contribute one count, or `total()` stops matching the
        // palette it was computed from.
        let mut properties = IndexMap::new();
        properties.insert("facing".to_owned(), "north".to_owned());
        assert!(
            translate_states("minecraft:spruce_door", &properties).is_err(),
            "premise: the states alone would already be unsupported",
        );
        let ir = one_state_ir(vec![BlockState {
            id: "minecraft:spruce_door".to_owned(),
            properties,
        }]);
        let counts = bedrock_report(&ir, &table(), &no_aliases()).counts;
        assert_eq!(
            counts,
            PortabilityCounts {
                portable: 0,
                degraded: 0,
                unsupported: 1,
            },
        );
        assert_eq!(counts.total(), 1, "one palette entry, one count");
    }

    #[test]
    fn an_absent_id_whose_states_would_degrade_is_still_unsupported() {
        // The two questions disagree here: the states translate (with loss)
        // while the block does not exist. Asking them in the other order
        // files the entry as `degraded`, which reads as "builds, looks
        // slightly wrong" for something that does not build at all — and
        // `total()` is identical either way, so no parity assertion notices.
        let properties = stair_props("south", "top", "outer_left");
        let translated = translate_states("minecraft:spruce_stairs", &properties)
            .expect("premise: the states translate");
        assert!(
            !translated.degraded.is_empty(),
            "premise: the states translate with loss, so `degraded` is the competing answer",
        );
        let ir = one_state_ir(vec![BlockState {
            id: "minecraft:spruce_stairs".to_owned(),
            properties,
        }]);
        assert_eq!(
            bedrock_report(&ir, &table(), &no_aliases()).counts,
            PortabilityCounts {
                portable: 0,
                degraded: 0,
                unsupported: 1,
            },
        );
    }

    #[test]
    fn a_pack_with_no_id_tables_refutes_nothing() {
        // `BlocksIndex::empty()` is what a pack shipping no `blocks`
        // component loads with. Reading it as "no version declares this"
        // would report every entry of every source as unsupported — the
        // inverse of the rule the lowering pass follows for the same
        // absence.
        let ir = one_state_ir(vec![
            BlockState::bare("minecraft:oak_planks"),
            BlockState::bare("minecraft:totally_not_a_block"),
        ]);
        let none = BlocksIndex::empty();
        assert_eq!(
            portability_for_java(&ir, &none, &no_aliases()).counts,
            PortabilityCounts {
                portable: 2,
                degraded: 0,
                unsupported: 0,
            },
        );
        assert_eq!(
            bedrock_report(&ir, &none, &no_aliases()).counts,
            PortabilityCounts {
                portable: 2,
                degraded: 0,
                unsupported: 0,
            },
        );
    }

    #[test]
    fn a_walkway_strip_is_counted_like_any_other_entry() {
        // Walkway strips are lowered into their own `BlockArray` under the
        // same `structures` map, keyed `walkway::SITE::FROM__TO`. Counting
        // only member structures would leave `connect ... path=@ID`
        // unreported, and a walkway of a block the edition lacks is exactly
        // the kind of gap that reads as a routing decision in-game.
        let mut structures = IndexMap::new();
        structures.insert(
            "site::duo::home1".to_owned(),
            one_block_array(
                "site::duo::home1",
                vec![BlockState::bare("minecraft:oak_planks")],
            ),
        );
        structures.insert(
            "walkway::duo::a.entry__b.entry".to_owned(),
            one_block_array(
                "walkway::duo::a.entry__b.entry",
                vec![BlockState::bare("minecraft:totally_not_a_block")],
            ),
        );
        let ir = BlockArrayIr {
            structures,
            placements: IndexMap::new(),
            walkways: IndexMap::new(),
            diagnostics: Vec::new(),
        };
        let report = bedrock_report(&ir, &table(), &no_aliases());
        assert_eq!(
            report.counts(),
            PortabilityCounts {
                portable: 1,
                degraded: 0,
                unsupported: 1,
            },
        );
        // And the reason, which the figure alone cannot hold: asking the
        // id question after the states one keeps `unsupported: 1` while
        // reversing the advice from "this block is not here, did you mean
        // ...?" to "the block is here and its states are not".
        assert_eq!(
            only_reason(&report),
            &UnsupportedReason::AbsentFromEdition {
                suggestion: None,
                aliases: Vec::new(),
            },
        );
    }

    #[test]
    fn empty_ir_produces_zero_counts() {
        // A file with no structures (`ir.structures.is_empty()`) is not
        // reachable from `cairn lower` today, but the counters must survive
        // it without panicking so future entry points can call them freely.
        let ir = BlockArrayIr {
            structures: IndexMap::new(),
            placements: IndexMap::new(),
            walkways: IndexMap::new(),
            diagnostics: Vec::new(),
        };
        assert_eq!(
            portability_for_java(&ir, &table(), &no_aliases()).counts,
            PortabilityCounts::default()
        );
        assert_eq!(
            bedrock_report(&ir, &table(), &no_aliases()).counts,
            PortabilityCounts::default()
        );
    }

    /// The reason of the single unsupported entry `report` names, or a
    /// panic naming what it found instead.
    fn only_reason(report: &PortabilityReport) -> &UnsupportedReason {
        assert_eq!(
            report.unsupported().len(),
            1,
            "expected exactly one named entry, got {:?}",
            report.unsupported(),
        );
        &report.unsupported()[0].reason
    }

    #[test]
    fn an_absent_id_is_named_with_the_nearest_id_the_edition_declares() {
        // The count says one entry has no form here; the entry says which
        // one, and the suggestion says what to write instead. The typo is
        // one the edit cap admits, so the suggestion is the point of the
        // test rather than incidental to it.
        let ir = one_state_ir(vec![BlockState::bare("minecraft:oak_planck")]);
        let report = portability_for_java(&ir, &table(), &no_aliases());
        assert_eq!(report.counts().unsupported, 1);
        assert_eq!(report.unsupported()[0].id, "minecraft:oak_planck");
        assert_eq!(
            only_reason(&report),
            &UnsupportedReason::AbsentFromEdition {
                suggestion: Some("minecraft:oak_planks".to_owned()),
                aliases: Vec::new(),
            },
        );
    }

    #[test]
    fn a_spelling_only_the_other_edition_has_is_named_under_both() {
        // A rename inside one edition's own range is not this case: the id
        // is absent from *every* version the table describes. Asked of
        // both counters because the id half of the question is the one
        // thing Java also asks, and a Bedrock-only spelling reaching a
        // Java palette is the way it gets asked in practice.
        let ir = one_state_ir(vec![BlockState::bare("minecraft:standing_sign")]);
        for report in [
            portability_for_java(&ir, &table(), &no_aliases()),
            bedrock_report(&ir, &table(), &no_aliases()),
        ] {
            assert_eq!(report.counts().unsupported, 1);
            assert_eq!(report.unsupported()[0].id, "minecraft:standing_sign");
            assert!(
                matches!(
                    only_reason(&report),
                    UnsupportedReason::AbsentFromEdition { .. }
                ),
                "got {:?}",
                report.unsupported()[0].reason,
            );
        }
    }

    /// The alias table for [`table`]: the one rename it describes, plus
    /// the cross-edition spelling of a block the table has under another
    /// name.
    fn aliases() -> AliasIndex {
        let catalog: AliasCatalog = serde_json::from_str(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "groups": [
                { "spellings": ["stonebrick", "stone_bricks"] },
                { "spellings": ["standing_sign", "oak_sign"] }
            ]
        }"#,
        )
        .expect("test catalog parses as JSON");
        AliasIndex::from_catalog(catalog).expect("test catalog folds")
    }

    /// An id this edition has under another name is reported with that
    /// name, which is the answer the distance search cannot reach.
    ///
    /// `standing_sign` is seven edits from `oak_sign` and the same block.
    /// Without the alias table the row says the edition does not have it
    /// and stops; that sentence is true about the id and useless about the
    /// build.
    #[test]
    fn a_renamed_id_is_reported_with_the_name_this_edition_uses() {
        let ir = one_state_ir(vec![BlockState::bare("minecraft:standing_sign")]);
        let with_table = portability_for_java(&ir, &table_with_oak_sign(), &aliases());
        assert_eq!(
            only_reason(&with_table),
            &UnsupportedReason::AbsentFromEdition {
                suggestion: None,
                aliases: vec!["minecraft:oak_sign".to_owned()],
            },
        );
    }

    /// The alias question is asked of the edition, not of one version.
    ///
    /// `stonebrick` is declared by `1.0` alone and `stone_bricks` by `1.1`
    /// alone, and this axis reports across the range — so each is the
    /// other's answer here, where a pinned build would answer with only the
    /// one its own version has.
    #[test]
    fn an_alias_any_version_declares_counts_for_the_edition() {
        let ir = one_state_ir(vec![BlockState::bare("minecraft:chiseled_stone_bricks")]);
        let catalog: AliasCatalog = serde_json::from_str(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "groups": [{
                "spellings": ["chiseled_stone_bricks", "stonebrick", "stone_bricks"]
            }]
        }"#,
        )
        .expect("test catalog parses as JSON");
        let index = AliasIndex::from_catalog(catalog).expect("test catalog folds");
        let report = portability_for_java(&ir, &table(), &index);
        assert_eq!(
            only_reason(&report),
            &UnsupportedReason::AbsentFromEdition {
                suggestion: None,
                aliases: vec![
                    "minecraft:stonebrick".to_owned(),
                    "minecraft:stone_bricks".to_owned(),
                ],
            },
            "both spellings are the edition's, one version each",
        );
    }

    /// A group whose other spellings this edition does not have either is
    /// no answer, and the row falls back to what it said before.
    #[test]
    fn an_alias_this_edition_lacks_is_not_offered() {
        let ir = one_state_ir(vec![BlockState::bare("minecraft:standing_sign")]);
        let report = portability_for_java(&ir, &table(), &aliases());
        assert_eq!(
            only_reason(&report),
            &UnsupportedReason::AbsentFromEdition {
                suggestion: None,
                aliases: Vec::new(),
            },
            "`oak_sign` is no more in this table than `standing_sign` is",
        );
    }

    /// [`table`] plus the one id the alias tests need it to declare.
    fn table_with_oak_sign() -> BlocksIndex {
        let catalog: BlocksCatalog = serde_json::from_str(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": {
                "mc_version": "1.0",
                "blocks": ["oak_planks", "oak_stairs", "oak_door", "oak_sign", "stonebrick"]
            },
            "diffs": [
                { "mc_version": "1.1", "inherits": "1.0",
                  "added": ["stone_bricks"], "removed": ["stonebrick"] }
            ]
        }"#,
        )
        .expect("test catalog parses as JSON");
        BlocksIndex::from_catalog(catalog).expect("test catalog folds")
    }

    #[test]
    fn an_id_nothing_resembles_is_named_without_a_suggestion() {
        // Failing to find a suggestion must not cost the name. An id this
        // far from every candidate is the ordinary case for a block that
        // belongs to the other edition entirely.
        let ir = one_state_ir(vec![BlockState::bare("minecraft:totally_not_a_block")]);
        let report = bedrock_report(&ir, &table(), &no_aliases());
        assert_eq!(report.unsupported()[0].id, "minecraft:totally_not_a_block");
        assert_eq!(
            only_reason(&report),
            &UnsupportedReason::AbsentFromEdition {
                suggestion: None,
                aliases: Vec::new(),
            },
        );
    }

    #[test]
    fn a_suggestion_is_scored_on_the_path_and_not_on_the_whole_id() {
        // `nearest_match`'s edit cap scales with input length, and every
        // vanilla id shares the ten-character `minecraft:` prefix — long
        // enough to buy an edit the typed part never earned. `ok_dr` is
        // three edits from `oak_door`, which a five-character input does
        // not admit and a fifteen-character one would.
        let ir = one_state_ir(vec![BlockState::bare("minecraft:ok_dr")]);
        let report = portability_for_java(&ir, &table(), &no_aliases());
        assert_eq!(
            only_reason(&report),
            &UnsupportedReason::AbsentFromEdition {
                suggestion: None,
                aliases: Vec::new(),
            },
        );
    }

    #[test]
    fn a_suggestion_can_come_from_any_version_the_edition_declares() {
        // The candidates are every id the edition has, not the oldest
        // version's list: this axis asks of the edition, and a block added
        // partway through the range is a block the edition has. `table()`
        // adds `stone_bricks` in its diff and removes `stonebrick` there,
        // so the base-only reading has a plausible wrong answer to give
        // (`stonebrick`, two edits away) rather than no answer at all.
        //
        // Asked in both directions. `stone_bricks` is in the diff and not
        // the base, `stonebrick` is in the base and not the diff, and each
        // is the nearest candidate to a different typo — so neither a
        // base-only nor a newest-only reading can pass both halves.
        for (typo, nearest) in [
            ("minecraft:stone_brickz", "minecraft:stone_bricks"),
            ("minecraft:stonebrik", "minecraft:stonebrick"),
        ] {
            let ir = one_state_ir(vec![BlockState::bare(typo)]);
            let report = bedrock_report(&ir, &table(), &no_aliases());
            assert_eq!(
                only_reason(&report),
                &UnsupportedReason::AbsentFromEdition {
                    suggestion: Some(nearest.to_owned()),
                    aliases: Vec::new(),
                },
                "for {typo}",
            );
        }
    }

    #[test]
    fn a_suggestion_is_never_drawn_from_the_other_namespace() {
        // `cairn_lang_core::suggest::nearest_namespaced_id` compares paths
        // within one namespace: a `mod:oak_planck` is not repaired by
        // `minecraft:oak_planks`, because the pack that would have to
        // declare it is the mod's.
        let ir = one_state_ir(vec![BlockState::bare("mod:oak_planck")]);
        let report = portability_for_java(&ir, &table(), &no_aliases());
        assert_eq!(
            only_reason(&report),
            &UnsupportedReason::AbsentFromEdition {
                suggestion: None,
                aliases: Vec::new(),
            },
        );
    }

    #[test]
    fn a_block_the_backend_maps_no_states_for_is_named_by_the_states_it_carries() {
        // The block exists on the edition, so the repair is not to the
        // material — a different answer from the absent case, and the
        // states are what says so. `mapped` comes off the error rather
        // than being restated here, so the day a second family is mapped
        // this row follows without an edit.
        let mut properties = IndexMap::new();
        properties.insert("facing".to_owned(), "north".to_owned());
        let ir = one_state_ir(vec![BlockState {
            id: "minecraft:oak_door".to_owned(),
            properties,
        }]);
        let report = bedrock_report(&ir, &table(), &no_aliases());
        assert_eq!(report.counts().unsupported, 1);
        assert_eq!(report.unsupported()[0].id, "minecraft:oak_door");
        assert_eq!(
            only_reason(&report),
            &UnsupportedReason::StatesUnmapped {
                states: "facing=north".to_owned(),
                mapped: "the stair family".to_owned(),
            },
        );
    }

    #[test]
    fn a_state_value_outside_the_java_domain_refuses_the_report() {
        // A value the pack was expected to reject is not a fact about
        // Bedrock. Counting it would put a validation gap in the same
        // figure as a stair whose corner shape Bedrock has no state for,
        // where the two have nothing in common but the number they land
        // in — so the palette is refused and no figure is offered for it.
        let ir = one_state_ir(vec![BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("up", "bottom", "straight"),
        }]);
        let invalid = portability_for_bedrock(&ir, &table(), &no_aliases())
            .expect_err("a leaked state value refuses the report");
        assert_eq!(
            invalid.leaks(),
            [BedrockStateError::UnknownStairState {
                id: "minecraft:oak_stairs".to_owned(),
                key: "facing",
                value: "up".to_owned(),
                valid: "east, west, south, north",
            }],
        );
    }

    #[test]
    fn a_state_key_the_backend_does_not_read_refuses_the_report() {
        // The neighbouring leak, asked separately: the two arrive through
        // different translator branches, and one arm folded into the other
        // would leave a variant no test could tell apart from its sibling.
        let ir = one_state_ir(vec![BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: {
                let mut m = stair_props("north", "bottom", "straight");
                m.insert("waterlogged".to_owned(), "true".to_owned());
                m
            },
        }]);
        let invalid = portability_for_bedrock(&ir, &table(), &no_aliases())
            .expect_err("a leaked state key refuses the report");
        assert_eq!(
            invalid.leaks(),
            [BedrockStateError::UnknownStairKey {
                id: "minecraft:oak_stairs".to_owned(),
                key: "waterlogged".to_owned(),
                handled: "facing, half, shape",
            }],
        );
    }

    #[test]
    fn a_leak_refuses_the_whole_palette_and_names_every_one_of_them() {
        // Two things at once, because they are the same decision. The
        // palette's other entries classify perfectly well — a portable
        // block, a degraded stair, and an entry that really is
        // unsupported — and none of those counts is reported, because a
        // figure over a palette this one entry proves invalid is not an
        // answer to "how much of this ports".
        //
        // And the walk does not stop at the first leak. They are one
        // bug's symptoms, and a reader repairing the pack wants the whole
        // list rather than one per run.
        let ir = one_state_ir(vec![
            BlockState::bare("minecraft:oak_planks"),
            BlockState {
                id: "minecraft:oak_stairs".to_owned(),
                properties: stair_props("south", "top", "outer_left"),
            },
            BlockState::bare("minecraft:nothing_like_this"),
            BlockState {
                id: "minecraft:oak_stairs".to_owned(),
                properties: stair_props("up", "bottom", "straight"),
            },
            BlockState {
                id: "minecraft:oak_stairs".to_owned(),
                properties: {
                    let mut m = stair_props("north", "top", "straight");
                    m.insert("waterlogged".to_owned(), "true".to_owned());
                    m
                },
            },
        ]);
        let invalid = portability_for_bedrock(&ir, &table(), &no_aliases())
            .expect_err("one leak refuses the palette however well the rest classifies");
        let leaked: Vec<String> = invalid.leaks().iter().map(ToString::to_string).collect();
        assert_eq!(
            leaked.len(),
            2,
            "every leak is named, not the first: {leaked:?}",
        );
        assert!(
            leaked[0].contains("facing=up") && leaked[1].contains("waterlogged"),
            "in palette order, unreworded: {leaked:?}",
        );
        // The premise: without the two leaked entries the same palette
        // classifies into all three categories, so the refusal is what
        // withheld them rather than there being nothing to withhold.
        let clean = one_state_ir(vec![
            BlockState::bare("minecraft:oak_planks"),
            BlockState {
                id: "minecraft:oak_stairs".to_owned(),
                properties: stair_props("south", "top", "outer_left"),
            },
            BlockState::bare("minecraft:nothing_like_this"),
        ]);
        assert_eq!(
            bedrock_report(&clean, &table(), &no_aliases()).counts(),
            PortabilityCounts {
                portable: 1,
                degraded: 1,
                unsupported: 1,
            },
        );
    }

    #[test]
    fn a_block_the_backend_maps_no_states_for_is_not_a_leak() {
        // The third refusal is the one that stays a count. The block is
        // on the edition and the states have no Bedrock form *yet* — a
        // fact about this backend that a build would hit and that no pack
        // validation could have prevented, which is what separates it
        // from the two above.
        let mut properties = IndexMap::new();
        properties.insert("facing".to_owned(), "north".to_owned());
        let ir = one_state_ir(vec![BlockState {
            id: "minecraft:oak_door".to_owned(),
            properties,
        }]);
        let report = portability_for_bedrock(&ir, &table(), &no_aliases())
            .expect("an unmapped family is a portability answer, not a leak");
        assert_eq!(report.counts().unsupported, 1);
    }

    #[test]
    fn a_degraded_entry_is_counted_and_not_named() {
        // Degraded entries have the same "which of the N" problem, and
        // this row deliberately does not answer it: the list is what the
        // `unsupported` figure counts, and nothing else, so a consumer can
        // read one against the other.
        let ir = one_state_ir(vec![BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("south", "top", "outer_left"),
        }]);
        let report = bedrock_report(&ir, &table(), &no_aliases());
        assert_eq!(report.counts().degraded, 1);
        assert!(
            report.unsupported().is_empty(),
            "got {:?}",
            report.unsupported(),
        );
    }

    #[test]
    fn the_named_entries_are_the_counted_ones_in_palette_order() {
        // One of each category, with the two unsupported ones separated by
        // entries that are not, so the order asserted is the palette's and
        // not an artefact of them being adjacent.
        let ir = one_state_ir(vec![
            BlockState::bare("minecraft:no_such_block_at_all"),
            BlockState::bare("minecraft:oak_planks"),
            BlockState {
                id: "minecraft:oak_stairs".to_owned(),
                properties: stair_props("south", "top", "outer_left"),
            },
            BlockState {
                id: "minecraft:oak_door".to_owned(),
                properties: {
                    let mut m = IndexMap::new();
                    m.insert("facing".to_owned(), "north".to_owned());
                    m
                },
            },
        ]);
        let report = bedrock_report(&ir, &table(), &no_aliases());
        assert_eq!(
            report.counts(),
            PortabilityCounts {
                portable: 1,
                degraded: 1,
                unsupported: 2,
            },
        );
        let named: Vec<&str> = report
            .unsupported
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(
            named,
            ["minecraft:no_such_block_at_all", "minecraft:oak_door"]
        );
        // The pairing invariant, asked where entries actually exist: over
        // a palette with none of them, "pushed without counting" and
        // "counted without pushing" both read as 0 == 0.
        assert_eq!(
            report.unsupported().len(),
            report.counts().unsupported as usize,
            "the list and the figure count the same entries",
        );
    }

    #[test]
    fn a_pack_that_cannot_refute_an_id_names_nothing() {
        // The "cannot refute" rule is what keeps a pack with no `blocks`
        // component from reporting every id as absent. The list has to
        // follow the count through that rule rather than be built from a
        // second, laxer reading of the same table.
        let ir = one_state_ir(vec![BlockState::bare("minecraft:totally_not_a_block")]);
        let none = BlocksIndex::empty();
        for report in [
            portability_for_java(&ir, &none, &no_aliases()),
            bedrock_report(&ir, &none, &no_aliases()),
        ] {
            assert_eq!(report.counts().unsupported, 0);
            assert!(
                report.unsupported().is_empty(),
                "got {:?}",
                report.unsupported()
            );
        }
    }

    #[test]
    fn total_returns_sum_of_categories() {
        let counts = PortabilityCounts {
            portable: 3,
            degraded: 2,
            unsupported: 1,
        };
        assert_eq!(counts.total(), 6);
    }
}
