//! Maturity tag for an [`IntentModule`](super::IntentModule).
//!
//! Cairn distinguishes three IR maturity states (see `spec/architecture.md`
//! §3.2). M2's `lower()` produces [`SemanticLevel::Grouped`] — the surface AST
//! has been mechanically lifted into named members, but materials/themes have
//! not been resolved yet. [`SemanticLevel::Lifted`] is the M3 target;
//! [`SemanticLevel::Raw`] is reserved for schematic-import paths (M5+).

use serde::Serialize;

/// Where an Intent IR sits in the raw → grouped → lifted progression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SemanticLevel {
    /// Schematic import that has not yet been clustered into member candidates.
    Raw,
    /// AST has been lowered into named members, but materials/themes have not
    /// been resolved against any registry. This is what M2's `lower()` returns.
    Grouped,
    /// Materials, themes, and per-edition resolution have completed; every
    /// member carries an `intent_state` and a `resolved_state`. M3+ target.
    Lifted,
}
