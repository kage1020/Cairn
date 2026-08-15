//! Per-edition palette-entry portability counters.
//!
//! Backs the "edition portability" axis of `cairn info`
//! (spec versioning-editions §10.5). The counters are computed against
//! the lowered [`BlockArrayIr`] so they see exactly the palette the
//! matching `cairn compile --edition X` would write — the source of truth
//! is [`crate::bedrock_state::translate_states`], the same function the
//! `.mcstructure` writer consumes when it emits bytes.
//!
//! Two independent questions decide an entry's category, asked in that
//! order.
//!
//! # Does the edition have the block at all?
//!
//! The palette an entry lands in is the one the matching
//! `cairn compile --edition X` would write, but it can still name a block
//! that edition has never had: an authored `@oak_sign` reaches a Bedrock
//! palette unchanged, and Bedrock spells that block `standing_sign`. Until
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
//! `translate_states` and folds the outcome into
//! `{portable, degraded, unsupported}`:
//!
//! - `Ok(StateTranslation { degraded: [], .. })` → **portable** (the intent
//!   round-trips into Bedrock states with no loss).
//! - `Ok(StateTranslation { degraded: non-empty, .. })` → **degraded** (the
//!   intent compiles but loses detail — e.g. a corner stair `shape` that
//!   Bedrock has no state for; the `.mcstructure` writer surfaces this as
//!   `W_INTENT_DEGRADED`).
//! - `Err(BedrockStateError)` → **unsupported** (no representation on
//!   Bedrock — an unmapped stateful family or an out-of-domain value that
//!   the writer would refuse).
//!
//! The counting granularity is per palette entry, matching the atomic unit
//! `translate_states` already exposes. A member whose lowering interns
//! several distinct palette entries (a gable roof with per-corner stairs)
//! contributes one row per interned entry, so the reported figures track
//! what the `.mcstructure` writer actually emits rather than a coarser
//! member-level abstraction.
//!
//! The [`BlockState::AIR`] slot at palette index 0 is skipped — every
//! palette carries it by construction (`Palette::new_with_air`), and it
//! is not a member-authored intent that could be "unsupported".

use cairn_lang_core::block_array::{BlockArrayIr, BlockState};

use crate::bedrock_state::translate_states;
use crate::registry::BlocksIndex;

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
    /// supported version of the edition declares, an unmapped stateful
    /// family, or a state value outside the Java domain.
    pub unsupported: u32,
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
/// to a Bedrock-only spelling puts one straight into a Java palette.
#[must_use]
pub fn portability_for_java(ir: &BlockArrayIr, blocks: &BlocksIndex) -> PortabilityCounts {
    let mut counts = PortabilityCounts::default();
    for entry in non_air_entries(ir) {
        if absent_from_edition(blocks, entry) {
            counts.unsupported = counts.unsupported.saturating_add(1);
            continue;
        }
        counts.portable = counts.portable.saturating_add(1);
    }
    counts
}

/// Feed every non-air palette entry Bedrock declares through
/// [`translate_states`] and fold the outcome into per-category counts.
///
/// An entry Bedrock does not declare is `unsupported` without reaching
/// `translate_states`, which answers about states and would report a
/// stateless unknown id as a clean translation.
#[must_use]
pub fn portability_for_bedrock(ir: &BlockArrayIr, blocks: &BlocksIndex) -> PortabilityCounts {
    let mut counts = PortabilityCounts::default();
    for entry in non_air_entries(ir) {
        if absent_from_edition(blocks, entry) {
            counts.unsupported = counts.unsupported.saturating_add(1);
            continue;
        }
        match translate_states(&entry.id, &entry.properties) {
            Ok(t) if t.degraded.is_empty() => {
                counts.portable = counts.portable.saturating_add(1);
            }
            Ok(_) => {
                counts.degraded = counts.degraded.saturating_add(1);
            }
            Err(_) => {
                counts.unsupported = counts.unsupported.saturating_add(1);
            }
        }
    }
    counts
}

/// Every authored palette entry across the IR's structures, in palette
/// order. Walkway strips carry their own [`crate::registry`]-independent
/// `BlockArray` under the same map, so a `connect ... path=@ID` is counted
/// like any member's material.
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

    use crate::registry::BlocksCatalog;

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
        let counts = portability_for_bedrock(&ir, &table());
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
        let counts = portability_for_java(&ir, &table());
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
            portability_for_java(&ir, &table()),
            PortabilityCounts::default()
        );
        assert_eq!(
            portability_for_bedrock(&ir, &table()),
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
            portability_for_bedrock(&ir, &blocks),
            PortabilityCounts {
                portable: 1,
                degraded: 0,
                unsupported: 1,
            },
        );
        assert_eq!(
            portability_for_java(&ir, &blocks),
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
            portability_for_bedrock(&ir, &blocks),
            PortabilityCounts {
                portable: 2,
                degraded: 0,
                unsupported: 0,
            },
        );
        assert_eq!(
            portability_for_java(&ir, &blocks),
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
        let counts = portability_for_bedrock(&ir, &table());
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
            portability_for_java(&ir, &none),
            PortabilityCounts {
                portable: 2,
                degraded: 0,
                unsupported: 0,
            },
        );
        assert_eq!(
            portability_for_bedrock(&ir, &none),
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
        assert_eq!(
            portability_for_bedrock(&ir, &table()),
            PortabilityCounts {
                portable: 1,
                degraded: 0,
                unsupported: 1,
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
            portability_for_java(&ir, &table()),
            PortabilityCounts::default()
        );
        assert_eq!(
            portability_for_bedrock(&ir, &table()),
            PortabilityCounts::default()
        );
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
