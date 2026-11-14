//! The meanings component of a registry pack: each canonical token whose block split into
//! several, with the meaning token that picks one of them, so `@cauldron[fluid=water]` names
//! `water_cauldron` (`spec/materials-themes` "Mappings across version and edition").

use cairn_lang_core::block_array::BlockState;
use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

/// The highest `meanings.schema_version` this build reads.
pub const SUPPORTED_MEANINGS_SCHEMA: u32 = 1;

/// The body of `meanings.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeaningsCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace for an id that names none.
    pub namespace: String,
    /// The tokens, one row each.
    pub tokens: Vec<MeaningRow>,
}

/// One `tokens` row of a [`MeaningsCatalog`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeaningRow {
    /// The canonical token whose block split, without the `@`.
    pub token: String,
    /// The meaning token: a property the block does not have.
    pub meaning: String,
    /// Each value of the meaning token, with the block it names.
    pub blocks: IndexMap<String, String>,
}

/// Validated meaning rows, ready for lookup.
#[derive(Debug, Clone, Default)]
pub struct MeaningIndex {
    /// Each token's id, namespaced, with its meaning token and the namespaced block per value.
    rows: IndexMap<String, (String, IndexMap<String, String>)>,
}

impl MeaningIndex {
    /// An index with no rows, for a pack without a `meanings` component.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Validates `catalog` into an index.
    ///
    /// # Errors
    ///
    /// [`MeaningsError`] for the first fault in the catalog.
    pub fn from_catalog(catalog: MeaningsCatalog) -> Result<Self, MeaningsError> {
        if catalog.schema_version > SUPPORTED_MEANINGS_SCHEMA {
            return Err(MeaningsError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_MEANINGS_SCHEMA,
            });
        }
        if catalog.tokens.is_empty() {
            return Err(MeaningsError::NoTokens);
        }
        let namespace = catalog.namespace.as_str();
        let mut rows = IndexMap::new();
        for row in catalog.tokens {
            let token = namespaced(namespace, &row.token);
            if row.blocks.is_empty() {
                return Err(MeaningsError::NoBlocks { token });
            }
            let blocks = row
                .blocks
                .iter()
                .map(|(value, block)| (value.clone(), namespaced(namespace, block)))
                .collect();
            if rows.insert(token.clone(), (row.meaning, blocks)).is_some() {
                return Err(MeaningsError::DuplicateToken { token });
            }
        }
        Ok(Self { rows })
    }

    /// The block `state` names through its token's meaning token, the meaning token dropped from
    /// its states; `None` when its id has no row or the value names no block.
    #[must_use]
    pub fn meant(&self, state: &BlockState) -> Option<BlockState> {
        let (meaning, blocks) = self.rows.get(&state.id)?;
        let block = blocks.get(state.properties.get(meaning)?)?;
        let mut properties = state.properties.clone();
        properties.shift_remove(meaning);
        Some(BlockState {
            id: block.clone(),
            properties,
        })
    }

    /// The meaning token of `id` with the values it takes, or `None` when `id` has no row.
    #[must_use]
    pub fn meaning_of(&self, id: &str) -> Option<(&str, Vec<String>)> {
        let (meaning, blocks) = self.rows.get(id)?;
        Some((meaning.as_str(), blocks.keys().cloned().collect()))
    }

    /// Every row as its token's id, its meaning token and the blocks it names, in declared order.
    pub fn rows(&self) -> impl Iterator<Item = (&str, &str, impl Iterator<Item = &str>)> {
        self.rows.iter().map(|(token, (meaning, blocks))| {
            (
                token.as_str(),
                meaning.as_str(),
                blocks.values().map(String::as_str),
            )
        })
    }
}

/// A fault in a [`MeaningsCatalog`], always the pack author's.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum MeaningsError {
    /// The catalog's `schema_version` is newer than this build reads.
    #[error(
        "unsupported registry pack meanings schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// The catalog declared no tokens; a pack without meanings leaves the component out.
    #[error("registry pack meanings declares no tokens")]
    NoTokens,
    /// A row named no block.
    #[error("registry pack meanings token `{token}` names no block")]
    NoBlocks {
        /// The token, namespaced.
        token: String,
    },
    /// Two rows named the same token.
    #[error("registry pack meanings declares `{token}` more than once")]
    DuplicateToken {
        /// The token, namespaced.
        token: String,
    },
}

#[cfg(test)]
mod tests;
