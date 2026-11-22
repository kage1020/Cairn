//! `cairn diff-blocks`: a structure file compared with what a source builds at the structure's
//! stamped target, the voxel diff that closes the lift loop (`spec/ecosystem-interop` "Diffing a
//! lift").

use std::io::ErrorKind;
use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::voxel_diff::{DiffReport, Metrics, Thresholds, floor3, percent};
use cairn_lang_formats::diff_blocks::{DiffBlocksRefusal, DiffBlocksRun, diff_blocks};
use cairn_lang_formats::import::{
    DecodeFileError, DecodedStructure, LossKind, decode_structure_file,
};
use cairn_lang_formats::info::FailureDocument;

use crate::cli::DiffFormat;
use crate::pipeline::read_source;
use crate::report::{print_json, report_diagnostics, report_run_level, report_structure_message};
use crate::structure_file::refuse_extension;

/// How many regions the text report lists before it counts the rest.
const LISTED_REGIONS: usize = 10;

pub(crate) fn run_diff_blocks(structure: &Path, file: &Path, format: DiffFormat) -> ExitCode {
    let source = match read_source(file) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let decoded = match decode_structure_file(structure) {
        Ok(decoded) => decoded,
        Err(DecodeFileError::UnknownExtension(_)) => {
            refuse_extension(structure, "diff-blocks");
            return ExitCode::from(2);
        }
        Err(err) => {
            eprintln!("error: {err}");
            return match err {
                DecodeFileError::Io { source, .. } if source.kind() == ErrorKind::NotFound => {
                    ExitCode::from(2)
                }
                DecodeFileError::Io { .. } => ExitCode::from(1),
                _ => undecoded(format),
            };
        }
    };
    let run = diff_blocks(&source, &decoded);
    report_run(file, &source, format, &run);
    if format == DiffFormat::Json
        && let Err(code) = print_json("the diff", &run.json_document(&source))
    {
        return code;
    }
    let Ok(compared) = &run.outcome else {
        return ExitCode::from(1);
    };
    if let Some(unread) = uncompared(&decoded) {
        eprintln!(
            "note: only blocks are compared, not the {unread} `{}` holds",
            structure.display()
        );
    }
    let report = &compared.document.report;
    if format == DiffFormat::Text {
        print_text(structure, file, &decoded, report);
    }
    if report.converged {
        return ExitCode::SUCCESS;
    }
    if format == DiffFormat::Text {
        report_unconverged(file, report);
    }
    ExitCode::from(1)
}

/// The exit of a structure that cannot be read as its format; under `--format json`, one document
/// per input still, `{"diagnostics": []}`.
fn undecoded(format: DiffFormat) -> ExitCode {
    if format == DiffFormat::Json
        && let Err(code) = print_json("diagnostics", &FailureDocument::new(Vec::new()))
    {
        return code;
    }
    ExitCode::from(1)
}

/// Report on stderr what building the source found, beside the document under `--format json`,
/// and why a refused run compares nothing.
fn report_run(file: &Path, source: &str, format: DiffFormat, run: &DiffBlocksRun) {
    let lines = LineStarts::new(source);
    let in_document = matches!(run.outcome, Err(DiffBlocksRefusal::Findings { .. }))
        && format == DiffFormat::Json;
    if !in_document {
        report_diagnostics(file, source, &lines, &run.findings);
    }
    match &run.outcome {
        Ok(compared) => compared.degraded.iter().for_each(report_structure_message),
        Err(refusal) => {
            for message in refusal.messages() {
                report_run_level(file, source, &lines, message);
            }
        }
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
