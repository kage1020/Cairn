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
use cairn_lang_core::suggest::candidate_list;
use cairn_lang_core::{Diagnostic, Edition, Module, Severity, lower};
use cairn_lang_formats::bedrock_state::degradation_detail;
use cairn_lang_formats::portability::{
    InvalidPalette, PortabilityReport, portability_for_bedrock, portability_for_java,
};
use cairn_lang_formats::registry::builtin_java;

use crate::cli::InfoFormat;
use crate::floors::{
    buildable_refusal, report_floors_left_out_of_the_neutral_row, report_version_notes,
    weigh_versions,
};
use crate::pipeline::{
    RequestedEditions, build_diagnostics, builtin_pack, dropped_scopes, load_module,
    requested_editions, supported_versions,
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

    // Surface the check pass (syntactic + resolver) and lowering
    // diagnostics before running the parity dry-run — the same contract
    // `run_check` / `run_lower` / `run_compile` honor. Without this, a
    // `.crn` carrying an `E_UNRESOLVED_SLOT` (or any other Error-severity
    // finding) would still get `cairn info` exit 0 with a portability row
    // computed against a partially unresolved IR, which is a poor CI gate.
    //
    // This gate is edition-neutral — `resolve(ir, None)` unions slot names
    // across per-edition variants — so a file whose only "problem" is that
    // one variant declares a slot the other does not still passes here.
    // The strict per-edition pass runs inside the dry-run below, which
    // reports whatever only it can see.
    let resolution = resolve(&ir, None);
    // `info` reports across every target in the pack's range, so there is
    // no single version to check ids against and the view carries no id
    // table. Guessing one would refuse ids that are fine on the version
    // the author compiles for; `cairn compile --target` is where the
    // question has an answer.
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&builtin_java().view(None)));
    // The editions this report covers, not both by default: `--editions
    // bedrock` asks for a Bedrock report, and a finding only Java's table
    // reaches would refuse the command before a single row was computed.
    // The resolver stays unpinned regardless — that gate is edition-neutral
    // by design, and the strict per-edition pass runs inside the dry-run.
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
    // Warnings on a run that still has a report keep going to stderr as
    // text, in both formats. Folding them into the report would change a
    // document downstream tooling already reads, which is a breaking
    // change and a decision of its own.
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
        // The same refusal the edition-neutral gate above gets, one pass
        // later: which pass raised the finding does not decide whether a
        // document is written. It does still decide what is in one — the
        // gate above hands `&combined` over whole, warnings included,
        // where this hands over the errors alone — which `spec/lint`
        // "Machine-readable payload" states rather than this code hiding.
        Err(refused) => {
            return report_failure_document(file, &source, reporting.report, &refused);
        }
    };

    let axes = compute_axes(&module, &ir, &resolution, rows);

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
/// Each edition in `editions` is walked once, in the order `--editions`
/// first names it: [`requested_editions`] is the only way to build a
/// [`RequestedEditions`]. [`run_info`] hands the same list to
/// [`build_diagnostics`], where it reaches only
/// `intended_target_findings`.
///
/// The per-edition pass is strict where the caller's neutral pass is
/// soft: a slot only one variant declares resolves there and not here, and
/// that finding is reported under the edition rather than folded into a
/// smaller `portable` count. A palette the pack refuses drops that
/// edition's row with a report of why; the other editions are still walked
/// to the end before the exit code is returned.
///
/// The version loop cannot be replaced by intersecting the range-wide
/// palette's id sets: with no target pinned every material takes its
/// default mapping, so a token the target respells is compared as the
/// wrong id. A version is buildable when it passes the source gates
/// [`run_compile`](crate::commands::compile::run_compile) applies — the pinned lowering raises no error, the
/// `@requires` floor is at or below it, and every declared scope lowered.
/// Each pinned lowering's findings print under the version that raised
/// them; the floor and the dropped scopes get one line per edition, since
/// their reason is the same for every version.
///
/// `already_reported` is the edition-neutral stream the caller has
/// printed; only diagnostics absent from it are reported, keyed by code
/// and span.
///
/// The error case is the findings the failure document owes rather than
/// an exit code, so the caller writes that document through the one place
/// that knows what `--format json` promises. Under
/// [`FailureReport::Json`] the errors come back here instead of printing,
/// because the document is written once, after every edition has been
/// walked, and a finding printed on the way would reach the reader ahead
/// of it. Under [`FailureReport::Text`] every finding prints inline where
/// it always has — under the note naming the edition that raised it — and
/// the list comes back empty, which leaves stderr saying exactly what it
/// says today. Warnings print on stderr either way; `spec/lint`
/// "Machine-readable payload" states that as the contract rather than
/// this code arguing for it.
///
/// A refused palette adds nothing to the list, in either format:
/// [`invalid_palette_report`] renders prose rather than a `Diagnostic`,
/// since no leak it names has a span in the source. So an error-severity finding from another edition can
/// leave the list non-empty with that refusal invisible in it, and a run
/// refused by the palette alone comes back as `Err` of an empty list —
/// which under `--format json` is the `{"diagnostics": []}` the spec asks
/// for, and under text is the exit code and the prose already on stderr.
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
        // Same reason as the pass above: no single version, so the lowering
        // gets no id table and raises no `E_UNKNOWN_ID`. The portability
        // fold below still reads the pack's tables — it asks the wider
        // question "does this edition have the block at all", which the
        // whole range can answer.
        let block_ir = lower_to_block_array(ir, &resolution, Some(&pack.view(None)));

        let only_here: Vec<_> = resolution
            .diagnostics
            .iter()
            .chain(block_ir.diagnostics.iter())
            .filter(|d| !already.contains(&(d.code.as_str(), d.span.start, d.span.end)))
            .cloned()
            .collect();
        edition_specific_error |= only_here.iter().any(|d| d.severity() == Severity::Error);
        // The errors are the run's refusal, so under `--format json` they
        // are held for the failure document. The warnings stay on stderr
        // in both formats: they belong to a row the caller is about to
        // discard, and a warning read beside a figure that was never
        // printed is worse placed in the document than beside the note
        // naming the edition that raised it. `spec/lint` "Machine-readable
        // payload" states that, so the header prints only when something
        // is left for it to head.
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
        // What it does not do is leave the edition. The rest of this body
        // reads the resolution and the pinned lowerings, not the palette,
        // so the dropped scopes and each version's refusals are findings
        // the leak has nothing to do with — and stderr is the only place
        // they can appear, since the report is discarded. Skipping them
        // here would hide within one edition exactly what walking every
        // edition exists to prevent between two.
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

        // The floors are collected per edition rather than once for the
        // file: an unscoped floor is in every edition's build and a scoped
        // one only in its own, so the set a Bedrock row is weighed against
        // is not the set a Java row is.
        let floors = declared_version_floors(module, edition);
        let considered = supported_versions(pack);
        let verdicts = weigh_versions(ir, &resolution, pack, &floors, &considered, &dropped);
        for (version, refusals) in &verdicts.refused {
            eprintln!("note: {} {version} refuses this source", edition.as_str());
            report_diagnostics(file, source, lines, refusals);
        }
        report_version_notes(file, source, lines, edition, &verdicts, &considered);

        // The one thing a refused palette does cost this edition: with no
        // counts there is no row to build. The run exits 1 below and the
        // list is dropped either way, so what this guards is the shape
        // rather than the output — a row whose figures came off a palette
        // no validated pack could have produced.
        if let Some(portability) = portability {
            let refusal = buildable_refusal(source, lines, &verdicts, &considered, &dropped);
            // Only `portable` is taken off the counts: the other two are
            // the lengths of the lists below, and `compute_axes` derives
            // them there so this call cannot pair a figure with a list
            // that disagrees.
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

    // Every requested edition is walked before returning, so one bad edition
    // does not hide a second one's findings.
    //
    // The flag is what decides this and not `refused.is_empty()`: a
    // refused palette refuses the run and contributes no finding, so an
    // empty list is not "nothing refused" — reading the list instead would
    // give that run exit 0 and a report missing a row.
    if edition_specific_error {
        return Err(refused);
    }
    Ok(rows)
}

