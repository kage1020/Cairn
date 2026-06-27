//! Maturity tag for an [`IntentModule`](super::IntentModule).
//!
//! Cairn distinguishes three IR maturity states (see `spec/architecture.md`
//! §3.2). The current `lower()` produces [`SemanticLevel::Grouped`] — the
//! surface AST has been mechanically lifted into named members, but
//! materials/themes have not been resolved yet. [`SemanticLevel::Lifted`]
//! is the next maturity tier; [`SemanticLevel::Raw`] is reserved for
//! schematic-import paths once that surface lands.

use serde::Serialize;

/// Where an Intent IR sits in the raw → grouped → lifted progression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SemanticLevel {
    /// Schematic import that has not yet been clustered into member candidates.
    Raw,
    /// AST has been lowered into named members, but materials/themes have
    /// not been resolved against any registry. This is what the current
    /// `lower()` returns.
    Grouped,
    /// Materials, themes, and per-edition resolution have completed; every
    /// member carries an `intent_state` and a `resolved_state`. Future
    /// maturity tier.
    Lifted,
}
