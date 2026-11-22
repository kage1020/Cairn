//! `cairn diff-blocks`: a structure file compared with what a source builds at the structure's
//! stamped target, the voxel diff that closes the lift loop (`spec/ecosystem-interop` "Diffing a
//! lift").

use std::borrow::Cow;
use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::voxel_diff::{
    DiffInput, DiffReport, Metrics, Thresholds, diff, floor3, percent,
};
use cairn_lang_core::{Edition, parse};
use cairn_lang_formats::bedrock_structure::frames_as_blocks;
use cairn_lang_formats::build::artifact::prepare_artifact;
use cairn_lang_formats::build::finding::Found;
use cairn_lang_formats::build::lowering::{BuildRefusal, Lowered, lower_for_target};
use cairn_lang_formats::build::run_level::{RunLevelMessage, floor_refusal_message, partial_build};
use cairn_lang_formats::build::scopes::built_scopes;
use cairn_lang_formats::data_version::{ResolvedTarget, resolve_target};
use cairn_lang_formats::import::{DecodedStructure, ImportFormat, LossKind, decode_structure};
use serde::Serialize;

use crate::cli::DiffFormat;
use crate::pipeline::load_module;
use crate::report::{
    print_json, report_diagnostics, report_failure_document, report_run_level,
    report_structure_message,
};
use crate::structure_file::{file_name, open_structure, refuse_extension};

/// How many regions the text report lists before it counts the rest.
const LISTED_REGIONS: usize = 10;

pub(crate) fn run_diff_blocks(structure: &Path, file: &Path, format: DiffFormat) -> ExitCode {
    let report = format.failure_report();
    let Some(structure_format) = ImportFormat::from_path(structure) else {
        refuse_extension(structure, "diff-blocks");
        return ExitCode::from(2);
    };
    let reader = match open_structure(structure) {
        Ok(reader) => reader,
        Err(code) => return code,
    };
    let (source, module) = match load_module(file, report) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    let decoded = match decode_structure(reader, structure_format, &file_name(structure)) {
        Ok(decoded) => decoded,
        Err(err) => {
            eprintln!("error: cannot compare `{}`: {err}", structure.display());
            return refused(file, &source, format);
        }
    };
    let edition = decoded.stamp.edition();
    let lowered = lower_for_target(
        &module,
        edition,
        resolve_target(edition, decoded.stamp.mc_version()),
    );
    let target = match compared_target(structure, file, format, &source, &lowered) {
        Ok(target) => target,
        Err(code) => return code,
    };
    match checked_by_backend(&lowered.block_ir, target) {
        Ok(degraded) => degraded.iter().for_each(report_structure_message),
        Err(refusal) => {
            report_structure_message(&refusal);
            return refused(file, &source, format);
        }
    }
    if let Some(unread) = uncompared(&decoded) {
        eprintln!(
            "note: only blocks are compared, not the {unread} `{}` holds",
            structure.display()
        );
    }
    let build = |text: &str| build_candidate(text, edition, target);
    let verdict = diff(&DiffInput {
        structure: decoded.bodies(),
        source: &source,
        module: &lowered.ir,
        built: &as_saved(edition, Cow::Borrowed(&lowered.block_ir)),
        build: &build,
    });
    match format {
        DiffFormat::Text => print_text(structure, file, &decoded, &verdict),
        DiffFormat::Json => {
            let document = DiffDocument {
                structure: &decoded.file_name,
                target: TargetDocument {
                    edition,
                    version: target.mc_version(),
                },
                saved: decoded.stamp.saved_label(),
                report: &verdict,
            };
            if let Err(code) = print_json("the diff", &document) {
                return code;
            }
        }
    }
    if verdict.converged {
        return ExitCode::SUCCESS;
    }
    if format == DiffFormat::Text {
        report_unconverged(file, &verdict);
    }
    ExitCode::from(1)
}

