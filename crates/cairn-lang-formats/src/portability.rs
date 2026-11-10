//! Per-edition palette-entry portability for `cairn info`, counted over the lowered IR so each
//! figure matches what `cairn compile --edition` emits. An id no version of the edition declares
//! is unsupported, answered by the pack's `absent` reason when it is the other edition's block; on
//! Bedrock the rest go through the `.mcstructure` writer's state translation at
//! every version that declares them, and are asked of the edition: a state some version maps is
//! not unsupported, and a loss at some version is a degradation.

use cairn_lang_core::block_array::{BlockArrayIr, BlockState};
use cairn_lang_core::resolve::{DegradedEntry, DroppedIntent, UnsupportedEntry, UnsupportedReason};
use cairn_lang_core::suggest::nearest_namespaced_id;

use thiserror::Error;

use crate::bedrock_state::{BedrockStateError, translate_states};
use crate::registry::{AbsentIndex, AliasIndex, BlocksIndex, StateMapIndex};

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
    absent: &AbsentIndex,
) -> PortabilityReport {
    let mut report = PortabilityReport::default();
    for entry in non_air_entries(ir) {
        if absent_from_edition(blocks, entry) {
            report.push_unsupported(absent_entry(blocks, aliases, absent, entry));
            continue;
        }
        report.count_portable();
    }
    report
}

/// Bedrock's report, each entry its tables declare classified by [`translate_states`] at every
/// version that declares it; without `states`, no entry has a form.
///
/// # Errors
///
/// [`InvalidPalette`] naming every entry whose blockstate a validated pack could not produce.
pub fn portability_for_bedrock(
    ir: &BlockArrayIr,
    blocks: &BlocksIndex,
    aliases: &AliasIndex,
    absent: &AbsentIndex,
    states: Option<&StateMapIndex>,
) -> Result<PortabilityReport, InvalidPalette> {
    let mut report = PortabilityReport::default();
    let mut leaks = Vec::new();
    for entry in non_air_entries(ir) {
        if absent_from_edition(blocks, entry) {
            report.push_unsupported(absent_entry(blocks, aliases, absent, entry));
            continue;
        }
        let mut written = false;
        let mut dropped: Vec<DroppedIntent> = Vec::new();
        let mut unmapped: Option<String> = None;
        let mut leaked: Option<BedrockStateError> = None;
        for version in blocks.versions() {
            let declared = blocks
                .ids_for(version)
                .is_some_and(|ids| ids.binary_search(&entry.id).is_ok());
            let Some(map) = states
                .and_then(|states| states.at(version))
                .filter(|_| declared)
            else {
                continue;
            };
            // No wildcard arm: a new refusal must be classified here.
            match translate_states(&map, entry) {
                Ok(translated) => {
                    written = true;
                    for intent in translated.degraded {
                        merge_dropped(&mut dropped, intent);
                    }
                }
                Err(BedrockStateError::Unmapped { reason, .. }) => unmapped = Some(reason),
                Err(
                    err @ (BedrockStateError::UnknownProperty { .. }
                    | BedrockStateError::UnknownValue { .. }),
                ) => leaked = Some(err),
            }
        }
        match (written, leaked, unmapped) {
            // At any version: a validated pack that writes the entry elsewhere still let it leak.
            (_, Some(leak), _) => leaks.push(leak),
            (true, None, _) if dropped.is_empty() => report.count_portable(),
            (true, None, _) => report.push_degraded(DegradedEntry {
                id: entry.id.clone(),
                states: entry.properties_text(),
                dropped,
            }),
            (false, None, detail) => report.push_unsupported(UnsupportedEntry {
                id: entry.id.clone(),
                reason: UnsupportedReason::StatesUnmapped {
                    states: entry.properties_text(),
                    detail: detail.unwrap_or_else(|| {
                        "no version that declares the block carries a state map".to_owned()
                    }),
                },
            }),
        }
    }
    if leaks.is_empty() {
        Ok(report)
    } else {
        Err(InvalidPalette { leaks })
    }
}

/// Adds `intent`, lost at one version, to the list: its version joins the same loss at earlier
/// versions, whose reason the one Java block's `sensitivity` row already gave.
fn merge_dropped(dropped: &mut Vec<DroppedIntent>, intent: DroppedIntent) {
    match dropped
        .iter_mut()
        .find(|seen| seen.key == intent.key && seen.value == intent.value)
    {
        Some(seen) => seen.mc_versions.extend(intent.mc_versions),
        None => dropped.push(intent),
    }
}

/// The entry for a block the tables refute: the pack's reason when the block is the other
/// edition's, or else the edition-wide aliases and nearest id `E_UNKNOWN_ID` would offer.
fn absent_entry(
    blocks: &BlocksIndex,
    aliases: &AliasIndex,
    absent: &AbsentIndex,
    state: &BlockState,
) -> UnsupportedEntry {
    let reason = match absent.reason_for(&state.id) {
        Some(detail) => UnsupportedReason::NoCounterpart {
            detail: detail.to_owned(),
        },
        None => UnsupportedReason::AbsentFromEdition {
            suggestion: nearest_namespaced_id(&state.id, blocks.declared_ids()),
            aliases: aliases
                .spellings_of(&state.id)
                .iter()
                .filter(|spelling| blocks.declared_by_some_version(spelling) == Some(true))
                .cloned()
                .collect(),
        },
    };
    UnsupportedEntry {
        id: state.id.clone(),
        reason,
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