/// One edition's portability figure, or `None` where its palette carried
/// blockstates a registry pack is expected to refuse.
///
/// There is no figure to print over such a palette — the counts would read
/// as ordinary portability — so the edition contributes no row and the run
/// exits 1. The refusal reports here, as prose: no leak it names has a
/// span in the source, so it is a run-level refusal rather than a finding,
/// the shape `spec/lint` "Machine-readable payload" gives one.
///
/// Split out of [`edition_rows`] so the `Err` arm can be driven without a
/// source: a `.crn` reaches it only through a state literal on a stair — a
/// `facing` or `half` value outside the Java domain, or a key other than
/// `facing` / `half` / `shape` — and the pack and compiler bugs it also
/// catches have no source at all. Taking the `Result` rather than
/// computing it is what lets
/// `a_refused_palette_says_which_edition_lost_its_row_and_whose_bug_it_is`
/// drive the decision with the lowering it already builds.
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
/// before it so the two blocks read in the order the row lists their
/// figures. The figure is `entries.len()`: [`PortabilityReport`] raises
/// `degraded` only beside a push. These go to stderr; a consumer that
/// wants them structured reads `edition_portability[].degraded_entries`.
///
/// One line per entry rather than per dropped intent, so the lines can be
/// counted against the figure, and each line carries the entry's states
/// because one id reaches the list once per state combination. The
/// sentence is `degradation_detail`, the one place the wording is
/// written; the build's `W_INTENT_DEGRADED` frames that same clause with
/// the block named, so the two tell one story rather than two.
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
        // `dropped` is non-empty by construction — an entry is in this
        // list because the translator handed back at least one — so the
        // join has nothing to fall back to.
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
/// figure is `entries.len()`: [`PortabilityReport`] raises `unsupported`
/// only beside a push, so the list's length is the number the row prints.
/// These go to stderr; a consumer that wants them structured reads
/// `edition_portability[].unsupported_entries`.
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
/// Returned rather than printed, like [`unsupported_notes`]. The header
/// says which edition lost its row and why; the entries are the
/// translator's own sentences, unreworded; the closing note says where
/// such a blockstate can come from. The one path from a `.crn` is a state
/// literal on a stair — a `facing` or `half` value outside the Java
/// domain (`@oak_stairs[facing=up]`), or a key other than `facing` /
/// `half` / `shape` — since nothing checks a literal against the target
/// yet; every other is a pack or compiler bug. A literal on any other
/// block never gets here: the walk counts it `unsupported` instead.
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
/// Two sentences for two repairs — change the material, or wait for the
/// backend to map the states — which is what the single figure they fold
/// into cannot be read as. Each one ends on what the reader can do,
/// including the one nobody can do anything about, because "nothing here
/// is yours to fix" is itself the answer that stops them looking.
fn unsupported_reason(reason: &UnsupportedReason) -> String {
    match reason {
        UnsupportedReason::AbsentFromEdition {
            suggestion,
            aliases,
        } => {
            let absent = "no supported version of this edition declares the block";
            // The alias table answers first where it has anything to say:
            // it is the pack stating that this edition has the block under
            // another name, which a distance over the id text cannot find
            // and which turns the row from a dead end into an edit.
            match (aliases.as_slice(), suggestion) {
                ([], None) => absent.to_owned(),
                ([], Some(suggestion)) => format!("{absent}; did you mean `{suggestion}`?"),
                (spellings, _) => format!(
                    "no supported version of this edition declares that id; it spells the \
                     block {}",
                    candidate_list(spellings),
                ),
            }
        }
        // The edition is not what cannot express these — this backend is,
        // and only so far. Saying otherwise would send an author to
        // abandon a design that works, and leave the missing mapping
        // unreported because nobody was told there was one.
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
    // Axis 1 is one entry because the answer is: the row reads only the
    // floors that name no edition, so there is nothing per-edition to
    // render (`spec/versioning-editions` "The `registry compatibility`
    // row"). A floor written in Java's numbering says nothing about the
    // file's Bedrock range, and the per-edition answer is the `buildable
    // targets` row below.
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

    // Beside the row it can contradict, and not folded into it: one is
    // what the file says it was designed for and the other what this
    // compiler can build it for, and a reader comparing them is doing the
    // comparison `E_INTENDED_TARGET_CAP` automates for the half of it that
    // is decidable.
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
/// The refusals are in the sentence rather than left to be inferred: a bare
/// `1.21.0` cannot be told apart from an edition whose pack declares only
/// that one version, and the difference between "one target of three" and
/// "the only target there is" is the whole point of the row.
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
