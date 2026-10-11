//! `cairn info`: the version axes of a source, one portability and
//! buildable-targets row per requested edition.

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
    supported_versions,
};
use crate::report::{
    FailureReport, Reporting, print_json, report_diagnostic, report_diagnostics,
    report_failure_document,
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

    // Any error refuses the report before the per-edition dry runs, as in
    // `check`, `lower` and `compile`. This gate is edition-neutral:
    // `resolve(ir, None)` unions slot names across theme variants, and the
    // strict per-edition pass runs in `edition_rows`.
    let resolution = resolve(&ir, None);
    // `info` spans every target, so the view pins no version: guessing one
    // would refuse ids that are fine on the version the author builds for.
    let mut block_ir = lower_to_block_array(
        &ir,
        &resolution,
        Some(&builtin_pack(Edition::Java).view(None)),
    );
    // The requested editions, not both: a finding only Java's table reaches
    // must not refuse an `--editions bedrock` report.
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
    // Warnings go to stderr in both formats: folding them into the report
    // would change a document tools already read.
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
        // The same refusal as the gate above, one pass later. That gate's
        // document carries its warnings too, this one only errors, as
        // `spec/lint` "Machine-readable payload" states.
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

/// One dry-run lower per requested edition, plus one per supported
/// version of that edition, folded into that edition's report row.
///
/// Each edition is walked once, in `--editions` order, since a
/// [`RequestedEditions`] holds no repeat.
///
/// The per-edition pass is strict where the caller's neutral pass is
/// soft: a slot only one variant declares resolves there and not here, and
/// that finding is reported under the edition rather than folded into a
/// smaller `portable` count. A palette the pack refuses drops that
/// edition's row with a report of why; the other editions are still walked
/// before returning.
///
/// The version loop cannot be replaced by intersecting the range-wide
/// palette's id sets: with no target pinned every material takes its
/// default mapping, so a token the target respells is compared as the
/// wrong id. A version is buildable when it passes the source gates
/// [`run_compile`](crate::commands::compile::run_compile) applies: the
/// pinned lowering raises no error, the `@requires` floor is at or below
/// it, and every declared scope lowered. Each pinned lowering's findings
/// print under the version that raised them; the floor and the dropped
/// scopes get one line per edition.
///
/// `already_reported` is the edition-neutral stream the caller printed;
/// only diagnostics absent from it, keyed by code and span, are reported.
///
/// `Err` carries the findings the failure document owes. Under
/// [`FailureReport::Json`] errors are held rather than printed, because
/// the document is written once after every edition and a finding printed
/// on the way would precede it; under [`FailureReport::Text`] they print
/// under the note naming their edition and the list comes back empty.
/// Warnings print on stderr either way (`spec/lint` "Machine-readable
/// payload").
///
/// A refused palette adds nothing to the list, since no leak
/// [`invalid_palette_report`] names has a span: a run refused by the
/// palette alone returns `Err` of an empty list, which under JSON is the
/// `{"diagnostics": []}` the spec asks for.
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
        // Unpinned, as above. The portability fold still reads the pack's
        // tables: whether the edition has a block at all is a question the
        // whole range answers.
        let block_ir = lower_to_block_array(ir, &resolution, Some(&pack.view(None)));

        let only_here: Vec<_> = resolution
            .diagnostics
            .iter()
            .chain(block_ir.diagnostics.iter())
            .filter(|d| !already.contains(&(d.code.as_str(), d.span.start, d.span.end)))
            .cloned()
            .collect();
        edition_specific_error |= only_here.iter().any(|d| d.severity() == Severity::Error);
        // The header prints only when something is left for it to head.
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
        // A refused palette does not end the edition's walk: the dropped
        // scopes and version refusals below do not read the palette, and
        // stderr is the only place they can appear.
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

        // Per edition: an unscoped floor is in every edition's build, a
        // scoped one only in its own.
        let floors = declared_version_floors(module, edition);
        let considered = supported_versions(pack);
        let verdicts = weigh_versions(ir, &resolution, pack, &floors, &considered, &dropped);
        for (version, refusals) in &verdicts.refused {
            eprintln!("note: {} {version} refuses this source", edition.as_str());
            report_diagnostics(file, source, lines, refusals);
        }
        report_version_notes(file, source, lines, edition, &verdicts, &considered);

        // A refused palette has no counts, so no row, though the run exits
        // 1 and drops the rows either way.
        if let Some(portability) = portability {
            let refusal = buildable_refusal(source, lines, &verdicts, &considered, &dropped);
            // Only `portable`: `compute_axes` derives the other two counts
            // from the lists, so no figure can disagree with its list.
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

    // The flag, not `refused.is_empty()`: a refused palette refuses the run
    // without contributing a finding.
    if edition_specific_error {
        return Err(refused);
    }
    Ok(rows)
}

/// One edition's portability figure, or `None` where its palette carried
/// blockstates a registry pack is expected to refuse.
///
/// Counts over such a palette would read as ordinary portability, so the
/// edition contributes no row and the run exits 1. The refusal is reported
/// here as prose: no leak it names has a span, so it is a run-level
/// refusal (`spec/lint` "Machine-readable payload").
///
/// Takes the `Result` so a test can drive the `Err` arm: a `.crn` reaches
/// it only through a stair state literal, and the pack and compiler bugs it
/// also catches have no source at all.
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

/// The notes naming the palette entries one edition builds with loss, in
/// the order they print, under the figure that counts them.
///
/// Returned rather than printed, like [`unsupported_notes`], and printed
/// before them, in the order the row lists the figures. One line per entry
/// rather than per dropped intent, so the lines count to the figure
/// ([`PortabilityReport`] raises `degraded` only beside a push), each with
/// its states, since one id is listed once per state combination. The
/// sentence is `degradation_detail`'s, the clause `W_INTENT_DEGRADED` also
/// frames.
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
        // Never empty: an entry is listed because the translator dropped
        // something.
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

/// The notes naming the palette entries one edition has no form for, in
/// the order they print, under the figure that counts them.
///
/// Returned rather than printed so a test can read the whole block. The
/// lines count to the row's figure: [`PortabilityReport`] raises
/// `unsupported` only beside a push.
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

/// The lines refusing one edition's portability row, in the order they
/// print.
///
/// Returned rather than printed, like [`unsupported_notes`]: which edition
/// lost its row, the translator's own sentences, and where such a
/// blockstate can come from. From a `.crn` the one path is a stair state
/// literal with a `facing` or `half` value outside the Java domain
/// (`@oak_stairs[facing=up]`) or a key other than `facing` / `half` /
/// `shape`; a literal on any other block is counted `unsupported` instead.
/// Every other path is a pack or compiler bug.
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
///
/// Two sentences for the two repairs the single figure cannot tell apart:
/// change the material, or wait for the backend to map the states. Each
/// ends on what the reader can do.
fn unsupported_reason(reason: &UnsupportedReason) -> String {
    match reason {
        UnsupportedReason::AbsentFromEdition {
            suggestion,
            aliases,
        } => {
            let absent = "no supported version of this edition declares the block";
            // The alias table answers first: it is the pack naming the block
            // this edition spells differently, which no edit distance finds.
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
        // The backend, not the edition, lacks these states; saying otherwise
        // would send an author away from a design that works.
        UnsupportedReason::StatesUnmapped { states, mapped } => format!(
            "the edition has the block; this compiler maps states for {mapped} so far, so \
             `{states}` has no form here yet — bind the slot to a property-free material, or \
             build for the other edition"
        ),
    }
}

/// `items` joined by `separator`, or `empty` when there are none.
fn joined_or(empty: &str, separator: &str, items: impl IntoIterator<Item = String>) -> String {
    let items: Vec<String> = items.into_iter().collect();
    if items.is_empty() {
        empty.to_owned()
    } else {
        items.join(separator)
    }
}

fn print_axes_report(axes: &VersionAxes) {
    // One entry: the row reads only floors that name no edition
    // (`spec/versioning-editions` "The `registry compatibility` row"), and
    // the per-edition answer is `buildable targets`.
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

    // Beside `buildable targets`, not folded into it: one is what the file
    // was designed for, the other what this compiler can build.
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

/// One edition's buildable versions, with the refusing ones named after
/// them.
///
/// The refusals are named because a bare `1.21.0` cannot be told apart
/// from a pack that declares only that version.
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
