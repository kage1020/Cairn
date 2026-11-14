//! A source lowered for one edition's build as `cairn compile` lowers it, with what can still
//! refuse the build: its findings, the scopes it dropped, and the `@requires` floors it declares.

use cairn_lang_core::ast::Module;
use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{
    FloorPlacement, FloorVerdict, VersionFloor, VersionOrder, declared_version_floors, resolve,
};
use cairn_lang_core::{Edition, lower};
use cairn_lang_redstone::Diagnostic as CircuitDiagnostic;

use crate::info::{
    Conflict, build_diagnostics, dropped_scopes, lay_circuits, requires_conflicts,
    supported_versions,
};
use crate::registry::builtin_pack;

/// The target a build is pinned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildTarget<'a> {
    /// The version, as a registry view takes it.
    pub mc_version: &'a str,
    /// Its key in its edition's version order, which a floor is weighed against.
    pub order_key: i64,
}

/// The `@requires` floors one edition's build is held to, with that edition's version order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredFloors {
    /// Never empty.
    pub floors: Vec<VersionFloor>,
    /// The edition's `DataVersion` table.
    pub order: VersionOrder,
}

/// The floor that refuses a target, and on which ground.
#[derive(Debug, Clone, Copy)]
pub enum FloorRefusal<'a> {
    /// The edition's version table cannot place this floor.
    Unplaceable(&'a VersionFloor),
    /// The target sits below this floor.
    Below(&'a VersionFloor),
}

impl DeclaredFloors {
    /// The floor that refuses the target at `order_key`, or `None`; an unplaceable floor is named
    /// first, since a later target cannot satisfy it.
    #[must_use]
    pub fn refusing(&self, order_key: i64) -> Option<FloorRefusal<'_>> {
        if let Some(floor) = self
            .floors
            .iter()
            .find(|floor| self.order.place(&floor.version) == FloorPlacement::Unplaceable)
        {
            return Some(FloorRefusal::Unplaceable(floor));
        }
        self.floors
            .iter()
            .find(|floor| self.order.verdict(&floor.version, order_key) == FloorVerdict::Below)
            .map(FloorRefusal::Below)
    }
}

/// A source lowered for one edition's build.
#[derive(Debug, Clone)]
pub struct Build {
    /// What the build paints, with every finding of the core passes.
    pub block_ir: BlockArrayIr,
    /// The findings of laying each struct's circuit, none when the lowering held an error.
    pub circuits: Vec<CircuitDiagnostic>,
    /// The scopes that produced no voxels.
    pub dropped_scopes: Vec<String>,
    /// The `@requires` floors the build is held to, if the source declares any.
    pub version_floors: Option<DeclaredFloors>,
    /// The floors no version declaring every id the build paints clears, whatever the target.
    pub conflicts: Vec<Conflict>,
}

/// `module` resolved and lowered for `edition`'s build, with its Intent IR.
///
/// Ids are checked against `target` unless it is `None` or a floor refuses it: the floor refusal
/// then explains the failed build, not an `E_UNKNOWN_ID`.
#[must_use]
pub fn lower_for_build(
    module: &Module,
    edition: Edition,
    target: Option<BuildTarget<'_>>,
) -> (IntentModule, Build) {
    let pack = builtin_pack(edition);
    let floors = declared_version_floors(module, edition);
    let version_floors = (!floors.is_empty()).then(|| DeclaredFloors {
        floors,
        order: pack.version_order(),
    });
    let mc_version = target
        .filter(|target| {
            version_floors
                .as_ref()
                .is_none_or(|declared| declared.refusing(target.order_key).is_none())
        })
        .map(|target| target.mc_version);
    let ir = lower(module);
    let resolution = resolve(&ir, Some(edition));
    let registry = pack.view(mc_version);
    let mut block_ir = lower_to_block_array(&ir, &resolution, Some(&registry));
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
    let conflicts = version_floors.as_ref().map_or_else(Vec::new, |declared| {
        requires_conflicts(
            &ir,
            &resolution,
            pack,
            &supported_versions(pack),
            &declared.floors,
            &declared.order,
        )
    });
    let build = Build {
        block_ir,
        circuits,
        dropped_scopes,
        version_floors,
        conflicts,
    };
    (ir, build)
}
