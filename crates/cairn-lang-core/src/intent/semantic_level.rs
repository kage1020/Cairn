//! Maturity tag for an [`IntentModule`](super::IntentModule).
//!
//! Cairn distinguishes three IR maturity states (`spec/architecture` "The Intent IR is rich and
//! carries invariants"). `lower()` produces [`SemanticLevel::Grouped`]: the surface AST lifted into
//! named members, with materials and themes unresolved. [`SemanticLevel::Lifted`] is the next tier,
//! and [`SemanticLevel::Raw`] is reserved for schematic import, which has no surface yet.

use serde::Serialize;

/// Where an Intent IR sits in the raw → grouped → lifted progression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SemanticLevel {
    /// Schematic import that has not yet been clustered into member candidates.
    Raw,
    /// AST lowered into named members, with materials and themes not yet resolved against any
    /// registry. What `lower()` returns.
    Grouped,
    /// Materials, themes and per-edition resolution done; every member carries an `intent_state`
    /// and a `resolved_state`. No pass produces it yet.
    Lifted,
}
