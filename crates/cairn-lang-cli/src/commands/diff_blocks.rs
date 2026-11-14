//! `cairn diff-blocks`: a structure file compared with what a source builds at the structure's
//! stamped target, the voxel diff that closes the lift loop (`spec/ecosystem-interop` "Diffing a
//! lift").

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::voxel_diff::{CONVERGENCE, DiffReport, Metrics, floor3, percent};
use cairn_lang_formats::diff_blocks::{
    DiffBlocksDocument, DiffBlocksRefusal, DiffBlocksRun, diff_blocks,
};
use cairn_lang_formats::import::{ImportFormat, ImportStamp, decode_structure};
use cairn_lang_formats::info::{Reporter, report_conflicts};

use crate::cli::DiffFormat;
use crate::commands::import::{file_name, open_structure, refuse_extension};
use crate::floors::enforce_version_floor;
use crate::pipeline::{read_source, report_partial_build};
use crate::report::{
    FailureReport, Found, Stderr, print_json, report_diagnostics, report_failure_document,
};

/// How many regions the text report lists before it counts the rest.
const LISTED_REGIONS: usize = 10;

pub(crate) fn run_diff_blocks(structure: &Path, file: &Path, format: DiffFormat) -> ExitCode {
    let Some(structure_format) = ImportFormat::from_path(structure) else {
        refuse_extension(structure, "diff-blocks");
        return ExitCode::from(2);
    };
    let reader = match open_structure(structure) {
        Ok(reader) => reader,
        Err(code) => return code,
    };
    let source = match read_source(file) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let decoded = match decode_structure(reader, structure_format, &file_name(structure)) {
        Ok(decoded) => decoded,
        Err(err) => {
            eprintln!("error: cannot compare `{}`: {err}", structure.display());
            return ExitCode::from(1);
        }
    };
    let run = diff_blocks(&source, &decoded);
    let report = format.failure_report();
    let document = match report_run(structure, file, &source, report, &decoded.stamp, run) {
        Ok(document) => document,
        Err(code) => return code,
    };
    match format {
        DiffFormat::Text => print_text(structure, file, &decoded.stamp, &document.report),
        DiffFormat::Json => {
            if let Err(code) = print_json("the diff", &document) {
                return code;
            }
        }
    }
    if document.report.converged {
        return ExitCode::SUCCESS;
    }
    if format == DiffFormat::Text {
        report_unconverged(file, &document.report);
    }
    ExitCode::from(1)
}

/// Report what building the source found, and return the document, or the exit code of the
/// refusal `run` ends in.
fn report_run(
    structure: &Path,
    file: &Path,
    source: &str,
    report: FailureReport,
    stamp: &ImportStamp,
    run: DiffBlocksRun,
) -> Result<DiffBlocksDocument, ExitCode> {
    let DiffBlocksRun {
        findings,
        conflicts,
        outcome,
    } = run;
    let lines = LineStarts::new(source);
    let refused_by_findings = matches!(outcome, Err(DiffBlocksRefusal::Findings));
    let failure = if refused_by_findings {
        Some(report_failure_document(file, source, report, &findings))
    } else {
        report_diagnostics(file, source, &lines, &findings);
        None
    };
    let mut stderr = Stderr {
        file,
        source,
        lines: &lines,
    };
    report_conflicts(
        &mut Reporter::new(file, source, &lines, &mut stderr),
        stamp.edition,
        &conflicts,
    );
    let refusal = match outcome {
        Ok(document) => return Ok(document),
        Err(refusal) => refusal,
    };
    if let Some(code) = failure {
        return Err(code);
    }
    match refusal {
        DiffBlocksRefusal::Findings | DiffBlocksRefusal::Conflicts => {}
        DiffBlocksRefusal::PartialBuild { dropped, built } => {
            report_partial_build(file, &dropped, built, "refusing to compare a partial build");
        }
        DiffBlocksRefusal::Floor(declared) => {
            let target = stamp.build_target();
            if enforce_version_floor(file, source, &declared, stamp.edition, target).is_err() {
                eprintln!(
                    "  note: `cairn diff-blocks` builds the source at the stamp of `{}`, {} {}, \
                     so its `@requires` must admit that target",
                    structure.display(),
                    stamp.edition,
                    stamp.mc_version,
                );
            }
        }
    }
    // One document per input under `--format json`: the refusal is stderr's, the warnings too.
    let none: [Found; 0] = [];
    Err(match report {
        FailureReport::Json => report_failure_document(file, source, report, &none),
        FailureReport::Text => ExitCode::from(1),
    })
}

