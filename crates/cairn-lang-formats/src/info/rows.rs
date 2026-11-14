//! One report row per requested edition: a dry-run lower with its circuits laid, its portability
//! figure, and one pinned lowering per version.

use std::collections::HashSet;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{
    DegradedEntry, EditionReport, UnsupportedEntry, UnsupportedReason, declared_version_floors,
    resolve,
};
use cairn_lang_core::suggest::{candidate_list, did_you_mean};
use cairn_lang_core::{Diagnostic, Edition, Module, Resolution, Severity};
use cairn_lang_redstone::Diagnostic as CircuitDiagnostic;

use super::Reporter;
use super::floors::{buildable_refusal, report_version_notes, weigh_versions};
use super::inferred::report_inferred_range;
use super::respelled::{report_respelled_notes, respelled_tokens};
use crate::build::circuits::{lay_circuits, raised_by_each, same_place};
use crate::build::finding::{Finding, Found};
use crate::build::run_level::palette_entry_name;
use crate::build::scopes::dropped_scopes;
use crate::portability::{
    InvalidPalette, PortabilityReport, portability_for_bedrock, portability_for_java,
};
use crate::registry::builtin_pack;
use crate::target::supported_versions;

/// Each requested edition's report row, from a dry-run lower per edition, its circuits laid, and
/// one per version.
///
/// `Err` holds the errors the JSON failure document owes; it is empty unless
/// `errors_in_document`, or when only a palette refused the run.
pub(super) fn edition_rows(
    out: &mut Reporter<'_>,
    module: &Module,
    ir: &IntentModule,
    editions: &[Edition],
    already_reported: &[Diagnostic],
    errors_in_document: bool,
) -> Result<Vec<EditionReport>, Vec<Found>> {
    let already: HashSet<(&str, usize, usize)> = already_reported
        .iter()
        .map(|d| (d.code.as_str(), d.span.start, d.span.end))
        .collect();
    let mut rows: Vec<EditionReport> = Vec::with_capacity(editions.len());
    let mut error_found = false;
    let mut refused: Vec<Found> = Vec::new();

    let lowered: Vec<_> = editions
        .iter()
        .map(|&edition| lowered_for(ir, edition, &already))
        .collect();
    let shared = report_shared_circuits(out, &lowered, &mut refused, errors_in_document);
    error_found |= shared.iter().any(|d| d.severity() == Severity::Error);

    for EditionLowering {
        edition,
        resolution,
        block_ir,
        found,
    } in lowered
    {
        let pack = builtin_pack(edition);
        let only_here: Vec<Found> = found
            .into_iter()
            .filter(|d| !matches!(d, Found::Circuit(d) if shared.iter().any(|s| same_place(s, d))))
            .collect();
        error_found |= only_here.iter().any(|d| d.severity() == Severity::Error);
        report_edition_findings(out, edition, only_here, &mut refused, errors_in_document);

        // A refused palette does not end the walk: the scope and version notes below still print.
        let portability = portability_figure(out, edition, edition_portability(edition, &block_ir));
        error_found |= portability.is_none();
        if let Some(portability) = &portability {
            for note in degraded_notes(edition, portability.degraded()) {
                out.line(&note);
            }
            for note in unsupported_notes(edition, portability.unsupported()) {
                out.line(&note);
            }
        }
        let (dropped, states_unmapped) =
            edition_wide_refusals(out, edition, &resolution, &block_ir, portability.as_ref());

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
        report_version_notes(out, edition, &verdicts, &considered);
        let inferred_range =
            report_inferred_range(out, ir, &resolution, edition, &considered, &floors);
        let respelled = respelled_tokens(
            &resolution,
            pack,
            &floors,
            &considered,
            out.source,
            out.lines,
        );
        report_respelled_notes(out, edition, &respelled);

        if let Some(portability) = portability {
            let refusal = buildable_refusal(
                out.source,
                out.lines,
                &verdicts,
                &considered,
                &dropped,
                &states_unmapped,
            );
            let portable = portability.counts().portable;
            let entries = portability.into_entries();
            rows.push(EditionReport {
                edition,
                portable,
                unsupported_entries: entries.unsupported,
                degraded_entries: entries.degraded,
                buildable: verdicts.buildable,
                considered,
                refusal,
                inferred_range,
                respelled,
            });
        }
    }

    // Not `refused.is_empty()`: a refused palette refuses the run without a finding.
    if error_found {
        return Err(refused);
    }
    Ok(rows)
}

/// Report what one edition's lowering alone found under a header naming the edition, holding
/// back each error for the failure document when `errors_in_document`.
fn report_edition_findings(
    out: &mut Reporter<'_>,
    edition: Edition,
    only_here: Vec<Found>,
    refused: &mut Vec<Found>,
    errors_in_document: bool,
) {
    let mut edition_header_printed = false;
    for d in only_here {
        if d.severity() == Severity::Error && errors_in_document {
            refused.push(d);
            continue;
        }
        if !edition_header_printed {
            out.line(&format!(
                "note: reported for --editions {}",
                edition.as_str()
            ));
            edition_header_printed = true;
        }
        out.finding(&d);
    }
}

