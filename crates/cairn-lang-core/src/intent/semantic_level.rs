//! Maturity tag for an [`IntentModule`](super::IntentModule) (`spec/architecture` "The Intent IR
//! is rich and carries invariants").

use serde::Serialize;

/// Where an Intent IR sits in the raw → grouped → lifted progression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SemanticLevel {
    /// Schematic import not yet clustered into members.
    Raw,
    /// Named members with materials and themes unresolved; what `lower()` returns.
    Grouped,
    /// Every member carries an `intent_state` and a `resolved_state`.
    Lifted,
}
