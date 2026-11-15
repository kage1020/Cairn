//! The blocks component of a registry pack: each version's block ids and the values each of their
//! state properties takes, so an unknown id fails (`spec/versioning-editions` "Fail-loud and
//! minimum-version inference"). Versions fold from a base by `inherits` and diffs
//! (`spec/versioning-editions` "Backend = data tables"), each diff from the version before it, and
//! a diff that removes an absent id, adds a present one or changes an absent one is refused.

mod table;

use std::collections::HashSet;
use std::fmt;
use std::marker::PhantomData;

use cairn_lang_core::Edition;
use indexmap::IndexMap;
use serde::de::{self, MapAccess, SeqAccess, Unexpected, Visitor};
use serde::{Deserialize, Deserializer};

use self::table::VersionTable;
use super::namespaced;

/// The highest `blocks.schema_version` this build reads; version 2 adds each block's states.
pub const SUPPORTED_BLOCKS_SCHEMA: u32 = 2;

/// The body of `blocks.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlocksCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace for an id that names none.
    pub namespace: String,
    /// The oldest version's full id list, which every diff folds from.
    pub base: BlocksBase,
    /// The diffs, in file order; each inherits the version listed just before it.
    #[serde(default)]
    pub diffs: Vec<BlocksDiff>,
}

/// The `base` row of a [`BlocksCatalog`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlocksBase {
    /// The version this list describes.
    pub mc_version: String,
    /// Every id valid in that version.
    pub blocks: BlockEntries,
}

/// One `diffs` row of a [`BlocksCatalog`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlocksDiff {
    /// The version this row describes.
    pub mc_version: String,
    /// The version it folds from.
    pub inherits: String,
    /// Ids the inherited version lacks.
    #[serde(default)]
    pub added: BlockEntries,
    /// Ids both versions declare whose states this one replaces.
    #[serde(default, deserialize_with = "unique_blocks")]
    pub changed: IndexMap<String, BlockStates>,
    /// Ids the inherited version has and this one drops.
    #[serde(default)]
    pub removed: Vec<String>,
}

/// Block ids as a table lists them: alone, as schema 1 writes them, or each with its states, as
/// schema 2 does.
#[derive(Debug, Clone)]
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

impl<'de> Deserialize<'de> for BlockEntries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries;

        impl<'de> Visitor<'de> for Entries {
            type Value = BlockEntries;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a list of block ids, or an object giving each block id its states")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut ids = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(id) = seq.next_element()? {
                    ids.push(id);
                }
                Ok(BlockEntries::Ids(ids))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                UniqueKeys::new("block")
                    .visit_map(map)
                    .map(BlockEntries::WithStates)
            }
        }

        deserializer.deserialize_any(Entries)
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockStates(IndexMap<String, Vec<StateValue>>);

impl<'de> Deserialize<'de> for BlockStates {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer
            .deserialize_map(UniqueKeys::new("state property"))
            .map(Self)
    }
}

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateValue {
    /// A Bedrock byte state, which holds 0 or 1.
    Bool(bool),
    /// A Bedrock int state.
    Int(i64),
    /// A string state, and every Java state.
    Str(String),
}

impl StateValue {
    /// Whether `self` and `other` are the same kind of value, whatever each holds.
    fn same_kind(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl<'de> Deserialize<'de> for StateValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Value;

        impl Visitor<'_> for Value {
            type Value = StateValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a state value: a bool, an integer or a string")
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(StateValue::Bool(value))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(StateValue::Int(value))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                i64::try_from(value)
                    .map(StateValue::Int)
                    .map_err(|_| E::invalid_value(Unexpected::Unsigned(value), &self))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(StateValue::Str(value.to_owned()))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(StateValue::Str(value))
            }
        }

        deserializer.deserialize_any(Value)
    }
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

/// An object read into an [`IndexMap`], refusing a key it already read: serde's own map keeps the
/// last of two, and the first would vanish without a word.
struct UniqueKeys<V> {
    /// What a key names, for the refusal.
    what: &'static str,
    value: PhantomData<V>,
}

impl<V> UniqueKeys<V> {
    fn new(what: &'static str) -> Self {
        Self {
            what,
            value: PhantomData,
        }
    }
}

