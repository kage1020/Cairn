//! What `cairn info` weighs: the edition-neutral gate, then for each requested edition a dry-run
//! lower with its circuits laid, its portability figure, and the verdict of each version.

use std::collections::HashSet;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{
    EditionReport, UnsupportedReason, declared_version_floors, resolve,
};
use cairn_lang_core::{Diagnostic, Edition, Module, Resolution, Severity};
use cairn_lang_redstone::Diagnostic as CircuitDiagnostic;

use super::circuits::{lay_circuits, raised_by_each, same_place};
use super::finding::{Finding, Found};
use super::floors::{VersionVerdicts, buildable_refusal, weigh_versions};
use super::gate::{RequestedEditions, build_diagnostics};
use super::run_level::palette_entry_name;
use super::scopes::dropped_scopes;
use crate::portability::{
    InvalidPalette, PortabilityReport, portability_for_bedrock, portability_for_java,
};
use crate::registry::builtin_pack;
use crate::target::supported_versions;

/// The findings of the edition-neutral gate: [`check()`](cairn_lang_core::check())'s and the
/// unpinned lowering's, with `@intended_targets` weighed in the requested editions.
#[must_use]
pub fn info_gate(module: &Module, ir: &IntentModule, asked: &RequestedEditions) -> Vec<Diagnostic> {
    // Edition-neutral; `edition_runs` runs the strict per-edition pass.
    let resolution = resolve(ir, None);
    // Unpinned: `info` spans every target, and pinning one would refuse ids fine on another.
    let mut block_ir = lower_to_block_array(
        ir,
        &resolution,
        Some(&builtin_pack(Edition::Java).view(None)),
    );
    // Only the requested editions: a Java-only finding must not refuse `--editions bedrock`.
    build_diagnostics(
        module,
        ir,
        None,
        asked.as_slice(),
        std::mem::take(&mut block_ir.diagnostics),
    )
}

/// Every requested edition's walk, after a gate that raised no error.
#[derive(Debug)]
pub struct EditionRuns {
    /// The circuit findings every edition raises at the same place, reported once.
    pub shared: Vec<CircuitDiagnostic>,
    /// One walk per requested edition, in the order they were asked for.
    pub editions: Vec<EditionRun>,
}

impl EditionRuns {
    /// Whether the walk refuses the run: an error among its findings, or a refused palette.
    #[must_use]
    pub fn refuses(&self) -> bool {
        self.shared.iter().any(|d| d.severity() == Severity::Error)
            || self.editions.iter().any(|run| {
                run.portability.is_err()
                    || run.found.iter().any(|d| d.severity() == Severity::Error)
            })
    }
}

/// One edition's dry-run lower and what it says about each version.
#[derive(Debug)]
pub struct EditionRun {
    /// The edition walked.
    pub edition: Edition,
    /// What its lowering finds beyond the gate's findings, then its circuits' findings beyond
    /// [`EditionRuns::shared`].
    pub found: Vec<Found>,
    /// How portable its palette is, or why the figure is withheld.
    pub portability: Result<PortabilityReport, InvalidPalette>,
    /// The scopes that produced no voxels, which refuse every version.
    pub dropped: Vec<String>,
    /// The palette entries whose states no version's state map has a form for, which refuse every
    /// version too.
    pub states_unmapped: Vec<String>,
    /// Every version the edition's pack can build for.
    pub considered: Vec<String>,
    /// How each of those versions answered.
    pub verdicts: VersionVerdicts,
}

impl EditionRun {
    /// This edition's `edition portability` and `buildable targets` row, or `None` when its
    /// palette was refused.
    #[must_use]
    pub fn report(self, source: &str, lines: &LineStarts) -> Option<EditionReport> {
        let portability = self.portability.ok()?;
        let refusal = buildable_refusal(
            source,
            lines,
            &self.verdicts,
            &self.considered,
            &self.dropped,
            &self.states_unmapped,
        );
        let portable = portability.counts().portable;
        let entries = portability.into_entries();
        Some(EditionReport {
            edition: self.edition,
            portable,
            unsupported_entries: entries.unsupported,
            degraded_entries: entries.degraded,
            buildable: self.verdicts.buildable,
            considered: self.considered,
            refusal,
        })
    }
}

