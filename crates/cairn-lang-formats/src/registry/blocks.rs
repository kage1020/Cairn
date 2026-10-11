//! Per-version block-id table component of a [`crate::registry::RegistryPack`].
//!
//! Answers one question: *does this block id exist in the target the compile
//! is pinned to?* `spec/versioning-editions` "Fail-loud and minimum-version
//! inference" makes an unknown id a hard error, and the structural
//! `namespace:path` check passes any well-formed id.
//!
//! The on-disk shape follows the folding rule of
//! `spec/versioning-editions` "Backend = data tables" — "fold versions with
//! `inherits + diffs`" — because the alternative, one full list per version,
//! repeats a thousand shared ids three times and hides the interesting part.
//! What a reader wants from `blocks.json` is exactly the diff: Bedrock
//! 1.21.40 is where `stonebrick` became `stone_bricks` and `light_block`
//! became `light_block_0` … `light_block_15`, and its `removed` list is that
//! flattening wave written out in full.
//!
//! Folding is validated rather than trusted. A diff that removes an id its
//! parent never had, or adds one the parent already has, is a pack-author
//! mistake that would otherwise fold cleanly into a table nobody notices is
//! wrong.

use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

/// Highest `blocks.schema_version` this Cairn build understands.
pub const SUPPORTED_BLOCKS_SCHEMA: u32 = 1;

/// On-disk `blocks.json` body.
#[derive(Debug, Clone, Deserialize)]
pub struct BlocksCatalog {
    /// Schema version of the catalog itself.
    pub schema_version: u32,
    /// Default id namespace. Applied to every entry that does not carry
    /// its own `namespace:` prefix, matching
    /// [`crate::registry::MaterialsCatalog::namespace`].
    pub namespace: String,
    /// The oldest version's full id list. Every other version is expressed
    /// as a diff reachable from here.
    pub base: BlocksBase,
    /// Diffs, each naming the version it inherits from. Rows resolve in
    /// file order, so a diff may only inherit a version an earlier row (or
    /// the base) already resolved; one that names a later version is
    /// refused rather than reordered. What is *not* required is ascending
    /// version order — the chain is whatever `inherits` describes.
    #[serde(default)]
    pub diffs: Vec<BlocksDiff>,
}

/// The `base` row of a [`BlocksCatalog`].
#[derive(Debug, Clone, Deserialize)]
pub struct BlocksBase {
    /// Version this list describes.
    pub mc_version: String,
    /// Every block id valid in that version, namespace optional.
    pub blocks: Vec<String>,
}

/// One `diffs` row of a [`BlocksCatalog`].
#[derive(Debug, Clone, Deserialize)]
pub struct BlocksDiff {
    /// Version this row describes.
    pub mc_version: String,
    /// Version this row starts from.
    pub inherits: String,
    /// Ids the inherited version does not have.
    #[serde(default)]
    pub added: Vec<String>,
    /// Ids the inherited version has and this one does not.
    #[serde(default)]
    pub removed: Vec<String>,
}

/// Validated, lookup-ready block-id tables — one sorted, fully namespaced
/// list per version.
#[derive(Debug, Clone)]
pub struct BlocksIndex {
    /// `mc_version → sorted ids`, in resolution order (base first, then
    /// each diff as it folded).
    by_version: IndexMap<String, Vec<String>>,
}

impl BlocksIndex {
    /// Index with no versions. Used when a pack omits the `blocks`
    /// component. Both lookups then answer "nothing to answer with" rather
    /// than "absent" — [`Self::ids_for`] misses every version and
    /// [`Self::declared_by_some_version`] returns `None` — and every caller
    /// reads that as "this pack cannot refute an id" rather than as "no id
    /// is valid".
    #[must_use]
    pub fn empty() -> Self {
        Self {
            by_version: IndexMap::new(),
        }
    }

    /// Fold a parsed [`BlocksCatalog`] into one table per version.
    ///
    /// # Errors
    ///
    /// Returns [`BlocksError`] for an unsupported `schema_version`, an empty
    /// or duplicate-carrying `base`, a duplicate `mc_version`, a diff whose
    /// `inherits` names a version not yet resolved, a `removed` id the
    /// parent does not have, an `added` id the parent already has, or a
    /// diff that folds to no ids at all.
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
            // Sorted-insert rather than push-then-sort: the position search
            // is the duplicate check, so a repeated id is named instead of
            // collapsing into a table one entry short of what the file
            // appears to declare.
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

