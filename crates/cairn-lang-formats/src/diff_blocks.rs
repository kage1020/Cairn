//! What `cairn diff-blocks` reports on a source and the structure file it lifts, computed once for
//! the CLI and the language server (`spec/ecosystem-interop` "The report in an editor").

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::voxel_diff::{DiffInput, DiffReport, diff};
use cairn_lang_core::{Edition, diagnose_parse_failure, parse};
use serde::Serialize;

use crate::build::artifact::prepare_artifact;
use crate::build::finding::{Finding, Found};
use crate::build::lowering::{BuildRefusal, Lowered, lower_for_target};
use crate::build::run_level::{RunLevelMessage, floor_refusal_message, partial_build};
use crate::build::scopes::built_scopes;
use crate::data_version::{ResolvedTarget, resolve_target};
use crate::import::DecodedStructure;
use crate::info::FailureDocument;

/// The document `cairn diff-blocks --format json` prints for a lift it compares.
#[derive(Debug, Serialize)]
pub struct DiffBlocksDocument {
    /// The structure file's name.
    pub structure: String,
    /// The structure's stamp, which the source is built at.
    pub target: StampedTarget,
    /// The version the structure was saved at.
    pub saved: String,
    /// The comparison.
    #[serde(flatten)]
    pub report: DiffReport,
}

/// The target a structure file is stamped with, as the document names it.
#[derive(Debug, Serialize)]
pub struct StampedTarget {
    /// The edition.
    pub edition: Edition,
    /// The version.
    pub version: String,
}

/// Why a run compares nothing, in the order `cairn diff-blocks` checks.
#[derive(Debug)]
pub enum DiffBlocksRefusal {
    /// The source does not parse, or a pass raises an error.
    Findings,
    /// A `@requires` floor admits no target declaring every id the build paints.
    Conflicts,
    /// A refusal no finding makes: a scope the build dropped, or a stamp the edition does not ship.
    RunLevel(RunLevelMessage),
    /// The finding by which a version floor refuses the stamp.
    Floor(RunLevelMessage),
    /// A structure the stamp's backend refuses to write.
    Backend(RunLevelMessage),
}

/// A run of `cairn diff-blocks`: what the build found, and the document or why there is none.
#[derive(Debug)]
pub struct DiffBlocksRun {
    /// Every finding of building the source, in the order the passes raised them.
    pub findings: Vec<Found>,
    /// `E_REQUIRES_CONFLICT` for each floor above the last version that declares some id the
    /// build paints.
    pub conflicts: Vec<RunLevelMessage>,
    /// The `W_INTENT_DEGRADED` warnings of the stamp's backend on what the source builds.
    pub degraded: Vec<RunLevelMessage>,
    /// The document, or why the run compares nothing.
    pub outcome: Result<DiffBlocksDocument, DiffBlocksRefusal>,
}

impl DiffBlocksRun {
    /// The document a refused run prints in place of the report: every finding for a refusal by
    /// findings, and none for a refusal of the whole run.
    #[must_use]
    pub fn failure_document(&self, source: &str) -> FailureDocument {
        let Err(DiffBlocksRefusal::Findings) = self.outcome else {
            return FailureDocument::new(Vec::new());
        };
        let lines = LineStarts::new(source);
        FailureDocument::new(
            self.findings
                .iter()
                .map(|d| d.render(source, &lines))
                .collect(),
        )
    }
}

/// Build `source` at the target `structure` is stamped with and compare the two, as
/// `spec/ecosystem-interop` "Diffing a lift" specifies.
#[must_use]
pub fn diff_blocks(source: &str, structure: &DecodedStructure) -> DiffBlocksRun {
    let module = match parse(source) {
        Ok(module) => module,
        Err(err) => {
            let lines = LineStarts::new(source);
            return refused(
                vec![Found::Core(diagnose_parse_failure(source, &lines, &err))],
                Vec::new(),
                DiffBlocksRefusal::Findings,
            );
        }
    };
    let edition = structure.stamp.edition();
    let lowered = lower_for_target(
        &module,
        edition,
        resolve_target(edition, structure.stamp.mc_version()),
    );
    let findings: Vec<Found> = lowered
        .block_ir
        .diagnostics
        .iter()
        .cloned()
        .map(Found::Core)
        .chain(lowered.circuits.iter().cloned().map(Found::Circuit))
        .collect();
    let conflicts = lowered.conflicts.clone();
    let target = match compared_target(&lowered) {
        Ok(target) => target,
        Err(refusal) => return refused(findings, conflicts, refusal),
    };
    let degraded = match checked_by_backend(&lowered.block_ir, target) {
        Ok(degraded) => degraded,
        Err(refusal) => return refused(findings, conflicts, DiffBlocksRefusal::Backend(refusal)),
    };
    let build = |text: &str| build_candidate(text, edition, target);
    let report = diff(&DiffInput {
        structure: structure.bodies(),
        source,
        module: &lowered.ir,
        built: &lowered.block_ir,
        build: &build,
    });
    DiffBlocksRun {
        findings,
        conflicts,
        degraded,
        outcome: Ok(DiffBlocksDocument {
            structure: structure.file_name.clone(),
            target: StampedTarget {
                edition,
                version: target.mc_version().to_owned(),
            },
            saved: structure.stamp.saved_label(),
            report,
        }),
    }
}

/// A run that compares nothing, for `refusal`.
fn refused(
    findings: Vec<Found>,
    conflicts: Vec<RunLevelMessage>,
    refusal: DiffBlocksRefusal,
) -> DiffBlocksRun {
    DiffBlocksRun {
        findings,
        conflicts,
        degraded: Vec::new(),
        outcome: Err(refusal),
    }
}

/// The target `lowered` is compared at, or the refusal `cairn compile` would make first.
fn compared_target(lowered: &Lowered) -> Result<&ResolvedTarget, DiffBlocksRefusal> {
    match lowered.verdict() {
        Ok(target) => Ok(target),
        Err(BuildRefusal::Findings) => Err(DiffBlocksRefusal::Findings),
        Err(BuildRefusal::Conflicts) => Err(DiffBlocksRefusal::Conflicts),
        Err(BuildRefusal::RunLevel(_)) if !lowered.dropped_scopes.is_empty() => {
            Err(DiffBlocksRefusal::RunLevel(partial_build(
                &lowered.dropped_scopes,
                built_scopes(&lowered.block_ir),
                "refusing to compare a partial build",
            )))
        }
        Err(BuildRefusal::RunLevel(message)) => Err(DiffBlocksRefusal::RunLevel(message)),
        Err(BuildRefusal::Floor(refusal)) => {
            Err(DiffBlocksRefusal::Floor(floor_refusal_message(refusal)))
        }
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

/// What `text` builds at `target`, `None` when the build or the backend refuses it.
fn build_candidate(text: &str, edition: Edition, target: &ResolvedTarget) -> Option<BlockArrayIr> {
    let module = parse(text).ok()?;
    let lowered = lower_for_target(&module, edition, Ok(target.clone()));
    if lowered.verdict().is_err() || checked_by_backend(&lowered.block_ir, target).is_err() {
        return None;
    }
    Some(lowered.block_ir)
}
