//! Semantic layer for the Intent IR: [`resolve`] pairs each `mat_slot=` with its theme slot, each
//! theme selector with the members it matches, and each `place` and `connect` across scopes;
//! [`compute_axes`] computes the version axes `cairn info` reports.

mod binding;
mod connect;
mod output_names;
mod resolution;
mod resolver;
mod selector;
mod site;
mod slots;
mod theme_reference;
mod theme_variant;
mod version_axes;

pub use crate::version::{
    FloorPlacement, FloorVerdict, Requirement, RequirementError, VersionOrder, compare_versions,
    parse_min_version, parse_requirement,
};
pub use binding::{SelectorMatch, ThemeBinding, TokenKind, classify_token};
pub use resolution::{
    PortRef, Resolution, ResolvedMemberBinding, ScopeResolution, ValidatedConnect,
};
pub use resolver::{place_scope_key, resolve};
pub(crate) use selector::select_the_same_members;
pub use version_axes::{
    BuildableRefusal, BuildableTargets, DeclaredFloor, DegradedEntry, DroppedIntent,
    EditionPortability, EditionReport, FloorDeclarer, FloorOrigin, FloorPart, RefusedTarget,
    RegistryRange, SemanticSensitiveFinding, TargetRefusal, UnsupportedEntry, UnsupportedReason,
    VersionAxes, VersionFloor, compute_axes, declared_version_floors, unscoped_version_floors,
    versions_satisfying,
};
