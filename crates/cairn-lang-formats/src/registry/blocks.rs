//! The blocks component of a registry pack: each version's block ids and the values each of their
//! state properties takes, so an unknown id fails (`spec/versioning-editions` "Fail-loud and
//! minimum-version inference"). Versions fold from a base by `inherits` and diffs
//! (`spec/versioning-editions` "Backend = data tables"), and a diff that removes an absent id, adds
//! a present one or changes an absent one is refused.

use std::fmt;
use std::sync::Arc;

use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

/// The highest `blocks.schema_version` this build reads; version 2 adds each block's states.
pub const SUPPORTED_BLOCKS_SCHEMA: u32 = 2;

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
    pub blocks: BlockEntries,
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
    pub added: BlockEntries,
    /// Ids both versions declare whose states this one replaces.
    #[serde(default)]
    pub changed: IndexMap<String, BlockStates>,
    /// Ids the inherited version has and this one drops.
    #[serde(default)]
    pub removed: Vec<String>,
}

/// Block ids as a table lists them: alone, as schema 1 writes them, or each with its states.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum BlockEntries {
    /// Ids whose states the table does not record.
    Ids(Vec<String>),
    /// Ids with the states each one takes.
    WithStates(IndexMap<String, BlockStates>),
}

impl Default for BlockEntries {
    fn default() -> Self {
        Self::Ids(Vec::new())
    }
}

impl BlockEntries {
    fn is_empty(&self) -> bool {
        match self {
            Self::Ids(ids) => ids.is_empty(),
            Self::WithStates(blocks) => blocks.is_empty(),
        }
    }

    /// Each id with its states, `None` where the table records none.
    fn entries(&self) -> Box<dyn Iterator<Item = (&str, Option<&BlockStates>)> + '_> {
        match self {
            Self::Ids(ids) => Box::new(ids.iter().map(|id| (id.as_str(), None))),
            Self::WithStates(blocks) => Box::new(
                blocks
                    .iter()
                    .map(|(id, states)| (id.as_str(), Some(states))),
            ),
        }
    }
}

/// A block's state properties, each with the values it takes in the order the game lists them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct BlockStates(IndexMap<String, Vec<StateValue>>);

impl BlockStates {
    /// The values `property` takes, or `None` when the block has no such property.
    #[must_use]
    pub fn domain(&self, property: &str) -> Option<&[StateValue]> {
        self.0.get(property).map(Vec::as_slice)
    }

    /// Each property with its values, in the table's order.
    pub fn properties(&self) -> impl Iterator<Item = (&str, &[StateValue])> {
        self.0
            .iter()
            .map(|(name, values)| (name.as_str(), values.as_slice()))
    }

    /// Whether the block has no state property.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One value of a state property: always a string on Java, a bool, int or string on Bedrock.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum StateValue {
    /// A Bedrock byte state, which holds 0 or 1.
    Bool(bool),
    /// A Bedrock int state.
    Int(i64),
    /// A string state, and every Java state.
    Str(String),
}

impl fmt::Display for StateValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::Str(value) => f.write_str(value),
        }
    }
}

/// One version's ids, sorted and namespaced, beside the states the table records for each.
#[derive(Debug, Clone, Default)]
struct VersionTable {
    ids: Vec<String>,
    states: Vec<Option<Arc<BlockStates>>>,
}

impl VersionTable {
    fn position(&self, id: &str) -> Result<usize, usize> {
        self.ids.binary_search_by(|probe| probe.as_str().cmp(id))
    }

    fn insert(&mut self, at: usize, id: String, states: Option<&BlockStates>) {
        self.ids.insert(at, id);
        self.states.insert(at, states.cloned().map(Arc::new));
    }
}

/// One sorted, namespaced block-id table per version, with each id's states where recorded.
#[derive(Debug, Clone)]
pub struct BlocksIndex {
    /// In resolution order: the base, then each diff.
    by_version: IndexMap<String, VersionTable>,
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
        let mut by_version: IndexMap<String, VersionTable> = IndexMap::new();

        let version = &catalog.base.mc_version;
        let mut base = VersionTable::default();
        for (block, states) in catalog.base.blocks.entries() {
            let id = namespaced(namespace, block);
            check_domains(version, &id, states)?;
            match base.position(&id) {
                Ok(_) => {
                    return Err(BlocksError::DuplicateBlock {
                        version: version.clone(),
                        id,
                    });
                }
                Err(at) => base.insert(at, id, states),
            }
        }
        by_version.insert(version.clone(), base);

        for diff in &catalog.diffs {
            let folded = fold_diff(namespace, diff, &by_version)?;
            by_version.insert(diff.mc_version.clone(), folded);
        }

