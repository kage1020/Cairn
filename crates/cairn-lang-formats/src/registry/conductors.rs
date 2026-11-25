//! The `conductors` component of a registry pack: the block states that conduct redstone power,
//! per version, which decide where a circuit meets the struct over it (`spec/redstone`
//! "Place-and-route").

use std::collections::HashMap;

use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

/// The highest `conductors.schema_version` this build reads.
pub const SUPPORTED_CONDUCTORS_SCHEMA: u32 = 1;

/// The body of `conductors.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConductorsCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace of every id the catalog names.
    pub namespace: String,
    /// The rows, each with the versions and blocks it describes.
    pub rows: Vec<ConductorRow>,
}

/// One row of [`ConductorsCatalog::rows`]: blocks that conduct in the same states at some versions.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConductorRow {
    /// The versions the row describes.
    pub mc_versions: Vec<String>,
    /// The canonical ids of the blocks it describes.
    pub ids: Vec<String>,
    /// The default state's value of each property [`Self::conducts`] reads.
    #[serde(default)]
    pub defaults: IndexMap<String, String>,
    /// Which of their states conduct, or none when every state does.
    #[serde(default)]
    pub conducts: Option<ConductsLookup>,
}

/// Whether a [`ConductorRow`]'s blocks conduct, looked up from the values of [`Self::reads`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConductsLookup {
    /// The properties read, in order.
    pub reads: Vec<String>,
    /// Their values, comma-joined, to whether the state conducts.
    pub map: IndexMap<String, bool>,
}

/// Why a `conductors` component does not validate.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConductorsError {
    /// The component's `schema_version` is newer than this build reads.
    #[error(
        "unsupported registry pack conductors schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// The component has no rows; a pack without conductor tables names no `conductors` file.
    #[error(
        "registry pack `conductors` has no rows; a pack without conductor tables names no \
         `conductors` file"
    )]
    NoRows,
    /// A row names no block, or no version, and so describes nothing.
    #[error("registry pack `conductors` `rows[{index}]` names no {missing}")]
    EmptyRow {
        /// The row's place in `rows`, from 0.
        index: usize,
        /// `"block"` or `"version"`.
        missing: &'static str,
    },
    /// Two rows describe one block at one version.
    #[error("registry pack `conductors` describes `{id}` twice at `{mc_version}`")]
    DuplicateId {
        /// The version.
        mc_version: String,
        /// The id.
        id: String,
    },
    /// A row's lookup or defaults are malformed.
    #[error("registry pack `conductors` row of `{id}`: {problem}")]
    MalformedRow {
        /// The row's first id.
        id: String,
        /// What is wrong.
        problem: String,
    },
}

/// The validated conductor tables of a pack, empty for a pack without the component.
#[derive(Debug, Clone, Default)]
pub struct ConductorIndex {
    rows: Vec<Row>,
    /// Per version, each id to its row.
    by_version: IndexMap<String, HashMap<String, usize>>,
}

#[derive(Debug, Clone)]
struct Row {
    defaults: IndexMap<String, String>,
    /// `None` when every state conducts.
    lookup: Option<Lookup>,
}

#[derive(Debug, Clone)]
struct Lookup {
    reads: Vec<String>,
    map: HashMap<String, bool>,
}

