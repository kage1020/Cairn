//! The absent component of a registry pack: the blocks and entities of the other edition this
//! edition has no counterpart for, each with the reason, which `E_PARITY_UNSUPPORTED` reports
//! (`spec/versioning-editions` "A block only the other edition has"). A block this edition spells
//! another way is a rename for `aliases`, not an absence, so the loader refuses an entry it
//! declares.

use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

/// The highest `absent.schema_version` this build reads; version 2 adds `entities`.
pub const SUPPORTED_ABSENT_SCHEMA: u32 = 2;

/// The body of `absent.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct AbsentCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace for an id that names none.
    pub namespace: String,
    /// The rows, each a set of blocks sharing one reason.
    #[serde(default)]
    pub entries: Vec<AbsentEntry>,
    /// The rows, each a set of entities sharing one reason.
    #[serde(default)]
    pub entities: Vec<AbsentEntry>,
}

/// One `entries` row of an [`AbsentCatalog`].
#[derive(Debug, Clone, Deserialize)]
pub struct AbsentEntry {
    /// The other edition's ids for the blocks or entities this edition has no counterpart for.
    pub ids: Vec<String>,
    /// What they are and why this edition has none, as a diagnostic note says it.
    pub reason: String,
}

/// Validated absences, ready for lookup.
#[derive(Debug, Clone)]
pub struct AbsentIndex {
    /// Each namespaced block id's reason, in declared order.
    reasons: IndexMap<String, String>,
    /// Each namespaced entity id's reason, in declared order.
    entities: IndexMap<String, String>,
}

impl AbsentIndex {
    /// An index with no entries, for a pack without an `absent` component.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            reasons: IndexMap::new(),
            entities: IndexMap::new(),
        }
    }

    /// Validates `catalog` into an index.
    ///
    /// # Errors
    ///
    /// [`AbsentError`] for the first fault in the catalog.
    pub fn from_catalog(catalog: AbsentCatalog) -> Result<Self, AbsentError> {
        if catalog.schema_version > SUPPORTED_ABSENT_SCHEMA {
            return Err(AbsentError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_ABSENT_SCHEMA,
            });
        }
        if catalog.entries.is_empty() && catalog.entities.is_empty() {
            return Err(AbsentError::NoEntries);
        }
        Ok(Self {
            reasons: reasons_of(&catalog.namespace, catalog.entries)?,
            entities: reasons_of(&catalog.namespace, catalog.entities)?,
        })
    }

    /// Why this edition has no counterpart for the namespaced `id`, or `None` when the pack does
    /// not list it.
    #[must_use]
    pub fn reason_for(&self, id: &str) -> Option<&str> {
        self.reasons.get(id).map(String::as_str)
    }

    /// Why this edition has no counterpart for the namespaced entity `id`, or `None` when the pack
    /// does not list it.
    #[must_use]
    pub fn entity_reason_for(&self, id: &str) -> Option<&str> {
        self.entities.get(id).map(String::as_str)
    }

    /// Every listed block id, namespaced, in declared order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.reasons.keys().map(String::as_str)
    }

    /// Every listed entity id, namespaced, in declared order.
    pub fn entity_ids(&self) -> impl Iterator<Item = &str> {
        self.entities.keys().map(String::as_str)
    }

    /// Whether the index lists no id.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reasons.is_empty() && self.entities.is_empty()
    }
}

/// Each id of `entries`, namespaced, with its row's reason, or the first fault in them.
fn reasons_of(
    namespace: &str,
    entries: Vec<AbsentEntry>,
) -> Result<IndexMap<String, String>, AbsentError> {
    let mut reasons = IndexMap::new();
    for entry in entries {
        if entry.ids.is_empty() {
            return Err(AbsentError::NoIds {
                reason: entry.reason,
            });
        }
        let reason = entry.reason.trim();
        if reason.is_empty() {
            return Err(AbsentError::NoReason {
                ids: entry.ids.join(", "),
            });
        }
        for id in &entry.ids {
            let id = namespaced(namespace, id);
            if reasons.insert(id.clone(), reason.to_owned()).is_some() {
                return Err(AbsentError::DuplicateId { id });
            }
        }
    }
    Ok(reasons)
}

/// A fault in an [`AbsentCatalog`], always the pack author's.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum AbsentError {
    /// The catalog's `schema_version` is newer than this build reads.
    #[error(
        "unsupported registry pack absent schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// The catalog declared no block and no entity; a pack without absences leaves the component
    /// out.
    #[error("registry pack absent declares no entries")]
    NoEntries,
    /// An entry listed no id.
    #[error("registry pack absent entry \"{reason}\" lists no ids")]
    NoIds {
        /// The entry's reason, which names it.
        reason: String,
    },
    /// An entry gave no reason, which the diagnostic would print as an empty note.
    #[error("registry pack absent entry [{ids}] gives no reason")]
    NoReason {
        /// The entry's ids, comma-joined.
        ids: String,
    },
    /// Two entries, or one twice, listed the same id.
    #[error("registry pack absent lists `{id}` more than once")]
    DuplicateId {
        /// The id, namespaced.
        id: String,
    },
}

#[cfg(test)]
mod tests;
