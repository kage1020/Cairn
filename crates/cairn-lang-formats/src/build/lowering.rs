//! A source lowered for a build: against a pinned target's id table, or with no target for the
//! findings `cairn check` reports without one.

use cairn_lang_core::block_array::{BlockArrayIr, findings_without_target, lower_to_block_array};
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{declared_version_floors, resolve};
use cairn_lang_core::{Diagnostic, Edition, Module, lower};
use cairn_lang_redstone::Diagnostic as CircuitDiagnostic;

use super::circuits::lay_circuits;
use super::floors::{DeclaredFloors, floor_refusing_target};
use super::gate::{build_diagnostics, weighed_editions};
use super::run_level::{RunLevelMessage, partial_build, unsupported_target, version_floor_refusal};
use super::scopes::{built_scopes, dropped_scopes};
use crate::data_version::UnsupportedTarget;
use crate::registry::builtin_pack;
use crate::target::{ResolvedTarget, resolve_target};

/// What [`check_lowering`] found, and what the run owes a refusal for.
#[derive(Debug)]
pub struct CheckLowering {
    /// The block-array pass's findings; without a target, those every target reading their line
    /// raises.
    pub diagnostics: Vec<Diagnostic>,
    /// Scopes that produced no voxels at the pinned target, empty without one.
    pub dropped_scopes: Vec<String>,
    /// How many scopes lowered at the pinned target, so the refusal can say "1 of 2"; 0 without
    /// one.
    pub built_scopes: usize,
    /// Why a `--target` did not resolve.
    pub unsupported: Option<UnsupportedTarget>,
}

/// Lower against `cairn check --target`'s id table, or with no target when none is given; an
/// unresolved target lowers unpinned, so every finding that needs no version is still reported.
#[must_use]
pub fn check_lowering(
    ir: &IntentModule,
    edition: Option<Edition>,
    target: Option<&str>,
) -> CheckLowering {
    // A `--target` comes with an `--edition` (`spec/compilation` "Target axes").
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

/// A source parsed, resolved and lowered for one edition's build, plus what the lowering failed
/// to produce.
#[derive(Debug)]
pub struct Lowered {
    /// The lowered structures, whose diagnostics are every finding `cairn check` reports for the
    /// build, then the lowering's.
    pub block_ir: BlockArrayIr,
    /// The findings of laying each struct's circuit, none when the lowering held an error.
    pub circuits: Vec<CircuitDiagnostic>,
    /// Scopes that produced no voxels.
    pub dropped_scopes: Vec<String>,
    /// The `@requires` floors this edition's build is held to, if the source declares any.
    pub version_floors: Option<DeclaredFloors>,
}

/// Resolve and lower `module` for `edition`'s build, its circuits laid.
///
/// Ids are checked against `target` unless it is `None` or the source's floors refuse it: the
/// floor refusal then explains the failed build, not an `E_UNKNOWN_ID`.
#[must_use]
pub fn lower_for_target(
    module: &Module,
    edition: Edition,
    target: Option<&ResolvedTarget>,
) -> Lowered {
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
    // `check`'s gate, so a build never certifies a source `check` rejects.
    block_ir.diagnostics = build_diagnostics(
        module,
        &ir,
        Some(edition),
        &weighed_editions(Some(edition)),
        std::mem::take(&mut block_ir.diagnostics),
    );
    let dropped_scopes = dropped_scopes(&resolution, &block_ir);
    let circuits = lay_circuits(&ir, &mut block_ir, edition, &registry);
    Lowered {
        block_ir,
        circuits,
        dropped_scopes,
        version_floors,
    }
}

impl Lowered {
    /// The target a build whose findings hold no error writes for, or the run-level refusal
    /// `cairn compile` makes before it writes: a lost scope, an unshipped `--target`, then a floor
    /// that rules the target out.
    ///
    /// # Errors
    ///
    /// The first of those refusals that holds.
    pub fn writable_target(
        &self,
        edition: Edition,
        target: Result<ResolvedTarget, UnsupportedTarget>,
    ) -> Result<ResolvedTarget, RunLevelMessage> {
        // Every code that drops a scope is a warning, so this is what refuses a partial build.
        if !self.dropped_scopes.is_empty() {
            return Err(partial_build(
                &self.dropped_scopes,
                built_scopes(&self.block_ir),
                "refusing to certify a partial build",
            ));
        }
        let target = target.map_err(|err| unsupported_target(&err))?;
        match self
            .version_floors
            .as_ref()
            .and_then(|floors| version_floor_refusal(floors, edition, &target))
        {
            Some(refusal) => Err(refusal),
            None => Ok(target),
        }
    }
}