    /// The sorted id list for one version, or `None` when the index does
    /// not describe that version.
    #[must_use]
    pub fn ids_for(&self, mc_version: &str) -> Option<&[String]> {
        self.by_version.get(mc_version).map(Vec::as_slice)
    }

    /// Whether any version this index describes declares `id`.
    ///
    /// The edition-wide form of [`Self::ids_for`], for callers that report
    /// across a version range instead of building for one of them. A rename
    /// inside the range leaves both spellings true for part of it, and both
    /// answer `Some(true)` here — "which versions" is a question only a
    /// pinned target can ask.
    ///
    /// `None` when the index describes no version at all: a pack shipping no
    /// `blocks` component has nothing to answer with, and reading that as
    /// "no version declares it" would make every id absent everywhere.
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

    /// Every id any version declares, version by version in resolution
    /// order.
    ///
    /// The edition-wide companion to [`Self::ids_for`], for a caller that
    /// reports across the range rather than building for one version — the
    /// same position [`Self::declared_by_some_version`] answers from.
    ///
    /// An id several versions share is yielded once per version: a
    /// distance scorer keeps the first of equal candidates, so
    /// deduplicating would cost an allocation to change nothing.
    pub fn declared_ids(&self) -> impl Iterator<Item = &str> {
        self.by_version
            .values()
            .flat_map(|ids| ids.iter().map(String::as_str))
    }

    /// Every version this index describes, in resolution order.
    pub fn versions(&self) -> impl Iterator<Item = &str> {
        self.by_version.keys().map(String::as_str)
    }

    /// Number of versions described.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_version.len()
    }

    /// `true` when the index describes no versions at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_version.is_empty()
    }
}

/// Errors raised while folding a [`BlocksCatalog`].
///
/// Every variant is a pack-author mistake rather than a user one, so each
/// names the version it was reading when it gave up.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlocksError {
    /// Catalog declared a `schema_version` this build does not understand.
    #[error(
        "unsupported registry pack blocks schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// Version the catalog declared.
        got: u32,
        /// Highest version this build supports.
        supported: u32,
    },
    /// `base.blocks` was empty. Every version folds from the base, so an
    /// empty one makes every id unknown in every version — a table that
    /// rejects the whole vanilla game is never what the author meant.
    #[error("registry pack blocks `base` for `{version}` declares no ids")]
    EmptyBase {
        /// Version the empty base claimed to describe.
        version: String,
    },
    /// A diff removed every id its parent had. The version would then
    /// declare no blocks at all and refuse `minecraft:air`, which is the
    /// same "a table that rejects the whole game" outcome
    /// [`Self::EmptyBase`] exists to catch, reached one step later.
    #[error("registry pack blocks table for `{version}` folds to no ids at all")]
    EmptyTable {
        /// Version whose fold came out empty.
        version: String,
    },
    /// One version listed the same id twice.
    #[error("registry pack blocks table for `{version}` declares `{id}` more than once")]
    DuplicateBlock {
        /// Version being folded.
        version: String,
        /// Verbatim id, after namespacing.
        id: String,
    },
    /// Two rows described the same `mc_version`.
    #[error("registry pack blocks declares version `{version}` more than once")]
    DuplicateVersion {
        /// Version declared twice.
        version: String,
    },
    /// A diff inherited from a version that has not been resolved. Either
    /// the name is a typo or the rows are ordered so the parent comes
    /// after the child.
    #[error(
        "registry pack blocks version `{version}` inherits `{inherits}`, which is not resolved before it"
    )]
    UnknownInherits {
        /// Version doing the inheriting.
        version: String,
        /// The name it named.
        inherits: String,
    },
    /// A diff removed an id its parent does not have. Silently ignoring it
    /// would leave the author believing a rename was recorded when only
    /// half of it was.
    #[error(
        "registry pack blocks version `{version}` removes `{id}`, which `{inherits}` does not declare"
    )]
    RemovedNotInParent {
        /// Version doing the removing.
        version: String,
        /// Version it inherits from.
        inherits: String,
        /// The id it tried to remove.
        id: String,
    },
    /// A diff added an id its parent already has.
    #[error(
        "registry pack blocks version `{version}` adds `{id}`, which `{inherits}` already declares"
    )]
    AddedAlreadyInParent {
        /// Version doing the adding.
        version: String,
        /// Version it inherits from.
        inherits: String,
        /// The id it tried to add.
        id: String,
    },
}

#[cfg(test)]
mod tests;
