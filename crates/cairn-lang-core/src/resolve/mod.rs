//! Semantic layer for the Intent IR: [`resolve`] pairs each `mat_slot=` with its theme slot, each
//! theme selector with the members it matches, and each `place` and `connect` across scopes;
//! [`compute_axes`] computes the version axes `cairn info` reports.

mod binding;
mod connect;
mod output_names;
mod resolution;
mod resolver;
mod scope;
mod selector;
mod site;
mod slots;
mod theme_reference;
mod theme_variant;
mod unresolved_ref;
mod version_axes;

pub use crate::version::{
    FloorPlacement, FloorVerdict, Requirement, RequirementError, VersionOrder, compare_versions,
    parse_min_version, parse_requirement,
};
pub use binding::{SelectorMatch, ThemeBinding, TokenKind, classify_token};
pub use resolution::{
    PortRef, Resolution, ResolvedMemberBinding, ScopeResolution, ValidatedConnect, place_scope_key,
};
pub(crate) use resolver::findings_on_branch;
pub use resolver::resolve;
pub(crate) use selector::select_the_same_members;
pub use version_axes::{
    BuildableRefusal, BuildableTargets, DeclaredFloor, DegradedEntry, DroppedIntent,
    EditionPortability, EditionReport, EditionRespellings, FloorDeclarer, FloorOrigin, FloorPart,
    InferredRange, LastDeclaredId, PaintedId, RefusedTarget, RegistryRange, RequiresConflict,
    RespelledToken, SemanticSensitiveFinding, SpellingTable, TargetRefusal, UnsupportedEntry,
    UnsupportedReason, VersionAxes, VersionFloor, WrittenSpelling, compute_axes,
    declared_version_floors, unscoped_version_floors, versions_satisfying,
};
