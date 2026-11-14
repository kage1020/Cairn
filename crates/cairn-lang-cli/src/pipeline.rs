//! The front half the subcommands share: reading, parsing, checking and lowering a source.

use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::block_array::{BlockArrayIr, findings_without_target, lower_to_block_array};
use cairn_lang_core::check::LineStarts;
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{declared_version_floors, resolve};
use cairn_lang_core::{Diagnostic, Edition, diagnose_parse_failure, lower, parse};
use cairn_lang_formats::info::{Conflict, kept_findings, requires_conflicts, supported_versions};
pub(crate) use cairn_lang_formats::info::{
    build_diagnostics, built_scopes, dropped_scopes, lay_circuits,
};
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_redstone::Diagnostic as CircuitDiagnostic;

use crate::floors::{DeclaredFloors, floor_refusing_target};
use crate::report::{FailureReport, report_failure_document};
use crate::target::{ResolvedTarget, resolve_target};

/// Read a source, or report why not: exit 2 for a path that names nothing, 1 for anything else.
pub(crate) fn read_source(file: &Path) -> Result<String, ExitCode> {
    std::fs::read_to_string(file).map_err(|err| {
        eprintln!("error: cannot read `{}`: {err}", file.display());
        match err.kind() {
            std::io::ErrorKind::NotFound => ExitCode::from(2),
            _ => ExitCode::from(1),
        }
    })
}

/// Read and parse a source, reporting a parse failure as `report` asks.
pub(crate) fn load_module(
    file: &Path,
    report: FailureReport,
) -> Result<(String, cairn_lang_core::ast::Module), ExitCode> {
    let source = read_source(file)?;
    let module = parse(&source).map_err(|err| {
        let lines = LineStarts::new(&source);
        let parse_failure = [diagnose_parse_failure(&source, &lines, &err)];
        report_failure_document(file, &source, report, &parse_failure)
    })?;
    Ok((source, module))
}

/// What [`check_lowering`] found, and what the run owes a refusal for.
pub(crate) struct CheckLowering {
    /// The block-array pass's findings; without a target, those every target reading their line
    /// raises.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Scopes that produced no voxels at the pinned target, empty without one.
    pub(crate) dropped_scopes: Vec<String>,
    /// How many scopes lowered at the pinned target, so the refusal can say "1 of 2"; 0 without
    /// one.
    pub(crate) built_scopes: usize,
    /// Why a `--target` did not resolve.
    pub(crate) unsupported: Option<cairn_lang_formats::data_version::UnsupportedTarget>,
}

/// Lower against `cairn check --target`'s id table, or with no target when none is given; an
/// unresolved target lowers unpinned, so every finding that needs no version is still reported.
pub(crate) fn check_lowering(
    ir: &cairn_lang_core::intent::IntentModule,
    edition: Option<Edition>,
    target: Option<&str>,
) -> CheckLowering {
    // clap refuses `--target` without `--edition` (`spec/compilation` "Target axes").
    let (Some(edition), Some(target)) = (edition, target) else {
        return CheckLowering {
            diagnostics: findings_without_target(ir, edition, &|edition, id| {
                builtin_pack(edition).may_declare(id)
            }),
            dropped_scopes: Vec::new(),
            built_scopes: 0,
            unsupported: None,
        };
    };
    let resolved = resolve_target(edition, target);
    let pinned = resolved.as_ref().ok().map(ResolvedTarget::mc_version);
    // The theme variant decides which id a `mat_slot=` reaches, so resolve for the edition.
    let resolution = resolve(ir, Some(edition));
    let registry = builtin_pack(edition).view(pinned);
    let block_ir = lower_to_block_array(ir, &resolution, Some(&registry));
    CheckLowering {
        dropped_scopes: dropped_scopes(&resolution, &block_ir),
        built_scopes: built_scopes(&block_ir),
        diagnostics: block_ir.diagnostics,
        unsupported: resolved.err(),
    }
}

/// Report the scopes a lowering lost, as `E_PARTIAL_BUILD`, ending the line with `because`.
pub(crate) fn report_partial_build(file: &Path, dropped: &[String], built: usize, because: &str) {
    eprintln!(
        "error[E_PARTIAL_BUILD]: {}: {} of {} requested scopes did not lower; {because}",
        file.display(),
        dropped.len(),
        dropped.len() + built,
    );
    for scope in dropped {
        eprintln!("  note: `{scope}` produced no voxels");
    }
}

/// The editions a command with one optional pin is about: the pin, or both without one.
pub(crate) fn weighed_editions(pin: Option<Edition>) -> Vec<Edition> {
    pin.map_or_else(|| vec![Edition::Java, Edition::Bedrock], |one| vec![one])
}

