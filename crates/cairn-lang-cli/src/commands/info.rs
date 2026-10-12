//! `cairn info`: a source's version axes, with one portability and buildable-targets row per
//! requested edition.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::resolve::{
    BuildableTargets, DegradedEntry, EditionReport, UnsupportedEntry, UnsupportedReason,
    VersionAxes, compute_axes, declared_version_floors, resolve,
};
use cairn_lang_core::suggest::{candidate_list, did_you_mean};
use cairn_lang_core::{Diagnostic, Edition, Module, Severity, lower};
use cairn_lang_formats::bedrock_state::degradation_detail;
use cairn_lang_formats::portability::{
    InvalidPalette, PortabilityReport, portability_for_bedrock, portability_for_java,
};
use cairn_lang_formats::registry::builtin_pack;

use crate::cli::InfoFormat;
use crate::floors::{
    buildable_refusal, report_floors_left_out_of_the_neutral_row, report_version_notes,
    weigh_versions,
};
use crate::pipeline::{
    RequestedEditions, build_diagnostics, dropped_scopes, load_module, requested_editions,
};
use crate::report::{
    FailureReport, Reporting, print_json, report_diagnostic, report_diagnostics,
    report_failure_document,
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

/// Each requested edition's report row, from a dry-run lower per edition and per version.
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
    module: &Module,
    ir: &cairn_lang_core::intent::IntentModule,
    editions: &RequestedEditions,
    already_reported: &[Diagnostic],
) -> Result<Vec<EditionReport>, Vec<Diagnostic>> {
    let already: std::collections::HashSet<(&str, usize, usize)> = already_reported
        .iter()
        .map(|d| (d.code.as_str(), d.span.start, d.span.end))
        .collect();
    let mut rows: Vec<EditionReport> = Vec::with_capacity(editions.as_slice().len());
    let mut edition_specific_error = false;
    let mut refused: Vec<Diagnostic> = Vec::new();

    for &edition in editions.as_slice() {
        let resolution = resolve(ir, Some(edition));
        let pack = builtin_pack(edition);
        // Unpinned, as above: the whole range answers whether the edition has a block at all.
        let block_ir = lower_to_block_array(ir, &resolution, Some(&pack.view(None)));

        let only_here: Vec<_> = resolution
            .diagnostics
            .iter()
            .chain(block_ir.diagnostics.iter())
            .filter(|d| !already.contains(&(d.code.as_str(), d.span.start, d.span.end)))
            .cloned()
            .collect();
        edition_specific_error |= only_here.iter().any(|d| d.severity() == Severity::Error);
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

        let portability = match edition {
            Edition::Java => Ok(portability_for_java(&block_ir, &pack.blocks, &pack.aliases)),
            Edition::Bedrock => portability_for_bedrock(&block_ir, &pack.blocks, &pack.aliases),
        };
        // A refused palette does not end the walk: the scope and version notes below still print.
        let portability = portability_figure(edition, portability);
        edition_specific_error |= portability.is_none();
        if let Some(portability) = &portability {
            for note in degraded_notes(edition, portability.degraded()) {
                eprintln!("{note}");
            }
            for note in unsupported_notes(edition, portability.unsupported()) {
                eprintln!("{note}");
            }
        }
        let dropped = dropped_scopes(&resolution, &block_ir);
        if !dropped.is_empty() {
            eprintln!(
                "note: no {} target can build this source: {} produced no voxels, and a partial \
                 build is not certified",
                edition.as_str(),
                dropped
                    .iter()
                    .map(|scope| format!("`{scope}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }

        let floors = declared_version_floors(module, edition);
        let considered = supported_versions(pack);
        let verdicts = weigh_versions(ir, &resolution, pack, &floors, &considered, &dropped);
        for (version, refusals) in &verdicts.refused {
            eprintln!("note: {} {version} refuses this source", edition.as_str());
            report_diagnostics(file, source, lines, refusals);
        }
        report_version_notes(file, source, lines, edition, &verdicts, &considered);

        if let Some(portability) = portability {
            let refusal = buildable_refusal(source, lines, &verdicts, &considered, &dropped);
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
    if edition_specific_error {
        return Err(refused);
    }
    Ok(rows)
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
        let detail: Vec<String> = entry.dropped.iter().map(degradation_detail).collect();
        format!(
            "  note: `{}[{}]` — {}",
            entry.id,
            entry.states,
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
        "  note: a validated pack cannot produce these, so each is either a state literal on a \
         stair in the source — a `facing` or `half` value outside the Java domain, or a key \
         other than `facing` / `half` / `shape`, which nothing checks against the target yet — \
         or a leak in the pack or this compiler. The figure is withheld rather than counting a \
         validation gap as ordinary portability"
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
        // The backend, not the edition, lacks these states; the message must not blame the edition.
        UnsupportedReason::StatesUnmapped { states, mapped } => format!(
            "the edition has the block; this compiler maps states for {mapped} so far, so \
             `{states}` has no form here yet — bind the slot to a property-free material, or \
             build for the other edition"
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