impl<'de, V: Deserialize<'de>> Visitor<'de> for UniqueKeys<V> {
    type Value = IndexMap<String, V>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "an object keyed by {}", self.what)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut read = IndexMap::with_capacity(map.size_hint().unwrap_or(0));
        while let Some(key) = map.next_key::<String>()? {
            if read.contains_key(&key) {
                return Err(de::Error::custom(format_args!(
                    "{} `{key}` is listed more than once",
                    self.what
                )));
            }
            let value = map.next_value()?;
            read.insert(key, value);
        }
        Ok(read)
    }
}

fn unique_blocks<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<IndexMap<String, BlockStates>, D::Error> {
    deserializer.deserialize_map(UniqueKeys::new("block"))
}

/// One sorted, namespaced block-id table per version, with each id's states where recorded.
#[derive(Debug, Clone)]
pub struct BlocksIndex {
    /// In the order the catalog lists them, each version folded from the one before it.
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

    /// Folds `catalog`, a table of `edition`'s blocks, into one table per version.
    ///
    /// # Errors
    ///
    /// [`BlocksError`] for the first fault in the catalog.
    pub fn from_catalog(catalog: BlocksCatalog, edition: Edition) -> Result<Self, BlocksError> {
        let schema = catalog.schema_version;
        if schema > SUPPORTED_BLOCKS_SCHEMA {
            return Err(BlocksError::UnsupportedSchemaVersion {
                got: schema,
                supported: SUPPORTED_BLOCKS_SCHEMA,
            });
        }
        if catalog.base.blocks.is_empty() {
            return Err(BlocksError::EmptyBase {
                version: catalog.base.mc_version,
            });
        }
        let rules = Rules {
            namespace: &catalog.namespace,
            schema,
            edition,
        };

        let version = &catalog.base.mc_version;
        rules.check_shape(version, "blocks", &catalog.base.blocks)?;
        let mut base = VersionTable::default();
        for (block, states) in catalog.base.blocks.entries() {
            let id = namespaced(rules.namespace, block);
            rules.check_domains(version, &id, states)?;
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
        let mut by_version: IndexMap<String, VersionTable> = IndexMap::new();
        by_version.insert(version.clone(), base);

        for diff in &catalog.diffs {
            let folded = rules.fold_diff(diff, &by_version)?;
            by_version.insert(diff.mc_version.clone(), folded);
        }

        Ok(Self { by_version })
    }

    /// The sorted ids of `mc_version`, or `None` when the index does not describe it.
    #[must_use]
    pub fn ids_for(&self, mc_version: &str) -> Option<&[String]> {
        self.by_version.get(mc_version).map(VersionTable::ids)
    }

    /// The states `id` takes in `mc_version`, or `None` when that version does not declare the
    /// id or, in a schema 1 table, records no states for it.
    #[must_use]
    pub fn states_of(&self, mc_version: &str, id: &str) -> Option<&BlockStates> {
        let table = self.by_version.get(mc_version)?;
        let at = table.position(id).ok()?;
        table.states(at)
    }

    /// The first version, in the catalog's order, that declares `id`. A version between this one
    /// and [`Self::until`] may still lack it, since a diff can remove an id a later one adds back.
    #[must_use]
    pub fn since(&self, id: &str) -> Option<&str> {
        self.by_version
            .iter()
            .find(|(_, table)| table.position(id).is_ok())
            .map(|(version, _)| version.as_str())
    }

    /// The last version, in the catalog's order, that declares `id`; see [`Self::since`].
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
            .flat_map(|table| table.ids().iter().map(String::as_str))
    }

    /// Every version this index describes, in the catalog's order.
    pub fn versions(&self) -> impl Iterator<Item = &str> {
        self.by_version.keys().map(String::as_str)
    }