/// The circuit findings `cairn check` reports: the pinned edition's, at the pinned version when
/// `target` names one it ships, or without a pin those each edition raises at the same span and
/// each edition's own, which touch a line under its guards (`spec/lint` "Error vs warning").
pub(crate) fn check_circuits(
    ir: &IntentModule,
    edition: Option<Edition>,
    target: Option<&str>,
) -> Vec<CircuitDiagnostic> {
    let resolved = edition
        .zip(target)
        .and_then(|(edition, target)| resolve_target(edition, target).ok());
    let pinned = resolved.as_ref().map(ResolvedTarget::mc_version);
    let editions = weighed_editions(edition);
    let runs: Vec<Vec<CircuitDiagnostic>> = editions
        .iter()
        .map(|&edition| {
            let registry = builtin_pack(edition).view(pinned);
            let mut built = lower_to_block_array(ir, &resolve(ir, Some(edition)), Some(&registry));
            lay_circuits(ir, &mut built, edition, &registry)
        })
        .collect();
    let guarded: Vec<Vec<&Span>> = editions
        .iter()
        .map(|&edition| ir.lines_guarded_for(edition))
        .collect();
    kept_findings(&runs, |run, finding| finding.touches(&guarded[run]))
}

/// A `.crn` read, parsed, resolved and lowered, plus what the lowering failed to produce.
pub(crate) struct Lowered {
    pub(crate) source: String,
    pub(crate) block_ir: BlockArrayIr,
    /// The findings of laying each struct's circuit, none when the lowering held an error.
    pub(crate) circuits: Vec<CircuitDiagnostic>,
    pub(crate) dropped_scopes: Vec<String>,
    /// The `@requires` floors this edition's build is held to, if the source declares any.
    pub(crate) version_floors: Option<DeclaredFloors>,
    /// The floors no version declaring every id the build paints clears, whatever the target.
    pub(crate) conflicts: Vec<Conflict>,
}

/// Read, parse, resolve and lower `file` for `edition`'s build.
///
/// Ids are checked against `target` unless it is `None` or the source's floors refuse it: the
/// floor refusal then explains the failed build, not an `E_UNKNOWN_ID`.
pub(crate) fn load_and_lower(
    file: &Path,
    edition: Edition,
    target: Option<&ResolvedTarget>,
) -> Result<Lowered, ExitCode> {
    let (source, module) = load_module(file, FailureReport::Text)?;
    Ok(lower_parsed(source, &module, edition, target).1)
}

/// `module`, parsed from `source`, resolved and lowered for `edition`'s build as
/// [`load_and_lower`] lowers a file, with its Intent IR.
pub(crate) fn lower_parsed(
    source: String,
    module: &cairn_lang_core::ast::Module,
    edition: Edition,
    target: Option<&ResolvedTarget>,
) -> (IntentModule, Lowered) {
    let floors = declared_version_floors(module, edition);
    let version_floors = (!floors.is_empty()).then(|| DeclaredFloors {
        floors,
        order: builtin_pack(edition).version_order(),
    });
    let mc_version = target
        .filter(|target| {
            version_floors.as_ref().is_none_or(|declared| {
                floor_refusing_target(&declared.floors, &declared.order, target).is_none()
            })
        })
        .map(ResolvedTarget::mc_version);
    let ir = lower(module);
    let resolution = resolve(&ir, Some(edition));
    let registry = builtin_pack(edition).view(mc_version);
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&registry));
    // `check`'s gate, so `compile` never certifies a source `check` rejects.
    block_ir.diagnostics = build_diagnostics(
        module,
        &ir,
        Some(edition),
        &weighed_editions(Some(edition)),
        std::mem::take(&mut block_ir.diagnostics),
    );
    let dropped_scopes = dropped_scopes(&resolution, &block_ir);
    let circuits = lay_circuits(&ir, &mut block_ir, edition, &registry);
    let conflicts = version_floors.as_ref().map_or_else(Vec::new, |declared| {
        let pack = builtin_pack(edition);
        requires_conflicts(
            &ir,
            &resolution,
            pack,
            &supported_versions(pack),
            &declared.floors,
            &declared.order,
        )
    });
    let lowered = Lowered {
        source,
        block_ir,
        circuits,
        dropped_scopes,
        version_floors,
        conflicts,
    };
    (ir, lowered)
}

/// A walled three-by-three `struct probe` lowered unpinned, for unit tests to rekey or corrupt.
#[cfg(test)]
pub(crate) fn lowered_probe(edition: Edition) -> BlockArrayIr {
    let module = parse("struct probe size=3x3\n  walls height=2\n").expect("the probe parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition));
    lower_to_block_array(&ir, &resolution, Some(&builtin_pack(edition).view(None)))
}
