//! `cairn info`: a source's version axes, with one portability and buildable-targets row per
//! requested edition.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::resolve::{
    BuildableTargets, DegradedEntry, EditionReport, UnsupportedEntry, UnsupportedReason,
    VersionAxes, compute_axes, declared_version_floors, resolve,
};
use cairn_lang_core::suggest::{candidate_list, did_you_mean};
use cairn_lang_core::{Diagnostic, Edition, Module, Resolution, Severity, lower};
use cairn_lang_formats::portability::{
    InvalidPalette, PortabilityReport, portability_for_bedrock, portability_for_java,
};
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_redstone::Diagnostic as CircuitDiagnostic;

use crate::cli::InfoFormat;
use crate::floors::{
    buildable_refusal, report_floors_left_out_of_the_neutral_row, report_version_notes,
    weigh_versions,
};
use crate::pipeline::{
    RequestedEditions, build_diagnostics, dropped_scopes, lay_circuits, load_module,
    raised_by_each, requested_editions, same_place,
};
use crate::report::{
    FailureReport, Finding, Found, Reporting, palette_entry_name, print_json, report_diagnostic,
    report_diagnostics, report_failure_document,
};
use crate::target::supported_versions;

pub(crate) fn run_info(file: &Path, editions: &[String], format: InfoFormat) -> ExitCode {
    let asked = match requested_editions(editions) {
        Ok(asked) => asked,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
    };
    let (source, module) = match load_module(file, format.failure_report()) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let ir = lower(&module);

    // The gate is edition-neutral; `edition_rows` runs the strict per-edition pass.
    let resolution = resolve(&ir, None);
    // Unpinned: `info` spans every target, and pinning one would refuse ids fine on another.
    let mut block_ir = lower_to_block_array(
        &ir,
        &resolution,
        Some(&builtin_pack(Edition::Java).view(None)),
    );
    // Only the requested editions: a Java-only finding must not refuse `--editions bedrock`.
    let combined = build_diagnostics(
        &module,
        &ir,
        None,
        asked.as_slice(),
        std::mem::take(&mut block_ir.diagnostics),
    );

    let lines = LineStarts::new(&source);
    let has_error = combined.iter().any(|d| d.severity() == Severity::Error);
    if has_error {
        return report_failure_document(file, &source, format.failure_report(), &combined);
    }
    // Warnings go to stderr in both formats.
    for d in &combined {
        report_diagnostic(file, &source, &lines, d);
    }

    report_floors_left_out_of_the_neutral_row(file, &source, &lines, &module);
    let reporting = Reporting {
        file,
        source: &source,
        lines: &lines,
        report: format.failure_report(),
    };
    let rows = match edition_rows(reporting, &module, &ir, &asked, &combined) {
        Ok(rows) => rows,
        // Only errors, unlike the gate's document: `spec/lint` "Machine-readable payload".
        Err(refused) => {
            return report_failure_document(file, &source, reporting.report, &refused);
        }
    };

    let axes = compute_axes(&module, rows);

    match format {
        InfoFormat::Text => {
            print_axes_report(&axes);
            ExitCode::SUCCESS
        }
        InfoFormat::Json => match print_json("version axes", &axes) {
            Ok(()) => ExitCode::SUCCESS,
            Err(code) => code,
        },
    }
}

/// The edition-wide causes of an empty `buildable`, each printed as the `note:` naming it: the
/// scopes that produced no voxels, and the palette entries the edition's writer cannot carry.
fn edition_wide_refusals(
    edition: Edition,
    resolution: &Resolution,
    block_ir: &BlockArrayIr,
    portability: Option<&PortabilityReport>,
) -> (Vec<String>, Vec<String>) {
    let dropped = dropped_scopes(resolution, block_ir);
    if !dropped.is_empty() {
        eprintln!(
            "note: no {} target can build this source: {} produced no voxels, and a partial \
             build is not certified",
            edition.as_str(),
            backticked(&dropped),
        );
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
        eprintln!(
            "note: no {} target can build this source: its writer cannot carry the states of {}",
            edition.as_str(),
            backticked(&states_unmapped),
        );
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

/// Each requested edition's report row, from a dry-run lower per edition, its circuits laid, and
/// one per version.
///
/// `Err` holds the errors the JSON failure document owes; it is empty under
/// [`FailureReport::Text`], or when only a palette refused the run.
fn edition_rows(
    reporting: Reporting<'_>,
    module: &Module,
    ir: &cairn_lang_core::intent::IntentModule,
    editions: &RequestedEditions,
    already_reported: &[Diagnostic],
) -> Result<Vec<EditionReport>, Vec<Found>> {
    let already: std::collections::HashSet<(&str, usize, usize)> = already_reported
        .iter()
        .map(|d| (d.code.as_str(), d.span.start, d.span.end))
        .collect();
    let mut rows: Vec<EditionReport> = Vec::with_capacity(editions.as_slice().len());
    let mut error_found = false;
    let mut refused: Vec<Found> = Vec::new();

    let lowered: Vec<_> = editions
        .as_slice()
        .iter()
        .map(|&edition| lowered_for(ir, edition, &already))
        .collect();
    let shared = report_shared_circuits(reporting, &lowered, &mut refused);
    error_found |= shared.iter().any(|d| d.severity() == Severity::Error);
    let Reporting {
        file,
        source,
        lines,
        report,
    } = reporting;

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
        let mut edition_header_printed = false;
        for d in only_here {
            if d.severity() == Severity::Error && report == FailureReport::Json {
                refused.push(d);
                continue;
            }
            if !edition_header_printed {
                eprintln!("note: reported for --editions {}", edition.as_str());
                edition_header_printed = true;
            }
            report_diagnostic(file, source, lines, &d);
        }

        // A refused palette does not end the walk: the scope and version notes below still print.
        let portability = portability_figure(edition, edition_portability(edition, &block_ir));
        error_found |= portability.is_none();
        if let Some(portability) = &portability {
            for note in degraded_notes(edition, portability.degraded()) {
                eprintln!("{note}");
            }
            for note in unsupported_notes(edition, portability.unsupported()) {
                eprintln!("{note}");
            }
        }
        let (dropped, states_unmapped) =
            edition_wide_refusals(edition, &resolution, &block_ir, portability.as_ref());

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
        for (version, refusals) in &verdicts.refused {
            eprintln!("note: {} {version} refuses this source", edition.as_str());
            report_diagnostics(file, source, lines, refusals);
        }
        report_version_notes(file, source, lines, edition, &verdicts, &considered);

        if let Some(portability) = portability {
            let refusal = buildable_refusal(
                source,
                lines,
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
            });
        }
    }

    // Not `refused.is_empty()`: a refused palette refuses the run without a finding.
    if error_found {
        return Err(refused);
    }
    Ok(rows)
}

