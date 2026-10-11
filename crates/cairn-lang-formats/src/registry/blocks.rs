//! The blocks component of a registry pack: each version's block ids, so an unknown id fails
//! (`spec/versioning-editions` "Fail-loud and minimum-version inference"). Versions fold from a
//! base by `inherits` and diffs (`spec/versioning-editions` "Backend = data tables"), and a diff
//! that removes an absent id or adds a present one is refused.

use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

/// The highest `blocks.schema_version` this build reads.
pub const SUPPORTED_BLOCKS_SCHEMA: u32 = 1;

/// The body of `blocks.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct BlocksCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace for an id that names none.
    pub namespace: String,
    /// The oldest version's full id list, which every diff folds from.
    pub base: BlocksBase,
    /// The diffs, in file order; each may inherit only a version resolved before it.
    #[serde(default)]
    pub diffs: Vec<BlocksDiff>,
}

/// The `base` row of a [`BlocksCatalog`].
#[derive(Debug, Clone, Deserialize)]
pub struct BlocksBase {
    /// The version this list describes.
    pub mc_version: String,
    /// Every id valid in that version.
    pub blocks: Vec<String>,
}

/// One `diffs` row of a [`BlocksCatalog`].
#[derive(Debug, Clone, Deserialize)]
pub struct BlocksDiff {
    /// The version this row describes.
    pub mc_version: String,
    /// The version it folds from.
    pub inherits: String,
    /// Ids the inherited version lacks.
    #[serde(default)]
    pub added: Vec<String>,
    /// Ids the inherited version has and this one drops.
    #[serde(default)]
    pub removed: Vec<String>,
}

/// One sorted, namespaced block-id table per version.
#[derive(Debug, Clone)]
pub struct BlocksIndex {
    /// In resolution order: the base, then each diff.
    by_version: IndexMap<String, Vec<String>>,
}

impl BlocksIndex {
    /// An index with no versions, for a pack without a `blocks` component; it refutes no id.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            by_version: IndexMap::new(),
        }
    }

    /// Folds `catalog` into one table per version.
    ///
    /// # Errors
    ///
    /// [`BlocksError`] for the first fault in the catalog.
    pub fn from_catalog(catalog: BlocksCatalog) -> Result<Self, BlocksError> {
        if catalog.schema_version > SUPPORTED_BLOCKS_SCHEMA {
            return Err(BlocksError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_BLOCKS_SCHEMA,
            });
        }
        if catalog.base.blocks.is_empty() {
            return Err(BlocksError::EmptyBase {
                version: catalog.base.mc_version,
            });
        }

        let namespace = catalog.namespace.as_str();
        let mut by_version: IndexMap<String, Vec<String>> = IndexMap::new();

        let mut base: Vec<String> = Vec::with_capacity(catalog.base.blocks.len());
        for block in &catalog.base.blocks {
            let id = namespaced(namespace, block);
            match base.binary_search(&id) {
                Ok(_) => {
                    return Err(BlocksError::DuplicateBlock {
                        version: catalog.base.mc_version.clone(),
                        id,
                    });
                }
                Err(at) => base.insert(at, id),
            }
        }
        by_version.insert(catalog.base.mc_version.clone(), base);

        for diff in &catalog.diffs {
            if by_version.contains_key(&diff.mc_version) {
                return Err(BlocksError::DuplicateVersion {
                    version: diff.mc_version.clone(),
                });
            }
            let parent =
                by_version
                    .get(&diff.inherits)
                    .ok_or_else(|| BlocksError::UnknownInherits {
                        version: diff.mc_version.clone(),
                        inherits: diff.inherits.clone(),
                    })?;
            let mut folded = parent.clone();
            for block in &diff.removed {
                let id = namespaced(namespace, block);
                let at =
                    folded
                        .binary_search(&id)
                        .map_err(|_| BlocksError::RemovedNotInParent {
                            version: diff.mc_version.clone(),
                            inherits: diff.inherits.clone(),
                            id: id.clone(),
                        })?;
                folded.remove(at);
            }
            for block in &diff.added {
                let id = namespaced(namespace, block);
                match folded.binary_search(&id) {
                    Ok(_) => {
                        return Err(BlocksError::AddedAlreadyInParent {
                            version: diff.mc_version.clone(),
                            inherits: diff.inherits.clone(),
                            id,
                        });
                    }
                    Err(at) => folded.insert(at, id),
                }
            }
            if folded.is_empty() {
                return Err(BlocksError::EmptyTable {
                    version: diff.mc_version.clone(),
                });
            }
            by_version.insert(diff.mc_version.clone(), folded);
        }

        Ok(Self { by_version })
    }

    /// The sorted ids of `mc_version`, or `None` when the index does not describe it.
    #[must_use]
    pub fn ids_for(&self, mc_version: &str) -> Option<&[String]> {
        self.by_version.get(mc_version).map(Vec::as_slice)
    }

    /// Whether any version declares `id`, or `None` when the index describes no version.
    #[must_use]
    pub fn declared_by_some_version(&self, id: &str) -> Option<bool> {
        if self.by_version.is_empty() {
            return None;
        }
        Some(
            self.by_version
                .values()
                .any(|ids| ids.binary_search_by(|probe| probe.as_str().cmp(id)).is_ok()),
        )
    }

    /// Every id each version declares, version by version, so a shared id repeats.
    pub fn declared_ids(&self) -> impl Iterator<Item = &str> {
        self.by_version
            .values()
            .flat_map(|ids| ids.iter().map(String::as_str))
    }

    /// Every version this index describes, in resolution order.
    pub fn versions(&self) -> impl Iterator<Item = &str> {
        self.by_version.keys().map(String::as_str)
    }

    /// The number of versions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_version.len()
    }

    /// Whether the index describes no versions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_version.is_empty()
    }
}