impl ConductorIndex {
    /// An index with no table, for a pack without the component.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Validate `catalog` into an index.
    ///
    /// # Errors
    ///
    /// [`ConductorsError`] when the schema is too new, the catalog has no rows, a row is empty or
    /// malformed, or two rows describe one block at one version.
    pub fn from_catalog(catalog: ConductorsCatalog) -> Result<Self, ConductorsError> {
        if catalog.schema_version > SUPPORTED_CONDUCTORS_SCHEMA {
            return Err(ConductorsError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_CONDUCTORS_SCHEMA,
            });
        }
        if catalog.rows.is_empty() {
            return Err(ConductorsError::NoRows);
        }
        let mut index = Self::default();
        for (at, row) in catalog.rows.into_iter().enumerate() {
            let Some(first) = row.ids.first() else {
                return Err(ConductorsError::EmptyRow {
                    index: at,
                    missing: "block",
                });
            };
            if row.mc_versions.is_empty() {
                return Err(ConductorsError::EmptyRow {
                    index: at,
                    missing: "version",
                });
            }
            index.rows.push(
                parse_row(&row).map_err(|problem| ConductorsError::MalformedRow {
                    id: first.clone(),
                    problem,
                })?,
            );
            for version in &row.mc_versions {
                let ids = index.by_version.entry(version.clone()).or_default();
                for id in &row.ids {
                    let id = namespaced(&catalog.namespace, id);
                    if ids.insert(id.clone(), at).is_some() {
                        return Err(ConductorsError::DuplicateId {
                            mc_version: version.clone(),
                            id,
                        });
                    }
                }
            }
        }
        Ok(index)
    }

    /// Whether the pack carries no conductor table.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_version.is_empty()
    }

    /// The versions the tables describe, in the order the component first names them.
    pub fn versions(&self) -> impl Iterator<Item = &str> {
        self.by_version.keys().map(String::as_str)
    }

    /// The ids `mc_version`'s table describes, or `None` when there is no table for it.
    pub fn ids_at(&self, mc_version: &str) -> Option<impl Iterator<Item = &str>> {
        Some(self.by_version.get(mc_version)?.keys().map(String::as_str))
    }

    /// Each state `mc_version`'s table names of a block, as `(id, property, value)`: every
    /// default its row gives and every value a lookup's key holds.
    pub fn named_states(&self, mc_version: &str) -> impl Iterator<Item = (&str, &str, &str)> {
        self.by_version
            .get(mc_version)
            .into_iter()
            .flatten()
            .flat_map(|(id, &at)| {
                let row = &self.rows[at];
                let keyed = row.lookup.iter().flat_map(|lookup| {
                    lookup
                        .map
                        .keys()
                        .flat_map(|key| lookup.reads.iter().map(String::as_str).zip(key.split(',')))
                });
                row.defaults
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .chain(keyed)
                    .map(move |(key, value)| (id.as_str(), key, value))
            })
    }

    /// Whether the canonical block `id` with `properties` conducts redstone power at
    /// `mc_version`, or `None` when there is no table for the version.
    #[must_use]
    pub fn conducts(
        &self,
        mc_version: &str,
        id: &str,
        properties: &IndexMap<String, String>,
    ) -> Option<bool> {
        let table = self.by_version.get(mc_version)?;
        let Some(&at) = table.get(id) else {
            return Some(false);
        };
        let row = &self.rows[at];
        let Some(lookup) = &row.lookup else {
            return Some(true);
        };
        let values: Option<Vec<&str>> = lookup
            .reads
            .iter()
            .map(|key| {
                properties
                    .get(key)
                    .or_else(|| row.defaults.get(key))
                    .map(String::as_str)
            })
            .collect();
        Some(
            values
                .and_then(|values| lookup.map.get(&values.join(",")).copied())
                .unwrap_or(false),
        )
    }
}

fn parse_row(row: &ConductorRow) -> Result<Row, String> {
    let Some(lookup) = &row.conducts else {
        if let Some(unread) = row.defaults.keys().next() {
            return Err(format!(
                "`defaults` gives `{unread}`, but the row has no `conducts` lookup to read it"
            ));
        }
        return Ok(Row {
            defaults: IndexMap::new(),
            lookup: None,
        });
    };
    if lookup.reads.is_empty() || lookup.map.is_empty() {
        return Err(
            "the `conducts` lookup reads no property or maps no value; a row without one \
             conducts in every state"
                .to_owned(),
        );
    }
    if let Some(missing) = lookup
        .reads
        .iter()
        .find(|key| !row.defaults.contains_key(*key))
    {
        return Err(format!(
            "the lookup reads `{missing}`, which `defaults` does not give"
        ));
    }
    if let Some(unread) = row.defaults.keys().find(|key| !lookup.reads.contains(key)) {
        return Err(format!(
            "`defaults` gives `{unread}`, which the lookup does not read"
        ));
    }
    if let Some(key) = lookup
        .map
        .keys()
        .find(|key| key.split(',').count() != lookup.reads.len())
    {
        return Err(format!(
            "the `conducts` key `{key}` holds no value for each of {}",
            lookup.reads.join(", ")
        ));
    }
    if !lookup.map.values().any(|conducts| *conducts) {
        return Err(
            "the lookup maps no state to `true`; a block that conducts in no state is left out"
                .to_owned(),
        );
    }
    Ok(Row {
        defaults: row.defaults.clone(),
        lookup: Some(Lookup {
            reads: lookup.reads.clone(),
            map: lookup
                .map
                .iter()
                .map(|(key, conducts)| (key.clone(), *conducts))
                .collect(),
        }),
    })
}

#[cfg(test)]
mod tests;
