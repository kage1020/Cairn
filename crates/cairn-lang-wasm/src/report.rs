//! What each export answers, as Rust values: the build `cairn compile` runs, the walk `cairn
//! info` reports, and the versions an edition can build for.

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::{LineStarts, RenderedDiagnostic};
use cairn_lang_core::resolve::VersionAxes;
use cairn_lang_core::{Edition, Module, Severity, compute_axes, diagnose_parse_failure, lower};
use cairn_lang_formats::build::artifact::prepare_artifact;
use cairn_lang_formats::build::finding::{Finding, Found, render_findings};
use cairn_lang_formats::build::gate::RequestedEditions;
use cairn_lang_formats::build::info::{edition_runs, info_gate};
use cairn_lang_formats::build::lowering::lower_for_target;
use cairn_lang_formats::build::run_level::{
    RenderedRunLevelMessage, RunLevelMessage, artifact_collision, invalid_palette,
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
    /// The run-level refusal, and the warnings that name no span, in the order they arose.
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
    if findings.iter().any(|d| d.severity() == Severity::Error) {
        return compiled;
    }
    match lowered.writable_target(edition, resolved) {
        Ok(target) => match artifacts(&lowered.block_ir, &target, &mut compiled.run_level, |m| {
            m.render(source, &lines)
        }) {
            Ok(artifacts) => {
                compiled.ok = true;
                compiled.artifacts = artifacts;
            }
            Err(refusal) => compiled.run_level.push(refusal.render(source, &lines)),
        },
        Err(refusal) => compiled.run_level.push(refusal.render(source, &lines)),
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

/// What [`info`] answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Informed {
    /// Whether `cairn info` would print the report: its exit 0.
    pub ok: bool,
    /// Every finding the walk reports: the gate's, then those every edition's circuit raises, then
    /// each edition's own.
    pub diagnostics: Vec<RenderedDiagnostic>,
    /// The run-level refusals: a palette a registry pack should have refused, per edition.
    pub run_level: Vec<RenderedRunLevelMessage>,
    /// The report `cairn info --format json` prints, when the run is `ok`.
    pub axes: Option<VersionAxes>,
}

/// `cairn info --editions <editions>` over `source`.
#[must_use]
pub fn info(source: &str, editions: &RequestedEditions) -> Informed {
    let lines = LineStarts::new(source);
    let mut informed = Informed {
        ok: false,
        diagnostics: Vec::new(),
        run_level: Vec::new(),
        axes: None,
    };
    let module = match parse(source, &lines) {
        Ok(module) => module,
        Err(parse_failure) => {
            informed.diagnostics.push(*parse_failure);
            return informed;
        }
    };
    let ir = lower(&module);
    let gate = info_gate(&module, &ir, editions);
    informed.diagnostics = render_findings(source, &lines, &gate);
    if gate.iter().any(|d| d.severity() == Severity::Error) {
        return informed;
    }
    let runs = edition_runs(&module, &ir, editions, &gate);
    informed
        .diagnostics
        .extend(render_findings(source, &lines, &runs.shared));
    for run in &runs.editions {
        informed
            .diagnostics
            .extend(render_findings(source, &lines, &run.found));
        if let Err(invalid) = &run.portability {
            informed
                .run_level
                .push(invalid_palette(run.edition, invalid).render(source, &lines));
        }
    }
    if runs.refuses() {
        return informed;
    }
    let rows = runs
        .editions
        .into_iter()
        .filter_map(|run| run.report(source, &lines))
        .collect();
    informed.ok = true;
    informed.axes = Some(compute_axes(&module, rows));
    informed
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