/// The circuit findings every edition of `lowered` raises at the same place, each reported once
/// outside any edition's header.
fn report_shared_circuits(
    out: &mut Reporter<'_>,
    lowered: &[EditionLowering],
    refused: &mut Vec<Found>,
    errors_in_document: bool,
) -> Vec<CircuitDiagnostic> {
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
    for d in &shared {
        let d = Found::Circuit(d.clone());
        if d.severity() == Severity::Error && errors_in_document {
            refused.push(d);
        } else {
            out.finding(&d);
        }
    }
    shared
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

/// The edition-wide causes of an empty `buildable`, each sent as the `note:` naming it: the
/// scopes that produced no voxels, and the palette entries the edition's writer cannot carry.
fn edition_wide_refusals(
    out: &mut Reporter<'_>,
    edition: Edition,
    resolution: &Resolution,
    block_ir: &BlockArrayIr,
    portability: Option<&PortabilityReport>,
) -> (Vec<String>, Vec<String>) {
    let dropped = dropped_scopes(resolution, block_ir);
    if !dropped.is_empty() {
        out.line(&format!(
            "note: no {} target can build this source: {} produced no voxels, and a partial \
             build is not certified",
            edition.as_str(),
            backticked(&dropped),
        ));
    }
    // A pinned compile refuses these at every version, with `E_STATE_UNMAPPED` at lowering.
    let states_unmapped: Vec<String> = portability
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
    if !states_unmapped.is_empty() {
        out.line(&format!(
            "note: no {} target can build this source: its writer cannot carry the states of {}",
            edition.as_str(),
            backticked(&states_unmapped),
        ));
    }
    (dropped, states_unmapped)
}

fn backticked(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
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

/// One edition's portability figure, or `None` after reporting a palette whose blockstates a
/// registry pack is expected to refuse.
fn portability_figure(
    out: &mut Reporter<'_>,
    edition: Edition,
    portability: Result<PortabilityReport, InvalidPalette>,
) -> Option<PortabilityReport> {
    match portability {
        Ok(portability) => Some(portability),
        Err(invalid) => {
            for line in invalid_palette_report(edition, &invalid) {
                out.line(&line);
            }
            None
        }
    }
}

/// The lines listing the palette entries one edition builds with loss.
fn degraded_notes(edition: Edition, entries: &[DegradedEntry]) -> Vec<String> {
    if entries.is_empty() {
        return Vec::new();
    }
    let mut notes = vec![format!(
        "note: what `degraded: {}` counts on {}:",
        entries.len(),
        edition.as_str(),
    )];
    notes.extend(entries.iter().map(|entry| {
        let detail: Vec<String> = entry
            .dropped
            .iter()
            .map(|dropped| format!("{} ({})", dropped.reason, dropped.mc_versions.join(", ")))
            .collect();
        format!(
            "  note: `{}` — {}",
            palette_entry_name(&entry.id, &entry.states),
            detail.join("; "),
        )
    }));
    notes
}

/// The lines listing the palette entries one edition has no form for.
fn unsupported_notes(edition: Edition, entries: &[UnsupportedEntry]) -> Vec<String> {
    if entries.is_empty() {
        return Vec::new();
    }
    let mut notes = vec![format!(
        "note: what `unsupported: {}` counts on {}:",
        entries.len(),
        edition.as_str(),
    )];
    notes.extend(entries.iter().map(|entry| {
        format!(
            "  note: `{}` — {}",
            entry.id,
            unsupported_reason(&entry.reason)
        )
    }));
    notes
}

/// The lines refusing one edition's portability row.
fn invalid_palette_report(edition: Edition, invalid: &InvalidPalette) -> Vec<String> {
    let mut lines = vec![format!(
        "error: the {} palette carries blockstates a registry pack is expected to refuse, so \
         this edition gets no portability figure:",
        edition.as_str(),
    )];
    lines.extend(
        invalid
            .leaks()
            .iter()
            .map(|leak| format!("  error: {leak}")),
    );
    lines.push(
        "  note: a validated pack cannot produce these, so each is either a state literal in the \
         source — a property the Java block does not have, or a value outside its domain, which \
         `E_STATE_DOMAIN` refuses at a pinned target but these figures' range-wide lowering does \
         not check — or a leak in the pack or this compiler. The figure is withheld rather than \
         counting a validation gap as ordinary portability"
            .to_owned(),
    );
    lines
}

/// One entry's reason, as the clause that follows its id.
fn unsupported_reason(reason: &UnsupportedReason) -> String {
    match reason {
        UnsupportedReason::AbsentFromEdition {
            suggestion,
            aliases,
        } => {
            let absent = "no supported version of this edition declares the block";
            // Aliases first: the pack's own respelling beats any edit-distance guess.
            match (aliases.as_slice(), suggestion) {
                ([], None) => absent.to_owned(),
                ([], Some(suggestion)) => format!("{absent}; {}", did_you_mean(suggestion)),
                (spellings, _) => format!(
                    "no supported version of this edition declares that id; it spells the \
                     block {}",
                    candidate_list(spellings),
                ),
            }
        }
        UnsupportedReason::NoCounterpart { detail } => format!(
            "only the other edition has the block: {detail} — bind the slot to a block this \
             edition has, or build for the other edition"
        ),
        UnsupportedReason::StatesUnmapped { states, detail } => format!(
            "the edition has the block, but no version's state map has a form for `{states}`: \
             {detail} — bind the slot to another block, or build for the other edition"
        ),
    }
}

#[cfg(test)]
mod tests;
