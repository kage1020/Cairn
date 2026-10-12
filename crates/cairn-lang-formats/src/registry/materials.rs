//! The materials component of a registry pack: the material tokens of `spec/materials-themes`
//! "Canonical vocabulary", each matched exactly to a block id.

use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

/// The highest `materials.schema_version` this build reads.
pub const SUPPORTED_MATERIALS_SCHEMA: u32 = 1;

/// The body of `materials.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct MaterialsCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace for a `block` that names none.
    pub namespace: String,
    /// The token mappings, in declared order.
    pub entries: Vec<MaterialEntry>,
}

/// One row of [`MaterialsCatalog::entries`].
#[derive(Debug, Clone, Deserialize)]
pub struct MaterialEntry {
    /// The token without its leading `@`.
    pub token: String,
    /// The block id, namespaced by [`MaterialsCatalog::namespace`] when it names none.
    pub block: String,
    /// Per-version spellings, for an id that changes inside the edition's supported range.
    #[serde(default)]
    pub overrides: Vec<MaterialOverride>,
}

/// One `overrides` row of a [`MaterialEntry`].
#[derive(Debug, Clone, Deserialize)]
pub struct MaterialOverride {
    /// The version this spelling applies to, matched exactly.
    pub mc_version: String,
    /// The id for that version, namespaced as [`MaterialEntry::block`] is.
    pub block: String,
}

/// A validated materials catalog, ready for lookup.
#[derive(Debug, Clone)]
pub struct MaterialsIndex {
    /// In declared order, which the typo suggestion's first-seen tie-break follows.
    by_token: IndexMap<String, MaterialMapping>,
}

/// What one token resolves to, across the versions its pack supports.
#[derive(Debug, Clone)]
struct MaterialMapping {
    /// The id for every version without an override.
    default: String,
    /// `mc_version → id`, for the versions that spell it differently.
    by_version: IndexMap<String, String>,
}

impl MaterialsIndex {
    /// A catalog with no entries, for a pack without a `materials` component.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            by_token: IndexMap::new(),
        }
    }

    /// Validates `catalog` into an index.
    ///
    /// # Errors
    ///
    /// [`MaterialsError`] for an unsupported schema, a duplicate token or a duplicate override.
    pub fn from_catalog(catalog: MaterialsCatalog) -> Result<Self, MaterialsError> {
        if catalog.schema_version > SUPPORTED_MATERIALS_SCHEMA {
            return Err(MaterialsError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_MATERIALS_SCHEMA,
            });
        }
        let namespace = catalog.namespace.as_str();
        let mut by_token: IndexMap<String, MaterialMapping> =
            IndexMap::with_capacity(catalog.entries.len());
        for entry in catalog.entries {
            if by_token.contains_key(&entry.token) {
                return Err(MaterialsError::DuplicateMaterialEntry { token: entry.token });
            }
            let mut by_version: IndexMap<String, String> = IndexMap::new();
            for over in entry.overrides {
                if by_version.contains_key(&over.mc_version) {
                    return Err(MaterialsError::DuplicateMaterialOverride {
                        token: entry.token,
                        mc_version: over.mc_version,
                    });
                }
                by_version.insert(over.mc_version, namespaced(namespace, &over.block));
            }
            by_token.insert(
                entry.token,
                MaterialMapping {
                    default: namespaced(namespace, &entry.block),
                    by_version,
                },
            );
        }
        Ok(Self { by_token })
    }

    /// The default id for `token`, ignoring per-version overrides.
    #[must_use]
    pub fn lookup_id(&self, token: &str) -> Option<&str> {
        self.by_token.get(token).map(|m| m.default.as_str())
    }

    /// The id for `token` as `mc_version` spells it, or the default without an override.
    #[must_use]
    pub fn lookup_id_for(&self, token: &str, mc_version: Option<&str>) -> Option<&str> {
        let mapping = self.by_token.get(token)?;
        let overridden = mc_version.and_then(|v| mapping.by_version.get(v));
        Some(overridden.unwrap_or(&mapping.default).as_str())
    }

    /// Every `(token, mc_version)` pair that carries an override, in declared order.
    pub fn overrides(&self) -> impl Iterator<Item = (&str, &str)> {
        self.by_token.iter().flat_map(|(token, mapping)| {
            mapping
                .by_version
                .keys()
                .map(move |version| (token.as_str(), version.as_str()))
        })
    }

    /// Every `(token, mc_version, id)` the catalog resolves to, `mc_version` `None` for a default.
    pub fn mappings(&self) -> impl Iterator<Item = (&str, Option<&str>, &str)> {
        self.by_token.iter().flat_map(|(token, mapping)| {
            std::iter::once((token.as_str(), None, mapping.default.as_str())).chain(
                mapping
                    .by_version
                    .iter()
                    .map(move |(v, id)| (token.as_str(), Some(v.as_str()), id.as_str())),
            )
        })
    }

    /// Every declared token, in declared order.
    pub fn tokens(&self) -> impl Iterator<Item = &str> {
        self.by_token.keys().map(String::as_str)
    }

    /// The number of declared tokens.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_token.len()
    }

    /// Whether no tokens are declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_token.is_empty()
    }
}

/// Errors raised while validating a [`MaterialsCatalog`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaterialsError {
    /// The catalog's `schema_version` is newer than this build reads.
    #[error(
        "unsupported registry pack materials schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// Two entries declared the same token.
    #[error("registry pack materials catalog declares token `{token}` more than once")]
    DuplicateMaterialEntry {
        /// The token.
        token: String,
    },
    /// One entry declared two overrides for the same version.
    #[error(
        "registry pack materials token `{token}` declares more than one override for `{mc_version}`"
    )]
    DuplicateMaterialOverride {
        /// The token.
        token: String,
        /// The version declared twice.
        mc_version: String,
    },
}

#[cfg(test)]
mod tests;