/// The target `lowered` is compared at, after reporting what it found, or the exit code when it
/// refuses the comparison: an error, a conflicting floor, a scope it dropped, or a `@requires`
/// floor the stamped target falls short of.
fn compared_target<'a>(
    structure: &Path,
    file: &Path,
    format: DiffFormat,
    source: &str,
    lowered: &'a Lowered,
) -> Result<&'a ResolvedTarget, ExitCode> {
    let findings: Vec<Found> = lowered
        .block_ir
        .diagnostics
        .iter()
        .cloned()
        .map(Found::Core)
        .chain(lowered.circuits.iter().cloned().map(Found::Circuit))
        .collect();
    let report = format.failure_report();
    let lines = LineStarts::new(source);
    // As in `compile`, every conflict is reported, beside the error findings too.
    let report_conflicts = || {
        for conflict in &lowered.conflicts {
            report_run_level(file, source, &lines, conflict);
        }
    };
    let refusal = match lowered.verdict() {
        Ok(target) => {
            report_diagnostics(file, source, &lines, &findings);
            return Ok(target);
        }
        Err(BuildRefusal::Findings) => {
            let code = report_failure_document(file, source, report, &findings);
            report_conflicts();
            return Err(code);
        }
        Err(refusal) => refusal,
    };
    report_diagnostics(file, source, &lines, &findings);
    report_conflicts();
    match refusal {
        BuildRefusal::Findings | BuildRefusal::Conflicts => {}
        BuildRefusal::RunLevel(_) if !lowered.dropped_scopes.is_empty() => {
            let message = partial_build(
                &lowered.dropped_scopes,
                built_scopes(&lowered.block_ir),
                "refusing to compare a partial build",
            );
            report_run_level(file, source, &lines, &message);
        }
        BuildRefusal::RunLevel(message) => report_run_level(file, source, &lines, &message),
        BuildRefusal::Floor(refusal) => {
            report_run_level(file, source, &lines, &floor_refusal_message(refusal));
            if let Ok(target) = &lowered.target {
                eprintln!(
                    "  note: `cairn diff-blocks` builds the source at the stamp of `{}`, {} {}, \
                     so its `@requires` must admit that target",
                    structure.display(),
                    target.edition(),
                    target.mc_version(),
                );
            }
        }
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

/// Each structure of `block_ir` checked by `target`'s backend as `cairn compile` checks it before
/// writing: the `W_INTENT_DEGRADED` warnings, or the first refusal.
fn checked_by_backend(
    block_ir: &BlockArrayIr,
    target: &ResolvedTarget,
) -> Result<Vec<RunLevelMessage>, RunLevelMessage> {
    let mut degraded = Vec::new();
    for (scope, array) in &block_ir.structures {
        degraded.extend(prepare_artifact(block_ir, scope, array, target)?.degraded);
    }
    Ok(degraded)
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

/// What `text` builds at `target`, `None` when the build or the backend refuses it.
fn build_candidate(text: &str, edition: Edition, target: &ResolvedTarget) -> Option<BlockArrayIr> {
    let module = parse(text).ok()?;
    let lowered = lower_for_target(&module, edition, Ok(target.clone()));
    if lowered.verdict().is_err() || checked_by_backend(&lowered.block_ir, target).is_err() {
        return None;
    }
    Some(as_saved(edition, Cow::Owned(lowered.block_ir)).into_owned())
}

/// The blocks `block_ir` saves as on `edition`: on Bedrock, each item frame as its block.
fn as_saved(edition: Edition, block_ir: Cow<'_, BlockArrayIr>) -> Cow<'_, BlockArrayIr> {
    match edition {
        Edition::Java => block_ir,
        Edition::Bedrock => Cow::Owned(frames_as_blocks(&block_ir)),
    }
}

/// The document `--format json` prints.
#[derive(Serialize)]
struct DiffDocument<'a> {
    /// The structure file's name.
    structure: &'a str,
    /// The structure's stamp, which the source is built at.
    target: TargetDocument<'a>,
    /// The version the structure was saved at.
    saved: String,
    #[serde(flatten)]
    report: &'a DiffReport,
}

#[derive(Serialize)]
struct TargetDocument<'a> {
    edition: Edition,
    version: &'a str,
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