/// The metrics as three report lines.
fn metric_lines(metrics: &Metrics) -> [String; 3] {
    [
        format!(
            "block IoU       {:<6} converges at >= {}",
            floor3(metrics.block_iou),
            CONVERGENCE.block_iou
        ),
        format!(
            "state_accuracy  {:<6} converges at >= {}",
            floor3(metrics.state_accuracy),
            CONVERGENCE.state_accuracy
        ),
        format!(
            "residual raw    {:<6} converges at <= {}",
            percent(metrics.residual_ratio),
            percent(CONVERGENCE.residual_ratio)
        ),
    ]
}

/// The metrics on one line.
fn metric_summary(metrics: &Metrics) -> String {
    format!(
        "block IoU {}, state_accuracy {}, residual raw {}",
        floor3(metrics.block_iou),
        floor3(metrics.state_accuracy),
        percent(metrics.residual_ratio)
    )
}

/// The text report on stdout.
fn print_text(structure: &Path, file: &Path, stamp: &ImportStamp, verdict: &DiffReport) {
    outln!(
        "{} against {} at {} {}, {}",
        structure.display(),
        file.display(),
        stamp.edition,
        stamp.mc_version,
        stamp.reason()
    );
    for line in metric_lines(&verdict.metrics) {
        outln!("{line}");
    }
    if verdict.bodies.len() > 1 {
        for body in &verdict.bodies {
            let name = match (&body.structure, &body.source) {
                (Some(structure), Some(source)) if structure == source => structure.clone(),
                (Some(structure), Some(source)) => format!("{structure} / {source}"),
                (Some(structure), None) => format!("{structure} / (none)"),
                (None, Some(source)) => format!("(none) / {source}"),
                (None, None) => String::new(),
            };
            outln!("  {name}: {}", metric_summary(&body.metrics));
        }
    }
    match verdict.regions.len() {
        0 => outln!("no cell differs"),
        1 => outln!("1 region differs:"),
        n => outln!("{n} regions differ:"),
    }
    for region in verdict.regions.iter().take(LISTED_REGIONS) {
        outln!("  {region}");
    }
    if verdict.regions.len() > LISTED_REGIONS {
        outln!(
            "  and {} more; `--format json` lists every region",
            verdict.regions.len() - LISTED_REGIONS
        );
    }
    if let Some(patch) = &verdict.patch {
        outln!(
            "suggested patch, reaching {}:",
            metric_summary(&patch.metrics)
        );
        for edit in &patch.edits {
            outln!("  {edit}");
        }
    }
}

/// `E_DECOMPILE_DIFF` on stderr, with what would converge and how to get there.
fn report_unconverged(file: &Path, verdict: &DiffReport) {
    eprintln!(
        "error[E_DECOMPILE_DIFF]: {}",
        verdict.metrics.shortfalls(&CONVERGENCE).join(", ")
    );
    eprintln!(
        "  note: a lift converges at block IoU >= {}, state_accuracy >= {} and residual raw <= {}",
        CONVERGENCE.block_iou,
        CONVERGENCE.state_accuracy,
        percent(CONVERGENCE.residual_ratio)
    );
    let file = file.display();
    if verdict.patch.is_some() {
        eprintln!(
            "  note: append the suggested patch to `{file}`, or edit what the regions name, and \
             run `cairn diff-blocks` again"
        );
    } else if verdict.regions.is_empty() {
        eprintln!(
            "  note: every cell matches; name what the `raw_block`, `raw_fill` and `raw_repeat` \
             lines of `{file}` build as the members they are, and run `cairn diff-blocks` again"
        );
    } else {
        eprintln!(
            "  note: edit what the regions name in `{file}`, and run `cairn diff-blocks` again"
        );
    }
}
