//! `cairn info`: a source's version axes, with one portability and buildable-targets row per
//! requested edition.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::resolve::{
    BuildableTargets, DegradedEntry, EditionReport, UnsupportedEntry, UnsupportedReason,
    VersionAxes, compute_axes,
};
use cairn_lang_core::suggest::{candidate_list, did_you_mean};
use cairn_lang_core::{Edition, Severity, lower};
use cairn_lang_formats::build::finding::{Finding, Found};
use cairn_lang_formats::build::gate::requested_editions;
use cairn_lang_formats::build::info::{EditionRun, EditionRuns, edition_runs, info_gate};
use cairn_lang_formats::build::run_level::{invalid_palette, palette_entry_name};

use crate::cli::InfoFormat;
use crate::floors::{report_floors_left_out_of_the_neutral_row, report_version_notes};
use crate::pipeline::load_module;
use crate::report::{
    FailureReport, Reporting, print_json, report_diagnostic, report_diagnostics,
    report_failure_document, report_run_level,
};

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
    let combined = info_gate(&module, &ir, &asked);

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
    let runs = edition_runs(&module, &ir, &asked, &combined);
    let rows = match edition_rows(reporting, runs) {
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

/// Print the edition-wide causes of an empty `buildable`, each as the `note:` naming it: the
/// scopes that produced no voxels, and the palette entries the edition's writer cannot carry.
fn report_edition_wide_refusals(run: &EditionRun) {
    let edition = run.edition.as_str();
    if !run.dropped.is_empty() {
        eprintln!(
            "note: no {edition} target can build this source: {} produced no voxels, and a \
             partial build is not certified",
            backticked(&run.dropped),
        );
    }
    if !run.states_unmapped.is_empty() {
        eprintln!(
            "note: no {edition} target can build this source: its writer cannot carry the states \
             of {}",
            backticked(&run.states_unmapped),
        );
    }
}

fn backticked(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Report each requested edition's walk and turn it into the edition's report row.
///
/// `Err` holds the errors the JSON failure document owes; it is empty under
/// [`FailureReport::Text`], or when only a palette refused the run.
fn edition_rows(
    Reporting {
        file,
        source,
        lines,
        report,
    }: Reporting<'_>,
    runs: EditionRuns,
) -> Result<Vec<EditionReport>, Vec<Found>> {
    let error_found = runs.refuses();
    let mut refused: Vec<Found> = Vec::new();
    // The circuit findings every edition raises, reported once outside any edition's header.
    for d in &runs.shared {
        if d.severity() == Severity::Error && report == FailureReport::Json {
            refused.push(Found::Circuit(d.clone()));
        } else {
            report_diagnostic(file, source, lines, d);
        }
    }

    let mut rows: Vec<EditionReport> = Vec::with_capacity(runs.editions.len());
    for run in runs.editions {
        let edition = run.edition;
        let mut edition_header_printed = false;
        for d in &run.found {
            if d.severity() == Severity::Error && report == FailureReport::Json {
                refused.push(d.clone());
                continue;
            }
            if !edition_header_printed {
                eprintln!("note: reported for --editions {}", edition.as_str());
                edition_header_printed = true;
            }
            report_diagnostic(file, source, lines, d);
        }

        // A refused palette does not end the walk: the scope and version notes below still print.
        match &run.portability {
            Ok(portability) => {
                for note in degraded_notes(edition, portability.degraded()) {
                    eprintln!("{note}");
                }
                for note in unsupported_notes(edition, portability.unsupported()) {
                    eprintln!("{note}");
                }
            }
            Err(invalid) => {
                report_run_level(file, source, lines, &invalid_palette(edition, invalid));
            }
        }
        report_edition_wide_refusals(&run);

        for (version, refusals) in &run.verdicts.refused {
            eprintln!("note: {} {version} refuses this source", edition.as_str());
            report_diagnostics(file, source, lines, refusals);
        }
        report_version_notes(file, source, lines, edition, &run.verdicts, &run.considered);

        rows.extend(run.report(source, lines));
    }

    // Not `refused.is_empty()`: a refused palette refuses the run without a finding.
    if error_found {
        return Err(refused);
    }
    Ok(rows)
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