/// Walk each requested edition, after a gate whose findings are `gate`.
#[must_use]
pub fn edition_runs(
    module: &Module,
    ir: &IntentModule,
    editions: &RequestedEditions,
    gate: &[Diagnostic],
) -> EditionRuns {
    let already: HashSet<(&str, usize, usize)> = gate
        .iter()
        .map(|d| (d.code.as_str(), d.span.start, d.span.end))
        .collect();
    let lowered: Vec<EditionLowering> = editions
        .as_slice()
        .iter()
        .map(|&edition| lowered_for(ir, edition, &already))
        .collect();
    let circuits: Vec<Vec<CircuitDiagnostic>> = lowered
        .iter()
        .map(|lowering| {
            lowering
                .found
                .iter()
                .filter_map(|d| match d {
                    Found::Circuit(d) => Some(d.clone()),
                    Found::Core(_) => None,
                })
                .collect()
        })
        .collect();
    let shared = raised_by_each(&circuits);
    let editions = lowered
        .into_iter()
        .map(
            |EditionLowering {
                 edition,
                 resolution,
                 block_ir,
                 found,
             }| {
                let pack = builtin_pack(edition);
                let found: Vec<Found> = found
                    .into_iter()
                    .filter(|d| {
                        !matches!(d, Found::Circuit(d) if shared.iter().any(|s| same_place(s, d)))
                    })
                    .collect();
                let portability = edition_portability(edition, &block_ir);
                let dropped = dropped_scopes(&resolution, &block_ir);
                // A pinned compile refuses these at every version, with `E_STATE_UNMAPPED`.
                let states_unmapped: Vec<String> = portability
                    .as_ref()
                    .into_iter()
                    .flat_map(PortabilityReport::unsupported)
                    .filter_map(|entry| match &entry.reason {
                        UnsupportedReason::StatesUnmapped { states, .. } => {
                            Some(palette_entry_name(&entry.id, states))
                        }
                        UnsupportedReason::AbsentFromEdition { .. }
                        | UnsupportedReason::NoCounterpart { .. } => None,
                    })
                    .collect();
                let floors = declared_version_floors(module, edition);
                let considered = supported_versions(pack);
                let verdicts = weigh_versions(
                    ir,
                    &resolution,
                    pack,
                    &floors,
                    &considered,
                    !dropped.is_empty() || !states_unmapped.is_empty(),
                );
                EditionRun {
                    edition,
                    found,
                    portability,
                    dropped,
                    states_unmapped,
                    considered,
                    verdicts,
                }
            },
        )
        .collect();
    EditionRuns { shared, editions }
}

/// A dry-run lower for one edition, its circuits laid.
struct EditionLowering {
    edition: Edition,
    resolution: Resolution,
    block_ir: BlockArrayIr,
    /// What the lowering finds beyond the gate's findings, then the circuits' findings.
    found: Vec<Found>,
}

/// `ir` lowered for `edition` alone, its circuits laid, with what that finds beyond `already`: the
/// lowering's findings, then the circuits' when none of those is an error.
fn lowered_for(
    ir: &IntentModule,
    edition: Edition,
    already: &HashSet<(&str, usize, usize)>,
) -> EditionLowering {
    let resolution = resolve(ir, Some(edition));
    // Unpinned, as the gate: the whole range answers whether the edition has a block at all.
    let registry = builtin_pack(edition).view(None);
    let mut block_ir = lower_to_block_array(ir, &resolution, Some(&registry));
    let mut only_here: Vec<Found> = resolution
        .diagnostics
        .iter()
        .chain(block_ir.diagnostics.iter())
        .filter(|d| !already.contains(&(d.code.as_str(), d.span.start, d.span.end)))
        .cloned()
        .map(Found::Core)
        .collect();
    // A member that failed to lower is not there for the circuit to wire.
    if !only_here.iter().any(|d| d.severity() == Severity::Error) {
        only_here.extend(
            lay_circuits(ir, &mut block_ir, edition, &registry)
                .into_iter()
                .map(Found::Circuit),
        );
    }
    EditionLowering {
        edition,
        resolution,
        block_ir,
        found: only_here,
    }
}

/// How portable `block_ir`'s palette is on `edition`'s built-in pack.
fn edition_portability(
    edition: Edition,
    block_ir: &BlockArrayIr,
) -> Result<PortabilityReport, InvalidPalette> {
    let pack = builtin_pack(edition);
    match edition {
        Edition::Java => Ok(portability_for_java(
            block_ir,
            pack.blocks(),
            pack.aliases(),
            pack.absent(),
        )),
        Edition::Bedrock => portability_for_bedrock(
            block_ir,
            pack.blocks(),
            pack.aliases(),
            pack.absent(),
            pack.states(),
        ),
    }
}
