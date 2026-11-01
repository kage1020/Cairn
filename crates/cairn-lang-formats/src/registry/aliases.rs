//! The aliases component of a registry pack: groups of the spellings one block has worn across
//! editions and versions, which `E_UNKNOWN_ID` offers where a typo search finds nothing. A group
//! names no version; the target's `blocks` table picks its members. A spelling two editions use
//! for different blocks, such as `snow`, gets no group, since one group would answer both.

use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

/// The highest `aliases.schema_version` this build reads.
pub const SUPPORTED_ALIASES_SCHEMA: u32 = 1;

/// The body of `aliases.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct AliasCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace for a spelling that names none.
    pub namespace: String,
    /// The groups, in the order a suggestion lists their members.
    pub groups: Vec<AliasGroup>,
}

/// One `groups` row of an [`AliasCatalog`].
#[derive(Debug, Clone, Deserialize)]
pub struct AliasGroup {
    /// Every spelling of the block, at least two.
    pub spellings: Vec<String>,
}

/// Validated alias groups, ready for lookup.
#[derive(Debug, Clone)]
pub struct AliasIndex {
    /// The groups in declared order, every spelling namespaced.
    groups: Vec<Vec<String>>,
    /// Each spelling's group; a spelling belongs to exactly one.
    by_spelling: IndexMap<String, usize>,
}

impl AliasIndex {
    /// An index with no groups, for a pack without an `aliases` component.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            groups: Vec::new(),
            by_spelling: IndexMap::new(),
        }
    }

    /// Validates `catalog` into an index.
    ///
    /// # Errors
    ///
    /// [`AliasError`] for the first fault in the catalog.
    pub fn from_catalog(catalog: AliasCatalog) -> Result<Self, AliasError> {
        if catalog.schema_version > SUPPORTED_ALIASES_SCHEMA {
            return Err(AliasError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_ALIASES_SCHEMA,
            });
        }
        if catalog.groups.is_empty() {
            return Err(AliasError::NoGroups);
        }

        let namespace = catalog.namespace.as_str();
        let mut groups: Vec<Vec<String>> = Vec::with_capacity(catalog.groups.len());
        let mut by_spelling: IndexMap<String, usize> = IndexMap::new();
        for group in catalog.groups {
            let at = groups.len();
            let mut spellings: Vec<String> = Vec::with_capacity(group.spellings.len());
            for spelling in group.spellings {
                let id = namespaced(namespace, &spelling);
                match by_spelling.get(&id) {
                    Some(&owner) if owner == at => {
                        return Err(AliasError::DuplicateSpelling { id });
                    }
                    Some(_) => return Err(AliasError::SpellingInTwoGroups { id }),
                    None => {}
                }
                by_spelling.insert(id.clone(), at);
                spellings.push(id);
            }
            if spellings.len() < 2 {
                return Err(AliasError::GroupTooSmall {
                    spellings: spellings.join(", "),
                });
            }
            groups.push(spellings);
        }
        Ok(Self {
            groups,
            by_spelling,
        })
    }

    /// Every spelling in `id`'s group, `id` included, in declared order; empty when none claims it.
    #[must_use]
    pub fn spellings_of(&self, id: &str) -> &[String] {
        self.by_spelling
            .get(id)
            .map_or(&[], |&at| self.groups[at].as_slice())
    }

    /// Every group, in declared order.
    pub fn groups(&self) -> impl Iterator<Item = &[String]> {
        self.groups.iter().map(Vec::as_slice)
    }

    /// The number of groups.
    #[must_use]
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Whether the index has no groups.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// A fault in an [`AliasCatalog`], always the pack author's.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum AliasError {
    /// The catalog's `schema_version` is newer than this build reads.
    #[error(
        "unsupported registry pack aliases schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// The catalog declared no groups; a pack without aliases leaves the component out.
    #[error("registry pack aliases declares no groups")]
    NoGroups,
    /// A group carried fewer than two spellings.
    #[error("registry pack aliases group [{spellings}] has fewer than two spellings")]
    GroupTooSmall {
        /// Its spellings, comma-joined; empty for an empty group.
        spellings: String,
    },
    /// One group listed the same spelling twice.
    #[error("registry pack aliases group declares `{id}` more than once")]
    DuplicateSpelling {
        /// The spelling, namespaced.
        id: String,
    },
    /// Two groups claimed the same spelling; merging them is usually the fix.
    #[error("registry pack aliases declares `{id}` in two groups")]
    SpellingInTwoGroups {
        /// The spelling both groups claimed.
        id: String,
    },
}

#[cfg(test)]
mod tests;
