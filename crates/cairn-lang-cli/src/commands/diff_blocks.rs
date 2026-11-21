//! `cairn diff-blocks`: a structure file compared with what a source builds at the structure's
//! stamped target, the voxel diff that closes the lift loop (`spec/ecosystem-interop` "Diffing a
//! lift").

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::voxel_diff::{DiffReport, Metrics, Thresholds, floor3, percent};
use cairn_lang_formats::build::finding::Found;
use cairn_lang_formats::diff_blocks::{
    DiffBlocksDocument, DiffBlocksRefusal, DiffBlocksRun, diff_blocks,
};
use cairn_lang_formats::import::{DecodedStructure, ImportFormat, LossKind, decode_structure};

use crate::cli::DiffFormat;
use crate::pipeline::read_source;
use crate::report::{
    print_json, report_diagnostics, report_failure_document, report_run_level,
    report_structure_message,
};
use crate::structure_file::{file_name, open_structure, refuse_extension};

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
            return refused(file, &source, format);
        }
    };
    let run = diff_blocks(&source, &decoded);
    let document = match report_run(structure, file, &source, format, &decoded, run) {
        Ok(document) => document,
        Err(code) => return code,
    };
    if let Some(unread) = uncompared(&decoded) {
        eprintln!(
            "note: only blocks are compared, not the {unread} `{}` holds",
            structure.display()
        );
    }
    match format {
        DiffFormat::Text => print_text(structure, file, &decoded, &document.report),
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
    format: DiffFormat,
    decoded: &DecodedStructure,
    run: DiffBlocksRun,
) -> Result<DiffBlocksDocument, ExitCode> {
    let DiffBlocksRun {
        findings,
        conflicts,
        degraded,
        outcome,
    } = run;
    let lines = LineStarts::new(source);
    // As in `compile`, every conflict is reported, beside the error findings too.
    let report_conflicts = || {
        for conflict in &conflicts {
            report_run_level(file, source, &lines, conflict);
        }
    };
    let refusal = match outcome {
        Ok(document) => {
            report_diagnostics(file, source, &lines, &findings);
            degraded.iter().for_each(report_structure_message);
            return Ok(document);
        }
        Err(DiffBlocksRefusal::Findings) => {
            let code = report_failure_document(file, source, format.failure_report(), &findings);
            report_conflicts();
            return Err(code);
        }
        Err(refusal) => refusal,
    };
    report_diagnostics(file, source, &lines, &findings);
    report_conflicts();
    match refusal {
        DiffBlocksRefusal::Findings | DiffBlocksRefusal::Conflicts => {}
        DiffBlocksRefusal::RunLevel(message) => report_run_level(file, source, &lines, &message),
        DiffBlocksRefusal::Floor(message) => {
            report_run_level(file, source, &lines, &message);
            eprintln!(
                "  note: `cairn diff-blocks` builds the source at the stamp of `{}`, {} {}, so its \
                 `@requires` must admit that target",
                structure.display(),
                decoded.stamp.edition(),
                decoded.stamp.mc_version(),
            );
        }
        DiffBlocksRefusal::Backend(message) => report_structure_message(&message),
    }
    Err(refused(file, source, format))
}

/// The exit of a refusal reported on stderr; under `--format json`, one document per input
/// still, `{"diagnostics": []}`.
fn refused(file: &Path, source: &str, format: DiffFormat) -> ExitCode {
    let none: [Found; 0] = [];
    match format {
        DiffFormat::Json => report_failure_document(file, source, format.failure_report(), &none),
        DiffFormat::Text => ExitCode::from(1),
    }
}

/// What `decoded` holds besides blocks, which the comparison leaves out, `None` when nothing.
fn uncompared(decoded: &DecodedStructure) -> Option<&'static str> {
    let holds = |kinds: &[LossKind]| decoded.losses.iter().any(|loss| kinds.contains(&loss.kind));
    let data = holds(&[LossKind::BlockEntities, LossKind::PositionData]);
    match (data, holds(&[LossKind::Entities])) {
        (true, true) => Some("block entity data and entities"),
        (true, false) => Some("block entity data"),
        (false, true) => Some("entities"),
        (false, false) => None,
    }
}

/// The metrics as three report lines, each with the threshold it converges at.
fn metric_lines(metrics: &Metrics, at: &Thresholds) -> [String; 3] {
    [
        format!(
            "block IoU       {:<6} converges at >= {}",
            floor3(metrics.block_iou),
            at.block_iou
        ),
        format!(
            "state_accuracy  {:<6} converges at >= {}",
            floor3(metrics.state_accuracy),
            at.state_accuracy
        ),
        format!(
            "residual raw    {:<6} converges at <= {}",
            percent(metrics.residual_ratio),
            percent(at.residual_ratio)
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
fn print_text(structure: &Path, file: &Path, decoded: &DecodedStructure, verdict: &DiffReport) {
    outln!(
        "{} against {} at {} {}, {}",
        structure.display(),
        file.display(),
        decoded.stamp.edition(),
        decoded.stamp.mc_version(),
        decoded.stamp.reason()
    );
    for line in metric_lines(&verdict.metrics, &verdict.thresholds) {
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
    let at = &verdict.thresholds;
    eprintln!(
        "error[E_DECOMPILE_DIFF]: {}",
        verdict.metrics.shortfalls(at).join(", ")
    );
    eprintln!(
        "  note: a lift converges at block IoU >= {}, state_accuracy >= {} and residual raw <= {}",
        at.block_iou,
        at.state_accuracy,
        percent(at.residual_ratio)
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
