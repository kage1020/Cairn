//! The `conductors` component of a registry pack: the block states that conduct redstone power,
//! per version, which decide where a circuit meets the struct over it (`spec/redstone`
//! "Place-and-route").

use indexmap::IndexMap;
use serde::Deserialize;

use super::held::{self, Held};
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

/// One row of [`ConductorsCatalog::rows`]: blocks that conduct in the same states at some versions,
/// or whose conduction no source settles there.
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
    /// Whether no source settles which of their states conduct.
    #[serde(default)]
    pub unsettled: bool,
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
    /// Per version, each id to its row, in the order the component names them.
    by_version: IndexMap<String, IndexMap<String, usize>>,
}

#[derive(Debug, Clone)]
struct Row {
    defaults: IndexMap<String, String>,
    /// Which states conduct, `None` for a row no source settles.
    conducts: Option<Held<bool>>,
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
                row.defaults
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .chain(row.conducts.iter().flat_map(Held::named))
                    .map(move |(key, value)| (id.as_str(), key, value))
            })
    }

    /// Whether `mc_version`'s table has a row for `id`.
    #[must_use]
    pub fn describes(&self, mc_version: &str, id: &str) -> bool {
        self.by_version
            .get(mc_version)
            .is_some_and(|ids| ids.contains_key(id))
    }

    /// The first block `mc_version`'s table looks up by state with a state its map holds no key
    /// for, as the id and the state's `property=value` pairs, where `domain` gives the values
    /// each property of a block takes there.
    pub fn unmapped(
        &self,
        mc_version: &str,
        domain: impl Fn(&str) -> Option<IndexMap<String, Vec<String>>>,
    ) -> Option<(&str, String)> {
        self.by_version
            .get(mc_version)?
            .iter()
            .find_map(|(id, &at)| {
                let lookup = self.rows[at].conducts.as_ref()?;
                let domain = domain(id)?;
                let state = lookup.unmapped(&domain)?;
                let state: Vec<String> = state
                    .into_iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                Some((id.as_str(), state.join(",")))
            })
    }

    /// Whether the canonical block `id` with `properties` conducts redstone power at
    /// `mc_version`, a block no row names conducting in no state; `None` when there is no table
    /// for the version, its row is unsettled, or its lookup maps no key for those states.
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
        row.conducts.as_ref()?.at(|key| {
            properties
                .get(key)
                .or_else(|| row.defaults.get(key))
                .map(String::as_str)
        })
    }
}

fn parse_row(row: &ConductorRow) -> Result<Row, String> {
    if row.unsettled {
        if row.conducts.is_some() || !row.defaults.is_empty() {
            return Err(
                "an unsettled row gives no `conducts` lookup or `defaults`, since no source \
                 settles which of its states conduct"
                    .to_owned(),
            );
        }
        return Ok(Row {
            defaults: IndexMap::new(),
            conducts: None,
        });
    }
    let Some(lookup) = &row.conducts else {
        if let Some(unread) = row.defaults.keys().next() {
            return Err(format!(
                "`defaults` gives `{unread}`, but the row has no `conducts` lookup to read it"
            ));
        }
        return Ok(Row {
            defaults: IndexMap::new(),
            conducts: Some(Held::Every(true)),
        });
    };
    let held = held::lookup(
        "conducts",
        &lookup.reads,
        &lookup.map,
        "a row without one conducts in every state",
        |conducts| Ok(*conducts),
    )?;
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
    if !lookup.map.values().any(|conducts| *conducts) {
        return Err(
            "the lookup maps no state to `true`; a block that conducts in no state is left out"
                .to_owned(),
        );
    }
    Ok(Row {
        defaults: row.defaults.clone(),
        conducts: Some(held),
    })
}

#[cfg(test)]
mod tests;