        Ok(Self { by_version })
    }

    /// The sorted ids of `mc_version`, or `None` when the index does not describe it.
    #[must_use]
    pub fn ids_for(&self, mc_version: &str) -> Option<&[String]> {
        self.by_version
            .get(mc_version)
            .map(|table| table.ids.as_slice())
    }

    /// The states `id` takes in `mc_version`, or `None` when that version does not declare the
    /// id or the table records no states for it.
    #[must_use]
    pub fn states_of(&self, mc_version: &str, id: &str) -> Option<&BlockStates> {
        let table = self.by_version.get(mc_version)?;
        let at = table.position(id).ok()?;
        table.states[at].as_deref()
    }

    /// The first version, in resolution order, that declares `id`.
    #[must_use]
    pub fn since(&self, id: &str) -> Option<&str> {
        self.by_version
            .iter()
            .find(|(_, table)| table.position(id).is_ok())
            .map(|(version, _)| version.as_str())
    }

    /// The last version, in resolution order, that declares `id`.
    #[must_use]
    pub fn until(&self, id: &str) -> Option<&str> {
        self.by_version
            .iter()
            .rev()
            .find(|(_, table)| table.position(id).is_ok())
            .map(|(version, _)| version.as_str())
    }

    /// Whether any version declares `id`, or `None` when the index describes no version.
    #[must_use]
    pub fn declared_by_some_version(&self, id: &str) -> Option<bool> {
        if self.by_version.is_empty() {
            return None;
        }
        Some(self.since(id).is_some())
    }

    /// Every id each version declares, version by version, so a shared id repeats.
    pub fn declared_ids(&self) -> impl Iterator<Item = &str> {
        self.by_version
            .values()
            .flat_map(|table| table.ids.iter().map(String::as_str))
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

/// The table `diff` describes, folded from the version it inherits.
fn fold_diff(
    namespace: &str,
    diff: &BlocksDiff,
    by_version: &IndexMap<String, VersionTable>,
) -> Result<VersionTable, BlocksError> {
    let version = &diff.mc_version;
    if by_version.contains_key(version) {
        return Err(BlocksError::DuplicateVersion {
            version: version.clone(),
        });
    }
    let parent = by_version
        .get(&diff.inherits)
        .ok_or_else(|| BlocksError::UnknownInherits {
            version: version.clone(),
            inherits: diff.inherits.clone(),
        })?;
    let mut folded = parent.clone();
    for block in &diff.removed {
        let id = namespaced(namespace, block);
        let at = folded
            .position(&id)
            .map_err(|_| BlocksError::RemovedNotInParent {
                version: version.clone(),
                inherits: diff.inherits.clone(),
                id: id.clone(),
            })?;
        folded.ids.remove(at);
        folded.states.remove(at);
    }
    for (block, states) in &diff.changed {
        let id = namespaced(namespace, block);
        check_domains(version, &id, Some(states))?;
        let at = folded
            .position(&id)
            .map_err(|_| BlocksError::ChangedNotInParent {
                version: version.clone(),
                inherits: diff.inherits.clone(),
                id: id.clone(),
            })?;
        folded.states[at] = Some(Arc::new(states.clone()));
    }
    for (block, states) in diff.added.entries() {
        let id = namespaced(namespace, block);
        check_domains(version, &id, states)?;
        match folded.position(&id) {
            Ok(_) => {
                return Err(BlocksError::AddedAlreadyInParent {
                    version: version.clone(),
                    inherits: diff.inherits.clone(),
                    id,
                });
            }
            Err(at) => folded.insert(at, id, states),
        }
    }
    if folded.ids.is_empty() {
        return Err(BlocksError::EmptyTable {
            version: version.clone(),
        });
    }
    Ok(folded)
}

/// Refuses a property with no value, which no block of the game can be written with.
fn check_domains(version: &str, id: &str, states: Option<&BlockStates>) -> Result<(), BlocksError> {
    let Some(empty) = states
        .into_iter()
        .flat_map(BlockStates::properties)
        .find(|(_, values)| values.is_empty())
    else {
        return Ok(());
    };
    Err(BlocksError::EmptyDomain {
        version: version.to_owned(),
        id: id.to_owned(),
        property: empty.0.to_owned(),
    })
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
    /// A diff changed the states of an id its parent does not have.
    #[error(
        "registry pack blocks version `{version}` changes `{id}`, which `{inherits}` does not declare"
    )]
    ChangedNotInParent {
        /// The changing version.
        version: String,
        /// The version it inherits.
        inherits: String,
        /// The id.
        id: String,
    },
    /// A state property listed no value.
    #[error(
        "registry pack blocks table for `{version}` gives `{id}` a `{property}` property with no value"
    )]
    EmptyDomain {
        /// The version being folded.
        version: String,
        /// The id, namespaced.
        id: String,
        /// The property.
        property: String,
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