    /// Every version this index describes, in the catalog's order, with its sorted ids.
    pub fn ids_by_version(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.by_version
            .iter()
            .map(|(version, table)| (version.as_str(), table.ids()))
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

/// What every table of one catalog is held to.
struct Rules<'a> {
    namespace: &'a str,
    schema: u32,
    edition: Edition,
}

impl Rules<'_> {
    /// The table `diff` describes, folded from the version listed just before it.
    fn fold_diff(
        &self,
        diff: &BlocksDiff,
        by_version: &IndexMap<String, VersionTable>,
    ) -> Result<VersionTable, BlocksError> {
        let version = &diff.mc_version;
        if by_version.contains_key(version) {
            return Err(BlocksError::DuplicateVersion {
                version: version.clone(),
            });
        }
        let (previous, parent) = by_version
            .last()
            .expect("the base is folded before any diff");
        if diff.inherits != *previous {
            return Err(if by_version.contains_key(&diff.inherits) {
                BlocksError::InheritsPastPrevious {
                    version: version.clone(),
                    inherits: diff.inherits.clone(),
                    previous: previous.clone(),
                }
            } else {
                BlocksError::UnknownInherits {
                    version: version.clone(),
                    inherits: diff.inherits.clone(),
                }
            });
        }
        self.check_shape(version, "added", &diff.added)?;
        if self.schema < 2 && !diff.changed.is_empty() {
            return Err(BlocksError::StatesBeforeSchema2 {
                schema_version: self.schema,
                version: version.clone(),
                field: "changed",
            });
        }

        let mut folded = parent.clone();
        let mut removed = HashSet::new();
        for block in &diff.removed {
            let id = namespaced(self.namespace, block);
            if !removed.insert(id.clone()) {
                return Err(BlocksError::RemovedTwice {
                    version: version.clone(),
                    id,
                });
            }
            let at = folded
                .position(&id)
                .map_err(|_| BlocksError::RemovedNotInParent {
                    version: version.clone(),
                    inherits: diff.inherits.clone(),
                    id: id.clone(),
                })?;
            folded.remove(at);
        }
        for (block, states) in &diff.changed {
            let id = namespaced(self.namespace, block);
            if removed.contains(&id) {
                return Err(BlocksError::RemovedAndChanged {
                    version: version.clone(),
                    id,
                });
            }
            self.check_domains(version, &id, Some(states))?;
            let at = folded
                .position(&id)
                .map_err(|_| BlocksError::ChangedNotInParent {
                    version: version.clone(),
                    inherits: diff.inherits.clone(),
                    id: id.clone(),
                })?;
            folded.replace_states(at, states);
        }
        let mut added = HashSet::new();
        for (block, states) in diff.added.entries() {
            let id = namespaced(self.namespace, block);
            if !added.insert(id.clone()) {
                return Err(BlocksError::DuplicateBlock {
                    version: version.clone(),
                    id,
                });
            }
            self.check_domains(version, &id, states)?;
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
        if folded.is_empty() {
            return Err(BlocksError::EmptyTable {
                version: version.clone(),
            });
        }
        Ok(folded)
    }

    /// Refuses bare ids where schema 2 records states, and states where schema 1 records none.
    fn check_shape(
        &self,
        version: &str,
        field: &'static str,
        entries: &BlockEntries,
    ) -> Result<(), BlocksError> {
        if entries.is_empty() {
            return Ok(());
        }
        match entries {
            BlockEntries::Ids(_) if self.schema >= 2 => Err(BlocksError::IdsWithoutStates {
                schema_version: self.schema,
                version: version.to_owned(),
                field,
            }),
            BlockEntries::WithStates(_) if self.schema < 2 => {
                Err(BlocksError::StatesBeforeSchema2 {
                    schema_version: self.schema,
                    version: version.to_owned(),
                    field,
                })
            }
            BlockEntries::Ids(_) | BlockEntries::WithStates(_) => Ok(()),
        }
    }

    /// Refuses a property no block of the game can be written with: one with no value, on Java a
    /// value that is not a string, values of mixed kinds, or a value listed twice.
    fn check_domains(
        &self,
        version: &str,
        id: &str,
        states: Option<&BlockStates>,
    ) -> Result<(), BlocksError> {
        for (property, values) in states.into_iter().flat_map(BlockStates::properties) {
            let (version, id, property) = (version.to_owned(), id.to_owned(), property.to_owned());
            let Some(first) = values.first() else {
                return Err(BlocksError::EmptyDomain {
                    version,
                    id,
                    property,
                });
            };
            if self.edition == Edition::Java
                && let Some(value) = values
                    .iter()
                    .find(|value| !matches!(value, StateValue::Str(_)))
            {
                return Err(BlocksError::JavaValueNotString {
                    version,
                    id,
                    property,
                    value: value.clone(),
                });
            }
            if let Some(other) = values.iter().find(|value| !value.same_kind(first)) {
                return Err(BlocksError::MixedDomain {
                    version,
                    id,
                    property,
                    first: first.clone(),
                    other: other.clone(),
                });
            }
            if let Some(value) = values
                .iter()
                .enumerate()
                .find_map(|(at, value)| values[..at].contains(value).then_some(value))
            {
                return Err(BlocksError::RepeatedValue {
                    version,
                    id,
                    property,
                    value: value.clone(),
                });
            }
        }
        Ok(())
    }
}

/// `value`'s kind with its article, as a refusal names it.
fn kind_of(value: &StateValue) -> &'static str {
    match value {
        StateValue::Bool(_) => "the boolean",
        StateValue::Int(_) => "the integer",
        StateValue::Str(_) => "the string",
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
    /// A schema 2 table listed bare ids where it records each id's states.
    #[error(
        "registry pack blocks schema_version {schema_version} writes `{field}` of `{version}` as an object giving each id its states, not a list of bare ids"
    )]
    IdsWithoutStates {
        /// The catalog's schema version.
        schema_version: u32,
        /// The version whose table it is.
        version: String,
        /// `blocks` for the base, or `added`.
        field: &'static str,
    },
    /// A table older than schema 2 gave states, which only schema 2 records.
    #[error(
        "registry pack blocks schema_version {schema_version} gives `{field}` of `{version}` states, which only schema_version 2 records"
    )]
    StatesBeforeSchema2 {
        /// The catalog's schema version.
        schema_version: u32,
        /// The version whose table it is.
        version: String,
        /// `blocks` for the base, `added` or `changed`.
        field: &'static str,
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
    /// A diff inherits an older version than the one listed just before it.
    #[error(
        "registry pack blocks version `{version}` inherits `{inherits}`, but the version listed before it is `{previous}`; each version inherits the one before it"
    )]
    InheritsPastPrevious {
        /// The inheriting version.
        version: String,
        /// The version it named.
        inherits: String,
        /// The version listed just before it.
        previous: String,
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
    /// A diff listed the same id twice in `removed`.
    #[error("registry pack blocks version `{version}` removes `{id}` more than once")]
    RemovedTwice {
        /// The removing version.
        version: String,
        /// The id, namespaced.
        id: String,
    },
    /// A diff both removed an id and changed its states.
    #[error(
        "registry pack blocks version `{version}` both removes `{id}` and changes its states; a removed id has no states to change"
    )]
    RemovedAndChanged {
        /// The diff's version.
        version: String,
        /// The id, namespaced.
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
    /// A Java state property listed a value that is not a string.
    #[error(
        "registry pack blocks table for `{version}` gives `{id}`'s `{property}` {} `{value}`, and every Java state value is a string",
        kind_of(value)
    )]
    JavaValueNotString {
        /// The version being folded.
        version: String,
        /// The id, namespaced.
        id: String,
        /// The property.
        property: String,
        /// The value.
        value: StateValue,
    },
    /// A state property's values were not all one kind.
    #[error(
        "registry pack blocks table for `{version}` mixes {} `{first}` with {} `{other}` in `{id}`'s `{property}`; a property's values are all one kind",
        kind_of(first),
        kind_of(other)
    )]
    MixedDomain {
        /// The version being folded.
        version: String,
        /// The id, namespaced.
        id: String,
        /// The property.
        property: String,
        /// The property's first value.
        first: StateValue,
        /// Its first value of another kind.
        other: StateValue,
    },
    /// A state property listed one value twice.
    #[error(
        "registry pack blocks table for `{version}` lists `{value}` more than once in `{id}`'s `{property}`"
    )]
    RepeatedValue {
        /// The version being folded.
        version: String,
        /// The id, namespaced.
        id: String,
        /// The property.
        property: String,
        /// The repeated value.
        value: StateValue,
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
