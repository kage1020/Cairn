//! Semantic layer for the Intent IR.
//!
//! [`resolve`] walks every `theme`, `def`, `struct` and `site` of a lowered
//! [`crate::intent::IntentModule`] and returns a [`Resolution`]: each
//! member's `mat_slot=NAME` paired with the applied theme's slot, each theme
//! selector row paired with the members it matches, each `place` and
//! `connect` resolved across scopes, and the diagnostics found on the way,
//! which `crate::check` merges into `cairn check`'s output. The module also
//! computes the version axes `cairn info` reports ([`compute_axes`]).
//!
//! Per-edition blockstate (the `Lifted` semantic level) is not resolved
//! here; `Member::resolved_state` is left untouched.
//!
//! The rules come from `spec/materials-themes` (slots, selectors, canonical
//! and abstract tokens) and `spec/versioning-editions` (the version axes).

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
