//! Per-edition palette-entry portability counters.
//!
//! Backs the "edition portability" axis of `cairn info`
//! (spec versioning-editions §10.5). The counters are computed against
//! the lowered [`BlockArrayIr`] so they see exactly the palette the
//! matching `cairn compile --edition X` would write — the source of truth
//! is [`crate::bedrock_state::translate_states`], the same function the
//! `.mcstructure` writer consumes when it emits bytes.
//!
//! Java is the base edition per §10.3 ("Java as the base, Bedrock as
//! overriding diffs"), so every palette entry counts as portable there —
//! the Java writer emits `properties` verbatim under the vanilla `.nbt`
//! schema. Bedrock funnels every entry through `translate_states` and
//! folds the outcome into `{portable, degraded, unsupported}`:
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
    /// Palette entries with no representation on this edition (an unmapped
    /// stateful family, or a state value outside the Java domain).
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

/// Java is the base edition — every non-air palette entry counts as
/// portable, and neither `degraded` nor `unsupported` ever fires.
///
/// Kept as its own entry point (rather than a hard-coded zero at the call
/// site) so a future Java-side degradation surface — for instance an
/// on-disk schema change that renders some older intents unsupported —
/// lands in one file, and so the CLI can call the two editions through a
/// symmetric interface.
#[must_use]
pub fn portability_for_java(ir: &BlockArrayIr) -> PortabilityCounts {
    let mut counts = PortabilityCounts::default();
    for ba in ir.structures.values() {
        for entry in &ba.palette.entries {
            if is_air(entry) {
                continue;
            }
            counts.portable = counts.portable.saturating_add(1);
        }
    }
    counts
}

/// Feed every non-air palette entry through
/// [`translate_states`] and fold the outcome into per-category counts.
#[must_use]
pub fn portability_for_bedrock(ir: &BlockArrayIr) -> PortabilityCounts {
    let mut counts = PortabilityCounts::default();
    for ba in ir.structures.values() {
        for entry in &ba.palette.entries {
            if is_air(entry) {
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
    }
    counts
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

    fn stair_props(facing: &str, half: &str, shape: &str) -> IndexMap<String, String> {
        // Same insertion order the lowering uses so the palette entries
        // resemble what `translate_states` sees at runtime.
        let mut m = IndexMap::new();
        m.insert("facing".to_owned(), facing.to_owned());
        m.insert("half".to_owned(), half.to_owned());
        m.insert("shape".to_owned(), shape.to_owned());
        m
    }

    fn one_state_ir(states: Vec<BlockState>) -> BlockArrayIr {
        // Reuse a single 1×1×1 volume — the counters only inspect the
        // palette, so the voxel grid can stay minimal.
        let mut palette = Palette::new_with_air();
        for s in states {
            palette.intern(s);
        }
        let ba = BlockArray {
            dims: Dims { x: 1, y: 1, z: 1 },
            palette,
            voxels: vec![PaletteIndex::AIR],
            block_entities: Vec::new(),
            entities: Vec::new(),
            source_scope: "struct::probe".to_owned(),
        };
        let mut structures = IndexMap::new();
        structures.insert("struct::probe".to_owned(), ba);
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
        let counts = portability_for_bedrock(&ir);
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
        let counts = portability_for_java(&ir);
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
        assert_eq!(portability_for_java(&ir), PortabilityCounts::default());
        assert_eq!(portability_for_bedrock(&ir), PortabilityCounts::default());
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
        assert_eq!(portability_for_java(&ir), PortabilityCounts::default());
        assert_eq!(portability_for_bedrock(&ir), PortabilityCounts::default());
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
