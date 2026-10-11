//! Per-edition palette-entry portability for `cairn info`, counted over the lowered IR so each
//! figure matches what `cairn compile --edition` emits. An id no version of the edition declares
//! is unsupported; on Bedrock the rest go through the `.mcstructure` writer's state translation.

use cairn_lang_core::block_array::{BlockArrayIr, BlockState};
use cairn_lang_core::resolve::{DegradedEntry, UnsupportedEntry, UnsupportedReason};
use cairn_lang_core::suggest::nearest_namespaced_id;

use thiserror::Error;

use crate::bedrock_state::{BedrockStateError, StateTranslation, translate_states};
use crate::registry::{AliasIndex, BlocksIndex};

/// One edition's portability counts and the entries behind them. The fields are private so each
/// count always matches its list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortabilityReport {
    counts: PortabilityCounts,
    degraded: Vec<DegradedEntry>,
    unsupported: Vec<UnsupportedEntry>,
}

/// Both of a [`PortabilityReport`]'s lists, moved out together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortabilityEntries {
    /// The entries [`PortabilityCounts::degraded`] counts, in palette order.
    pub degraded: Vec<DegradedEntry>,
    /// The entries [`PortabilityCounts::unsupported`] counts, in palette order.
    pub unsupported: Vec<UnsupportedEntry>,
}

/// Palette-entry counts per portability category.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PortabilityCounts {
    /// Entries that translate losslessly.
    pub portable: u32,
    /// Entries that translate but lose detail, which a build reports as `W_INTENT_DEGRADED`.
    pub degraded: u32,
    /// Entries the edition cannot represent: an undeclared block, or states with no mapping.
    pub unsupported: u32,
}

/// A palette carrying blockstates the registry pack should have refused, with every such entry.
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

    /// The entries [`PortabilityCounts::unsupported`] counts, in palette order.
    #[must_use]
    pub fn unsupported(&self) -> &[UnsupportedEntry] {
        &self.unsupported
    }

    /// The entries [`PortabilityCounts::degraded`] counts, in palette order.
    #[must_use]
    pub fn degraded(&self) -> &[DegradedEntry] {
        &self.degraded
    }

    /// Both lists, moved out; read [`Self::counts`] first.
    #[must_use]
    pub fn into_entries(self) -> PortabilityEntries {
        PortabilityEntries {
            degraded: self.degraded,
            unsupported: self.unsupported,
        }
    }

    fn count_portable(&mut self) {
        self.counts.portable = self.counts.portable.saturating_add(1);
    }

    fn push_degraded(&mut self, entry: DegradedEntry) {
        self.counts.degraded = self.counts.degraded.saturating_add(1);
        self.degraded.push(entry);
    }

    fn push_unsupported(&mut self, entry: UnsupportedEntry) {
        self.counts.unsupported = self.counts.unsupported.saturating_add(1);
        self.unsupported.push(entry);
    }
}

impl PortabilityCounts {
    /// Every non-air palette entry counted.
    #[must_use]
    pub fn total(self) -> u32 {
        self.portable
            .saturating_add(self.degraded)
            .saturating_add(self.unsupported)
    }
}

/// Java's report: as the base edition, every entry its tables declare is portable.
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

/// Bedrock's report, each entry its tables declare classified by [`translate_states`].
///
/// # Errors
///
/// [`InvalidPalette`] naming every entry whose blockstate a validated pack could not produce.
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
        // No wildcard arm or `..`: a new refusal or field must be classified here.
        match translate_states(entry) {
            Ok(StateTranslation {
                states: _,
                degraded,
            }) if degraded.is_empty() => {
                report.count_portable();
            }
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

/// The entry for a block the tables refute, with the edition-wide aliases and nearest id
/// `E_UNKNOWN_ID` would offer.
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

/// Every non-air palette entry across the IR's structures, walkways included.
fn non_air_entries(ir: &BlockArrayIr) -> impl Iterator<Item = &BlockState> {
    ir.structures
        .values()
        .flat_map(|ba| ba.palette.entries.iter())
        .filter(|entry| !is_air(entry))
}

/// Whether the tables prove no version has this block; a pack without tables proves nothing.
fn absent_from_edition(blocks: &BlocksIndex, state: &BlockState) -> bool {
    blocks.declared_by_some_version(&state.id) == Some(false)
}

fn is_air(state: &BlockState) -> bool {
    state.id == BlockState::AIR_ID
}