/// A fault in a [`BlocksCatalog`], always the pack author's.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlocksError {
    /// The catalog's `schema_version` is newer than this build reads.
    #[error(
        "unsupported registry pack blocks schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// `base.blocks` was empty.
    #[error("registry pack blocks `base` for `{version}` declares no ids")]
    EmptyBase {
        /// The version the base describes.
        version: String,
    },
    /// A diff removed every id its parent had.
    #[error("registry pack blocks table for `{version}` folds to no ids at all")]
    EmptyTable {
        /// The version that folded empty.
        version: String,
    },
    /// One version listed the same id twice.
    #[error("registry pack blocks table for `{version}` declares `{id}` more than once")]
    DuplicateBlock {
        /// The version being folded.
        version: String,
        /// The id, namespaced.
        id: String,
    },
    /// Two rows described the same `mc_version`.
    #[error("registry pack blocks declares version `{version}` more than once")]
    DuplicateVersion {
        /// The version declared twice.
        version: String,
    },
    /// A diff inherits a version not resolved before it: a typo, or rows out of order.
    #[error(
        "registry pack blocks version `{version}` inherits `{inherits}`, which is not resolved before it"
    )]
    UnknownInherits {
        /// The inheriting version.
        version: String,
        /// The version it named.
        inherits: String,
    },
    /// A diff removed an id its parent does not have.
    #[error(
        "registry pack blocks version `{version}` removes `{id}`, which `{inherits}` does not declare"
    )]
    RemovedNotInParent {
        /// The removing version.
        version: String,
        /// The version it inherits.
        inherits: String,
        /// The id.
        id: String,
    },
    /// A diff added an id its parent already has.
    #[error(
        "registry pack blocks version `{version}` adds `{id}`, which `{inherits}` already declares"
    )]
    AddedAlreadyInParent {
        /// The adding version.
        version: String,
        /// The version it inherits.
        inherits: String,
        /// The id.
        id: String,
    },
}

#[cfg(test)]
mod tests;
