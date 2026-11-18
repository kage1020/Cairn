//! A source lowered for one edition's build as `cairn compile` lowers it, with what can still
//! refuse the build: its findings, the scopes it dropped, and a version floor that rules out the
//! target.

use cairn_lang_core::ast::Module;
use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::weigh_target;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{VersionFloor, declared_version_floors, resolve};
use cairn_lang_core::{Diagnostic, Edition, lower};
use cairn_lang_redstone::Diagnostic as CircuitDiagnostic;

use crate::info::{Conflict, build_diagnostics, dropped_scopes, lay_circuits, requires_conflicts};
use crate::registry::{builtin_pack, builtin_version_tables};

/// A source lowered for one edition's build.
#[derive(Debug, Clone)]
pub struct Build {
    /// What the build paints, with every finding of the core passes.
    pub block_ir: BlockArrayIr,
    /// The findings of laying each struct's circuit, none when the lowering held an error.
    pub circuits: Vec<CircuitDiagnostic>,
    /// The scopes that produced no voxels.
    pub dropped_scopes: Vec<String>,
    /// The finding by which the source's floors refuse the target, which then pins no version.
    pub floor_refusal: Option<Diagnostic>,
    /// The floors above the last version that declares some id the build paints, whatever the
    /// target.
    pub conflicts: Vec<Conflict>,
}

/// The finding that refuses the version `target` for `floors`, the floors `edition`'s build is
/// held to, or `None` when every floor admits it.
///
/// # Panics
///
/// When every floor is placeable and `target` names no row of `edition`'s version table.
#[must_use]
pub fn floor_refusal(
    floors: &[VersionFloor],
    edition: Edition,
    target: &str,
) -> Option<Diagnostic> {
    weigh_target(
        floors,
        edition,
        target,
        &builtin_version_tables(),
        &|version| format!("--target {version}"),
    )
}

/// `module` resolved and lowered for `edition`'s build at the version `target`, with its Intent
/// IR. Ids are checked against no version when `target` is `None` or a floor refuses it, so the
/// refusal, not an `E_UNKNOWN_ID`, explains the failed build.
///
/// # Panics
///
/// When the source declares a floor `edition` can place and `target` is no row of its table.
#[must_use]
pub fn lower_for_build(
    module: &Module,
    edition: Edition,
    target: Option<&str>,
) -> (IntentModule, Build) {
    let pack = builtin_pack(edition);
    let floors = declared_version_floors(module, edition);
    let refusal = target.and_then(|target| floor_refusal(&floors, edition, target));
    let mc_version = target.filter(|_| refusal.is_none());
    let ir = lower(module);
    let resolution = resolve(&ir, Some(edition));
    let registry = pack.view(mc_version);
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&registry));
    let conflicts = if floors.is_empty() {
        Vec::new()
    } else {
        let considered = pack.targetable_versions();
        let at_newest = (mc_version.is_some()
            && mc_version == considered.last().map(String::as_str))
        .then_some(block_ir.diagnostics.as_slice());
        requires_conflicts(
            &ir,
            &resolution,
            pack,
            &considered,
            &floors,
            &pack.version_order(),
            at_newest,
        )
    };
    // `check`'s gate, so a build never certifies a source `check` rejects.
    block_ir.diagnostics = build_diagnostics(
        module,
        &ir,
        Some(edition),
        &[edition],
        std::mem::take(&mut block_ir.diagnostics),
    );
    let dropped_scopes = dropped_scopes(&resolution, &block_ir);
    let circuits = lay_circuits(&ir, &mut block_ir, edition, &registry);
    let build = Build {
        block_ir,
        circuits,
        dropped_scopes,
        floor_refusal: refusal,
        conflicts,
    };
    (ir, build)
}
