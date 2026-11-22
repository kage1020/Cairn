//! What `cairn diff-blocks` reports on a source and the structure file it lifts
//! (`spec/ecosystem-interop` "The report").

use std::slice;

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::voxel_diff::{DiffInput, DiffReport, diff};
use cairn_lang_core::{Edition, diagnose_parse_failure, parse};
use serde::Serialize;

use crate::build::artifact::prepare_artifact;
use crate::build::finding::{Found, render_findings};
use crate::build::lowering::{BuildRefusal, Lowered, lower_for_target};
use crate::build::run_level::{
    RunLevelMessage, RunLevelNote, floor_refusal_message, partial_build,
};
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

/// A comparison a run made: the document, and what the stamp's backend warned of on the way.
#[derive(Debug)]
pub struct Compared {
    /// The document.
    pub document: DiffBlocksDocument,
    /// The `W_INTENT_DEGRADED` warnings of the stamp's backend on what the source builds.
    pub degraded: Vec<RunLevelMessage>,
}

/// Why a run compares nothing, in the order `cairn diff-blocks` checks.
#[derive(Debug)]
pub enum DiffBlocksRefusal {
    /// The source does not parse, or a pass raises an error.
    Findings {
        /// `E_REQUIRES_CONFLICT` for each floor above the last version that declares some id the
        /// build paints.
        conflicts: Vec<RunLevelMessage>,
    },
    /// `E_REQUIRES_CONFLICT` for each floor above the last version that declares some id the
    /// build paints, with no error among the findings.
    Conflicts(Vec<RunLevelMessage>),
    /// A refusal no finding makes: a scope the build dropped.
    RunLevel(RunLevelMessage),
    /// The finding by which a version floor refuses the stamp, ending with a note naming it.
    Floor(RunLevelMessage),
    /// A structure the stamp's backend refuses to write.
    Backend(RunLevelMessage),
}

impl DiffBlocksRefusal {
    /// The run-level messages the refusal reports, in the order `cairn diff-blocks` prints them.
    #[must_use]
    pub fn messages(&self) -> &[RunLevelMessage] {
        match self {
            Self::Findings { conflicts } | Self::Conflicts(conflicts) => conflicts,
            Self::RunLevel(message) | Self::Floor(message) | Self::Backend(message) => {
                slice::from_ref(message)
            }
        }
    }
}

/// A run of `cairn diff-blocks`: what building the source found, and the comparison or why there
/// is none.
#[derive(Debug)]
pub struct DiffBlocksRun {
    /// Every finding of building the source, in the order the passes raised them.
    pub findings: Vec<Found>,
    /// The comparison, or why the run makes none.
    pub outcome: Result<Compared, DiffBlocksRefusal>,
}

/// The document `cairn diff-blocks --format json` prints for a run.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum DiffBlocksJson<'a> {
    /// The report of a comparison.
    Report(&'a DiffBlocksDocument),
    /// Every finding of a source refused by its findings, and none for a run refused whole.
    Refused(FailureDocument),
}

impl DiffBlocksRun {
    /// The document `cairn diff-blocks --format json` prints for the run over `source`.
    #[must_use]
    pub fn json_document(&self, source: &str) -> DiffBlocksJson<'_> {
        match &self.outcome {
            Ok(compared) => DiffBlocksJson::Report(&compared.document),
            Err(DiffBlocksRefusal::Findings { .. }) => {
                let lines = LineStarts::new(source);
                DiffBlocksJson::Refused(FailureDocument::new(render_findings(
                    source,
                    &lines,
                    &self.findings,
                )))
            }
            Err(_) => DiffBlocksJson::Refused(FailureDocument::new(Vec::new())),
        }
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
                DiffBlocksRefusal::Findings {
                    conflicts: Vec::new(),
                },
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
    let target = match compared_target(&lowered, structure) {
        Ok(target) => target,
        Err(refusal) => return refused(findings, refusal),
    };
    let degraded = match checked_by_backend(&lowered.block_ir, target) {
        Ok(degraded) => degraded,
        Err(refusal) => return refused(findings, DiffBlocksRefusal::Backend(refusal)),
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
        outcome: Ok(Compared {
            document: DiffBlocksDocument {
                structure: structure.file_name.clone(),
                target: StampedTarget {
                    edition,
                    version: target.mc_version().to_owned(),
                },
                saved: structure.stamp.saved_label(),
                report,
            },
            degraded,
        }),
    }
}

/// A run that compares nothing, for `refusal`.
fn refused(findings: Vec<Found>, refusal: DiffBlocksRefusal) -> DiffBlocksRun {
    DiffBlocksRun {
        findings,
        outcome: Err(refusal),
    }
}

/// The target `lowered` is compared at, or the refusal `cairn compile` would make first.
fn compared_target<'a>(
    lowered: &'a Lowered,
    structure: &DecodedStructure,
) -> Result<&'a ResolvedTarget, DiffBlocksRefusal> {
    match lowered.verdict() {
        Ok(target) => Ok(target),
        Err(BuildRefusal::Findings) => Err(DiffBlocksRefusal::Findings {
            conflicts: lowered.conflicts.clone(),
        }),
        Err(BuildRefusal::Conflicts) => {
            Err(DiffBlocksRefusal::Conflicts(lowered.conflicts.clone()))
        }
        Err(BuildRefusal::RunLevel(_)) if !lowered.dropped_scopes.is_empty() => {
            Err(DiffBlocksRefusal::RunLevel(partial_build(
                &lowered.dropped_scopes,
                built_scopes(&lowered.block_ir),
                "refusing to compare a partial build",
            )))
        }
        Err(BuildRefusal::RunLevel(message)) => Err(DiffBlocksRefusal::RunLevel(message)),
        Err(BuildRefusal::Floor(refusal)) => {
            let mut message = floor_refusal_message(refusal);
            message.notes.push(RunLevelNote::note(format!(
                "`cairn diff-blocks` builds the source at the stamp of `{}`, {} {}, so its \
                 `@requires` must admit that target",
                structure.file_name,
                structure.stamp.edition(),
                structure.stamp.mc_version(),
            )));
            Err(DiffBlocksRefusal::Floor(message))
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
