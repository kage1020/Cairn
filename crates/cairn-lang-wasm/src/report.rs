//! What each export answers, as Rust values: the build `cairn compile` runs, the report `cairn
//! info` prints, and the versions an edition can build for.

use std::path::Path;

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::{LineStarts, RenderedDiagnostic};
use cairn_lang_core::{Edition, Module, Severity, diagnose_parse_failure};
use cairn_lang_formats::build::artifact::prepare_artifact;
use cairn_lang_formats::build::finding::{Finding, Found, render_findings};
use cairn_lang_formats::build::lowering::lower_for_target;
use cairn_lang_formats::build::run_level::{
    RenderedRunLevelMessage, RunLevelMessage, artifact_collision,
};
use cairn_lang_formats::info::{
    FailureDocument, InfoDocument, InfoRequest, InfoSink, RequestedEditions,
};
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_formats::target::{ResolvedTarget, resolve_target, supported_versions};
use serde::Serialize;

/// One structure file a build writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// The file's name, `cottage.nbt` or `cottage.mcstructure`.
    pub name: String,
    /// The file's bytes, as `cairn compile` writes them.
    pub bytes: Vec<u8>,
}

/// What [`compile`] answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Compiled {
    /// Whether the build wrote its structure files: what `cairn compile` exits 0 for.
    pub ok: bool,
    /// Every finding the build reports, in the shape `spec/lint` "Machine-readable payload" gives.
    pub diagnostics: Vec<RenderedDiagnostic>,
    /// The run-level refusals, `E_REQUIRES_CONFLICT` among them, and the warnings that name no
    /// span, in the order they arose.
    pub run_level: Vec<RenderedRunLevelMessage>,
    /// The block-array IR the structure files are written from, once no finding is an error.
    pub ir: Option<BlockArrayIr>,
    /// The structure files, in the IR's order, when the build is `ok`.
    #[serde(skip)]
    pub artifacts: Vec<Artifact>,
}

/// `cairn compile --edition <edition> --target <target>` over `source`, writing to memory.
#[must_use]
pub fn compile(source: &str, edition: Edition, target: &str) -> Compiled {
    let lines = LineStarts::new(source);
    let mut compiled = Compiled {
        ok: false,
        diagnostics: Vec::new(),
        run_level: Vec::new(),
        ir: None,
        artifacts: Vec::new(),
    };
    let module = match parse(source, &lines) {
        Ok(module) => module,
        Err(parse_failure) => {
            compiled.diagnostics.push(*parse_failure);
            return compiled;
        }
    };
    let resolved = resolve_target(edition, target);
    let lowered = lower_for_target(&module, edition, resolved.as_ref().ok());
    let findings: Vec<Found> = lowered
        .block_ir
        .diagnostics
        .iter()
        .cloned()
        .map(Found::Core)
        .chain(lowered.circuits.iter().cloned().map(Found::Circuit))
        .collect();
    compiled.diagnostics = render_findings(source, &lines, &findings);
    compiled.run_level = lowered
        .conflicts
        .iter()
        .map(|conflict| conflict.render(source, &lines))
        .collect();
    if findings.iter().any(|d| d.severity() == Severity::Error) {
        return compiled;
    }
    // A conflict holds at every target, so it stops the build ahead of the target's floor check.
    if lowered.conflicts.is_empty() {
        match lowered.writable_target(edition, resolved) {
            Ok(target) => {
                match artifacts(&lowered.block_ir, &target, &mut compiled.run_level, |m| {
                    m.render(source, &lines)
                }) {
                    Ok(artifacts) => {
                        compiled.ok = true;
                        compiled.artifacts = artifacts;
                    }
                    Err(refusal) => compiled.run_level.push(refusal.render(source, &lines)),
                }
            }
            Err(refusal) => compiled.run_level.push(refusal.render(source, &lines)),
        }
    }
    compiled.ir = Some(lowered.block_ir);
    compiled
}

/// Every structure of `block_ir` written for `target`, each warning its backend raises pushed to
/// `warnings` as it arises.
///
/// # Errors
///
/// The backend's refusal of a structure, or two structures whose files share a name.
///
/// # Panics
///
/// If a checked structure fails to encode into memory.
fn artifacts(
    block_ir: &BlockArrayIr,
    target: &ResolvedTarget,
    warnings: &mut Vec<RenderedRunLevelMessage>,
    render: impl Fn(&RunLevelMessage) -> RenderedRunLevelMessage,
) -> Result<Vec<Artifact>, RunLevelMessage> {
    let mut written: Vec<(&str, Artifact)> = Vec::with_capacity(block_ir.structures.len());
    for (scope, array) in &block_ir.structures {
        let prepared = prepare_artifact(block_ir, scope, array, target)?;
        warnings.extend(prepared.degraded.iter().map(&render));
        if let Some((first, _)) = written
            .iter()
            .find(|(_, artifact)| artifact.name == prepared.file_name)
        {
            return Err(artifact_collision(&prepared.file_name, first, scope));
        }
        let mut bytes = Vec::new();
        prepared
            .structure
            .write(&mut bytes)
            .expect("a checked structure fails to write only on I/O, and memory takes every byte");
        written.push((
            scope,
            Artifact {
                name: prepared.file_name,
                bytes,
            },
        ));
    }
    Ok(written.into_iter().map(|(_, artifact)| artifact).collect())
}

/// What [`info`] answers: the document `cairn info --format json` prints.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Informed {
    /// The report, when no finding refuses the run.
    Report(Box<InfoDocument>),
    /// The findings that refuse it, as `spec/lint` "Machine-readable payload" gives them.
    Refused(FailureDocument),
}

/// `cairn info --editions <editions> --format json` over `source`, which has no path and so no
/// lock beside it.
#[must_use]
pub fn info(source: &str, editions: &RequestedEditions) -> Informed {
    let request = InfoRequest {
        file: Path::new(""),
        source,
        editions,
        lock: Path::new(""),
        errors_in_document: true,
    };
    match cairn_lang_formats::info::info(&request, &mut Unsent) {
        Ok(document) => Informed::Report(Box::new(document)),
        Err(refused) => Informed::Refused(FailureDocument::new(source, &refused)),
    }
}

/// What `cairn info` writes on stderr, which a JSON run leaves outside its document.
struct Unsent;

impl InfoSink for Unsent {
    fn finding(&mut self, _: &Found) {}

    fn line(&mut self, _: &str) {}
}

/// The versions one edition's built-in pack can build for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Targets {
    /// The version `latest` names.
    pub latest: String,
    /// Every version a `target` may name, in ascending release order.
    pub versions: Vec<String>,
}

/// The versions `edition` can build for, and the one `latest` names.
///
/// # Panics
///
/// If the built-in pack's `latest` row is not a version it can build for.
#[must_use]
pub fn targets(edition: Edition) -> Targets {
    let latest = resolve_target(edition, "latest")
        .expect("the built-in pack's `latest` row is targetable")
        .mc_version()
        .to_owned();
    Targets {
        latest,
        versions: supported_versions(builtin_pack(edition)),
    }
}

/// `source` parsed, or its `E_PARSE` finding rendered.
fn parse(source: &str, lines: &LineStarts) -> Result<Module, Box<RenderedDiagnostic>> {
    cairn_lang_core::parse(source)
        .map_err(|err| Box::new(diagnose_parse_failure(source, lines, &err).render(source, lines)))
}
