//! What `cairn diff-blocks` reports on a source and the structure file it lifts, computed once for
//! the CLI and the language server (`spec/ecosystem-interop` "The report in an editor").

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::voxel_diff::{DiffInput, DiffReport, diff};
use cairn_lang_core::{Edition, Severity, diagnose_parse_failure, parse};
use serde::Serialize;

use crate::build::{Build, BuildTarget, DeclaredFloors, lower_for_build};
use crate::import::DecodedStructure;
use crate::info::{Conflict, FailureDocument, Found, built_scopes};

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

/// Why a run compares nothing.
#[derive(Debug)]
pub enum DiffBlocksRefusal {
    /// The source does not parse, or a pass raises an error.
    Findings,
    /// A `@requires` floor conflicts with the blocks the build paints.
    Conflicts,
    /// Scopes produced no voxels at the stamp.
    PartialBuild {
        /// The scopes that produced none.
        dropped: Vec<String>,
        /// How many scopes did.
        built: usize,
    },
    /// A `@requires` floor refuses the stamp.
    Floor(DeclaredFloors),
}

/// A run of `cairn diff-blocks`: what the build found, and the document or why there is none.
#[derive(Debug)]
pub struct DiffBlocksRun {
    /// Every finding of building the source, in the order the passes raised them.
    pub findings: Vec<Found>,
    /// The floors no version declaring every id the build paints clears, as
    /// `E_REQUIRES_CONFLICT` reports them.
    pub conflicts: Vec<Conflict>,
    /// The document, or why the run compares nothing.
    pub outcome: Result<DiffBlocksDocument, DiffBlocksRefusal>,
}

impl DiffBlocksRun {
    /// The document a refused run prints in place of the report: every finding for a refusal by
    /// findings, and none for a refusal of the whole run.
    #[must_use]
    pub fn failure_document(&self, source: &str) -> FailureDocument {
        match self.outcome {
            Err(DiffBlocksRefusal::Findings) => FailureDocument::new(source, &self.findings),
            _ => FailureDocument::new(source, &[]),
        }
    }
}

/// Build `source` at the target `structure` is stamped with and compare the two, as
/// `spec/ecosystem-interop` "Diffing a lift" specifies.
///
/// # Panics
///
/// When the stamp names no row of its edition's version table, which no import stamps.
#[must_use]
pub fn diff_blocks(source: &str, structure: &DecodedStructure) -> DiffBlocksRun {
    let module = match parse(source) {
        Ok(module) => module,
        Err(err) => {
            let lines = LineStarts::new(source);
            return DiffBlocksRun {
                findings: vec![Found::Core(diagnose_parse_failure(source, &lines, &err))],
                conflicts: Vec::new(),
                outcome: Err(DiffBlocksRefusal::Findings),
            };
        }
    };
    let stamp = &structure.stamp;
    let target = stamp.build_target();
    let (ir, build) = lower_for_build(&module, stamp.edition, Some(target));
    let Build {
        block_ir,
        circuits,
        dropped_scopes,
        version_floors,
        conflicts,
    } = build;
    let findings: Vec<Found> = block_ir
        .diagnostics
        .iter()
        .cloned()
        .map(Found::Core)
        .chain(circuits.into_iter().map(Found::Circuit))
        .collect();
    let refusal = if findings.iter().any(|d| d.severity() == Severity::Error) {
        Some(DiffBlocksRefusal::Findings)
    } else if !conflicts.is_empty() {
        Some(DiffBlocksRefusal::Conflicts)
    } else if !dropped_scopes.is_empty() {
        Some(DiffBlocksRefusal::PartialBuild {
            dropped: dropped_scopes,
            built: built_scopes(&block_ir),
        })
    } else {
        version_floors
            .filter(|declared| declared.refusing(target.order_key).is_some())
            .map(DiffBlocksRefusal::Floor)
    };
    if let Some(refusal) = refusal {
        return DiffBlocksRun {
            findings,
            conflicts,
            outcome: Err(refusal),
        };
    }
    let build = |text: &str| build_candidate(text, stamp.edition, target);
    let report = diff(&DiffInput {
        units: &structure.units,
        source,
        module: &ir,
        built: &block_ir,
        build: &build,
    });
    DiffBlocksRun {
        findings,
        conflicts,
        outcome: Ok(DiffBlocksDocument {
            structure: structure.file_name.clone(),
            target: StampedTarget {
                edition: stamp.edition,
                version: stamp.mc_version.clone(),
            },
            saved: stamp.saved_label(),
            report,
        }),
    }
}

/// What `text` builds at `target`, `None` when it does not parse, a pass raises an error or a
/// scope is dropped.
fn build_candidate(text: &str, edition: Edition, target: BuildTarget<'_>) -> Option<BlockArrayIr> {
    let module = parse(text).ok()?;
    let (_, build) = lower_for_build(&module, edition, Some(target));
    let refused = build
        .block_ir
        .diagnostics
        .iter()
        .any(|d| d.severity() == Severity::Error)
        || build
            .circuits
            .iter()
            .any(|d| d.severity() == Severity::Error)
        || !build.dropped_scopes.is_empty();
    (!refused).then_some(build.block_ir)
}
