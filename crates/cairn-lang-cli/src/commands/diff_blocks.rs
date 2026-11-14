//! `cairn diff-blocks`: a structure file compared with what a source builds at the structure's
//! stamped target, the voxel diff that closes the lift loop (`spec/ecosystem-interop` "Diffing a
//! lift").

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::voxel_diff::{
    CONVERGENCE, DiffInput, DiffReport, Metrics, diff, floor3, percent,
};
use cairn_lang_core::{Edition, Severity, parse};
use cairn_lang_formats::import::{DecodedStructure, ImportFormat, decode_structure};
use serde::Serialize;

use crate::cli::DiffFormat;
use crate::commands::import::{file_name, open_structure, refuse_extension};
use crate::floors::enforce_version_floor;
use crate::pipeline::{Lowered, built_scopes, load_module, lower_parsed, report_partial_build};
use crate::report::{Finding, Found, print_json, report_diagnostics, report_failure_document};
use crate::target::{ResolvedTarget, resolve_target};

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
            return ExitCode::from(1);
        }
    };
    let edition = decoded.stamp.edition;
    let target = match resolve_target(edition, &decoded.stamp.mc_version) {
        Ok(target) => target,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };
    let (ir, lowered) = lower_parsed(source, &module, edition, Some(&target));
    if let Some(code) = refuse_build(structure, file, format, &lowered, edition, &target) {
        return code;
    }
    let build = |text: &str| build_candidate(text, edition, &target);
    let verdict = diff(&DiffInput {
        units: &decoded.units,
        source: &lowered.source,
        module: &ir,
        built: &lowered.block_ir,
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

/// Report what `lowered` found, and the exit code when it refuses the comparison: an error, a
/// scope it dropped, or a `@requires` floor the stamped `target` falls short of.
fn refuse_build(
    structure: &Path,
    file: &Path,
    format: DiffFormat,
    lowered: &Lowered,
    edition: Edition,
    target: &ResolvedTarget,
) -> Option<ExitCode> {
    let Lowered {
        source,
        block_ir,
        circuits,
        dropped_scopes,
        version_floors,
    } = lowered;
    let findings: Vec<Found> = block_ir
        .diagnostics
        .iter()
        .cloned()
        .map(Found::Core)
        .chain(circuits.iter().cloned().map(Found::Circuit))
        .collect();
    let report = format.failure_report();
    if findings.iter().any(|d| d.severity() == Severity::Error) {
        return Some(report_failure_document(file, source, report, &findings));
    }
    let lines = LineStarts::new(source);
    report_diagnostics(file, source, &lines, &findings);
    if !dropped_scopes.is_empty() {
        report_partial_build(
            file,
            dropped_scopes,
            built_scopes(block_ir),
            "refusing to compare a partial build",
        );
    } else if let Some(floors) = version_floors
        && let Err(_) = enforce_version_floor(file, source, floors, edition, target)
    {
        eprintln!(
            "  note: `cairn diff-blocks` builds the source at the stamp of `{}`, {edition} {}, \
             so its `@requires` must admit that target",
            structure.display(),
            target.mc_version(),
        );
    } else {
        return None;
    }
    // One document per input under `--format json`: the refusal is stderr's, the warnings too.
    let none: [Found; 0] = [];
    Some(match format {
        DiffFormat::Json => report_failure_document(file, source, report, &none),
        DiffFormat::Text => ExitCode::from(1),
    })
}

/// What `text` builds at `target`, `None` when it does not parse, a pass raises an error or a
/// scope is dropped.
fn build_candidate(text: &str, edition: Edition, target: &ResolvedTarget) -> Option<BlockArrayIr> {
    let module = parse(text).ok()?;
    let (_, lowered) = lower_parsed(text.to_owned(), &module, edition, Some(target));
    let refused = lowered
        .block_ir
        .diagnostics
        .iter()
        .any(|d| d.severity() == Severity::Error)
        || lowered
            .circuits
            .iter()
            .any(|d| d.severity() == Severity::Error)
        || !lowered.dropped_scopes.is_empty();
    (!refused).then_some(lowered.block_ir)
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
fn print_text(structure: &Path, file: &Path, decoded: &DecodedStructure, verdict: &DiffReport) {
    outln!(
        "{} against {} at {} {}, {}",
        structure.display(),
        file.display(),
        decoded.stamp.edition,
        decoded.stamp.mc_version,
        decoded.stamp.reason()
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
