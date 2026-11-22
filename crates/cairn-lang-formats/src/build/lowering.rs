//! A source lowered for a build: against a pinned target's id table, or with no target for the
//! findings `cairn check` reports without one.

use cairn_lang_core::block_array::{BlockArrayIr, findings_without_target, lower_to_block_array};
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{declared_version_floors, resolve};
use cairn_lang_core::{Diagnostic, Edition, Module, Severity, lower};
use cairn_lang_redstone::Diagnostic as CircuitDiagnostic;

use super::circuits::lay_circuits;
use super::conflicts::requires_conflicts;
use super::finding::Finding;
use super::floors::floor_refusal;
use super::gate::build_diagnostics;
use super::run_level::{RunLevelMessage, partial_build, requires_conflict, unsupported_target};
use super::scopes::{built_scopes, dropped_scopes};
use crate::data_version::{ResolvedTarget, UnsupportedTarget, resolve_target};
use crate::registry::{BuiltinEditions, builtin_pack, builtin_view};

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
            diagnostics: findings_without_target(ir, edition, &BuiltinEditions),
            dropped_scopes: Vec::new(),
            built_scopes: 0,
            unsupported: None,
        };
    };
    let resolved = resolve_target(edition, target);
    let pinned = resolved.as_ref().ok().map(ResolvedTarget::mc_version);
    // The theme variant decides which id a `mat_slot=` reaches, so resolve for the edition.
    let resolution = resolve(ir, Some(edition));
    let registry = builtin_view(edition, pinned);
    let block_ir = lower_to_block_array(ir, &resolution, Some(&registry));
    CheckLowering {
        dropped_scopes: dropped_scopes(&resolution, &block_ir),
        built_scopes: built_scopes(&block_ir),
        diagnostics: block_ir.diagnostics,
        unsupported: resolved.err(),
    }
}

/// A source parsed, resolved and lowered for one edition's build at a target, plus what the
/// lowering failed to produce.
#[derive(Debug)]
pub struct Lowered {
    /// The target the build was asked for, or why it does not resolve.
    pub target: Result<ResolvedTarget, UnsupportedTarget>,
    /// The lowered structures, whose diagnostics are every finding `cairn check` reports for the
    /// build, then the lowering's.
    pub block_ir: BlockArrayIr,
    /// The findings of laying each struct's circuit, none when the lowering held an error.
    pub circuits: Vec<CircuitDiagnostic>,
    /// Scopes that produced no voxels.
    pub dropped_scopes: Vec<String>,
    /// The finding by which the source's floors refuse the target, which then pins no version.
    pub floor_refusal: Option<Diagnostic>,
    /// `E_REQUIRES_CONFLICT` for each floor above the last version that declares some id the
    /// build paints, which refuses the build at every target.
    pub conflicts: Vec<RunLevelMessage>,
}

/// Resolve and lower `module` for `edition`'s build at `target`, its circuits laid.
///
/// Ids are checked against the target unless it does not resolve or the source's floors refuse
/// it: the floor refusal then explains the failed build, not an `E_UNKNOWN_ID`.
#[must_use]
pub fn lower_for_target(
    module: &Module,
    edition: Edition,
    target: Result<ResolvedTarget, UnsupportedTarget>,
) -> Lowered {
    let floors = declared_version_floors(module, edition);
    let resolved = target.as_ref().ok();
    let refusal = resolved.and_then(|target| floor_refusal(&floors, edition, target));
    let mc_version = resolved
        .filter(|_| refusal.is_none())
        .map(ResolvedTarget::mc_version);
    let ir = lower(module);
    let resolution = resolve(&ir, Some(edition));
    let pack = builtin_pack(edition);
    let registry = builtin_view(edition, mc_version);
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
        .iter()
        .map(|conflict| requires_conflict(edition, conflict))
        .collect()
    };
    // `check`'s gate, so a build never certifies a source `check` rejects.
    block_ir.diagnostics = build_diagnostics(
        module,
        &ir,
        Some(edition),
        &Edition::weighed(Some(edition)),
        std::mem::take(&mut block_ir.diagnostics),
    );
    let dropped_scopes = dropped_scopes(&resolution, &block_ir);
    let circuits = lay_circuits(&ir, &mut block_ir, edition, &registry);
    Lowered {
        target,
        block_ir,
        circuits,
        dropped_scopes,
        floor_refusal: refusal,
        conflicts,
    }
}

/// Why a lowered build writes nothing.
#[derive(Debug)]
pub enum BuildRefusal<'a> {
    /// A finding of the lowering, or of laying a circuit, is an error.
    Findings,
    /// A floor admits no target declaring every id the build paints: [`Lowered::conflicts`].
    Conflicts,
    /// A refusal no finding makes: a lost scope, or a target that does not resolve.
    RunLevel(RunLevelMessage),
    /// The finding by which the source's floors refuse the target.
    Floor(&'a Diagnostic),
}

impl Lowered {
    /// The target the build writes for, or the first refusal `cairn compile` makes before it
    /// writes: an error finding, a conflicting floor, a lost scope, a target that does not
    /// resolve, then a floor that rules the target out.
    ///
    /// # Errors
    ///
    /// The first of those refusals that holds.
    pub fn verdict(&self) -> Result<&ResolvedTarget, BuildRefusal<'_>> {
        let errs = self
            .block_ir
            .diagnostics
            .iter()
            .map(Diagnostic::severity)
            .chain(self.circuits.iter().map(Finding::severity))
            .any(|severity| severity == Severity::Error);
        if errs {
            return Err(BuildRefusal::Findings);
        }
        // A conflict holds at every target, so it stops the build ahead of the target's floor.
        if !self.conflicts.is_empty() {
            return Err(BuildRefusal::Conflicts);
        }
        // Every code that drops a scope is a warning, so this is what refuses a partial build.
        if !self.dropped_scopes.is_empty() {
            return Err(BuildRefusal::RunLevel(partial_build(
                &self.dropped_scopes,
                built_scopes(&self.block_ir),
                "refusing to certify a partial build",
            )));
        }
        let target = self
            .target
            .as_ref()
            .map_err(|err| BuildRefusal::RunLevel(unsupported_target(err)))?;
        match &self.floor_refusal {
            Some(refusal) => Err(BuildRefusal::Floor(refusal)),
            None => Ok(target),
        }
    }
}

#[cfg(test)]
mod tests;