/// The circuit findings every edition of `lowered` raises at the same place, each reported once, as
/// the gate's findings are, rather than under each edition's header.
fn report_shared_circuits(
    Reporting {
        file,
        source,
        lines,
        report,
    }: Reporting<'_>,
    lowered: &[EditionLowering],
    refused: &mut Vec<Found>,
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
        if d.severity() == Severity::Error && report == FailureReport::Json {
            refused.push(Found::Circuit(d.clone()));
        } else {
            report_diagnostic(file, source, lines, d);
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
    ir: &cairn_lang_core::intent::IntentModule,
    edition: Edition,
    already: &std::collections::HashSet<(&str, usize, usize)>,
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

/// One edition's portability figure, or `None` after reporting a palette whose blockstates a
/// registry pack is expected to refuse.
fn portability_figure(
    edition: Edition,
    portability: Result<PortabilityReport, InvalidPalette>,
) -> Option<PortabilityReport> {
    match portability {
        Ok(portability) => Some(portability),
        Err(invalid) => {
            for line in invalid_palette_report(edition, &invalid) {
                eprintln!("{line}");
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

fn joined_or(empty: &str, separator: &str, items: impl IntoIterator<Item = String>) -> String {
    let items: Vec<String> = items.into_iter().collect();
    if items.is_empty() {
        empty.to_owned()
    } else {
        items.join(separator)
    }
}

fn print_axes_report(axes: &VersionAxes) {
    // One range for every edition: `spec/versioning-editions` "The `registry compatibility` row".
    outln!(
        "registry compatibility:  {} .. {}",
        axes.registry_compat.min,
        axes.registry_compat.max,
    );

    let portability_line = joined_or(
        "(no editions requested)",
        "   ",
        axes.edition_portability.iter().map(|ep| {
            format!(
                "{}: portable: {}  degraded: {}  unsupported: {}",
                capitalise(ep.edition.as_str()),
                ep.portable,
                ep.degraded,
                ep.unsupported,
            )
        }),
    );
    outln!("edition portability:     {portability_line}");

    let buildable_line = joined_or(
        "(no editions requested)",
        "   ",
        axes.buildable_targets.iter().map(|bt| {
            format!(
                "{}: {}",
                capitalise(bt.edition.as_str()),
                buildable_text(bt)
            )
        }),
    );
    outln!("buildable targets:       {buildable_line}");

    let intended_line = joined_or(
        "(none declared)",
        ", ",
        axes.intended_targets.iter().cloned(),
    );
    outln!("intended targets:        {intended_line}");

    let semantic_line = joined_or(
        "(none)",
        ", ",
        axes.semantic_sensitive
            .iter()
            .map(|f| format!("{}({} @{})", f.member, f.reason, f.boundary_version)),
    );
    outln!("semantic-sensitive:      {semantic_line}");
}

/// One edition's buildable versions, with the refusing ones named after them.
fn buildable_text(targets: &BuildableTargets) -> String {
    let refused: Vec<&str> = targets
        .considered
        .iter()
        .filter(|version| !targets.buildable.contains(version))
        .map(String::as_str)
        .collect();
    let built = if targets.buildable.is_empty() {
        String::from("none")
    } else {
        targets.buildable.join(", ")
    };
    if refused.is_empty() {
        return built;
    }
    let tail = if let [only] = refused.as_slice() {
        format!("{only} refuses")
    } else if targets.buildable.is_empty() {
        format!("{} all refuse", refused.join(", "))
    } else {
        format!("{} refuse", refused.join(", "))
    };
    format!("{built} ({tail})")
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests;
