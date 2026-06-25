//! Abstract material catalog component of a [`crate::registry::RegistryPack`].
//!
//! Maps spec §7.2 abstract material tokens (`@floor.wood.broadleaf`) onto the
//! canonical Minecraft block ids the block-array IR stores. The catalog
//! itself is a flat list of `(token, block)` pairs; the surrounding tree
//! structure suggested by the token names is *not* enforced by this layer
//! because resolution is exact-match — the JSON shape stays cheap to
//! validate and the surrounding tree can grow into the manifest later
//! without churning every consumer now.
//!
//! The block-array lowering pass consumes [`MaterialsIndex`] indirectly
//! through `cairn_lang_core::block_array::AbstractMaterialResolver`. That
//! trait is satisfied here so `core` never depends on `formats`.

use indexmap::IndexMap;
use serde::Deserialize;

use cairn_lang_core::block_array::{AbstractMaterialResolver, BlockState};

/// Highest `materials.schema_version` this Cairn build understands.
pub const SUPPORTED_MATERIALS_SCHEMA: u32 = 1;

/// On-disk `materials.json` body.
///
/// Field order matters only for the `parses_minimal_catalog` test; serde
/// reads by name so the on-disk JSON can use any key order.
#[derive(Debug, Clone, Deserialize)]
pub struct MaterialsCatalog {
    /// Schema version of the catalog itself.
    pub schema_version: u32,
    /// Default Minecraft id namespace for entries whose `block` value does
    /// not carry one. A future modded pack can override per-entry by writing
    /// `"create:cogwheel"` instead of `"oak_planks"`.
    pub namespace: String,
    /// Token → block mappings in declared order.
    pub entries: Vec<MaterialEntry>,
}

/// One row of [`MaterialsCatalog::entries`].
#[derive(Debug, Clone, Deserialize)]
pub struct MaterialEntry {
    /// Inner body of the `@TOKEN` literal — no leading `@`.
    pub token: String,
    /// Resolved Minecraft block id. May omit the namespace, in which case
    /// the catalog's [`MaterialsCatalog::namespace`] is prepended.
    pub block: String,
}

/// Validated, lookup-ready abstract-material catalog.
#[derive(Debug, Clone)]
pub struct MaterialsIndex {
    /// `token → fully-namespaced id`, in insertion order. `IndexMap` is here
    /// so the suggestion-pool order matches the declared order — the
    /// `nearest_match` tie-break is "first-seen wins", and the on-disk order
    /// is the most readable bias for that.
    by_token: IndexMap<String, String>,
}

impl MaterialsIndex {
    /// Catalog with no entries. Used when a registry pack omits the
    /// `materials` component (older packs, or a `--registry-pack` that has
    /// not been ported to PR2's schema yet). Lookups always miss.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            by_token: IndexMap::new(),
        }
    }

    /// Build a [`MaterialsIndex`] from a parsed [`MaterialsCatalog`].
    ///
    /// # Errors
    ///
    /// Returns [`MaterialsError`] when the catalog declares an unsupported
    /// `schema_version` or contains a duplicate `token`. Both are pack-author
    /// bugs that can silently corrupt lookups, so they fail at load time.
    pub fn from_catalog(catalog: MaterialsCatalog) -> Result<Self, MaterialsError> {
        if catalog.schema_version > SUPPORTED_MATERIALS_SCHEMA {
            return Err(MaterialsError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_MATERIALS_SCHEMA,
            });
        }
        let mut by_token: IndexMap<String, String> = IndexMap::with_capacity(catalog.entries.len());
        for entry in catalog.entries {
            let id = if entry.block.contains(':') {
                entry.block
            } else {
                format!("{}:{}", catalog.namespace, entry.block)
            };
            if by_token.insert(entry.token.clone(), id).is_some() {
                return Err(MaterialsError::DuplicateMaterialEntry { token: entry.token });
            }
        }
        Ok(Self { by_token })
    }

    /// Look up an abstract token. Returns the catalog's resolved id (already
    /// namespaced) when the token is declared, otherwise `None`.
    #[must_use]
    pub fn lookup_id(&self, token: &str) -> Option<&str> {
        self.by_token.get(token).map(String::as_str)
    }

    /// Iterate every declared token in insertion order.
    pub fn known_tokens(&self) -> impl Iterator<Item = &str> {
        self.by_token.keys().map(String::as_str)
    }

    /// Number of declared entries. Useful for diagnostics that want to say
    /// "the catalog declares N tokens but none match".
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_token.len()
    }

    /// `true` when no tokens are declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_token.is_empty()
    }
}

impl AbstractMaterialResolver for MaterialsIndex {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        self.lookup_id(token).map(BlockState::bare)
    }

    fn known_tokens(&self) -> Vec<String> {
        self.by_token.keys().cloned().collect()
    }
}

/// Errors raised while validating a [`MaterialsCatalog`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaterialsError {
    /// Catalog declared a `schema_version` this build does not understand.
    #[error(
        "unsupported registry pack materials schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// Version the catalog declared.
        got: u32,
        /// Highest version this build supports.
        supported: u32,
    },
    /// Two `entries` rows declared the same `token`. Silent overwrite would
    /// pin lookups on the *last* row, which is invisible to the pack author
    /// and almost always a copy-paste bug.
    #[error("registry pack materials catalog declares token `{token}` more than once")]
    DuplicateMaterialEntry {
        /// Verbatim token text from the catalog.
        token: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Result<MaterialsIndex, MaterialsError> {
        let catalog: MaterialsCatalog =
            serde_json::from_str(src).expect("test catalog parses as JSON");
        MaterialsIndex::from_catalog(catalog)
    }

    #[test]
    fn parses_minimal_catalog() {
        let src = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "token": "floor.wood.broadleaf", "block": "oak_planks" }
            ]
        }"#;
        let index = parse(src).expect("catalog");
        assert_eq!(index.len(), 1);
        assert_eq!(
            index.lookup_id("floor.wood.broadleaf"),
            Some("minecraft:oak_planks"),
        );
    }

    #[test]
    fn lookup_resolves_known_token() {
        let src = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "token": "wall.stone.cobble", "block": "cobblestone" }
            ]
        }"#;
        let index = parse(src).unwrap();
        let state = AbstractMaterialResolver::lookup(&index, "wall.stone.cobble").unwrap();
        assert_eq!(state.id, "minecraft:cobblestone");
        assert!(state.properties.is_empty());
    }

    #[test]
    fn lookup_unknown_returns_none() {
        let src = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "token": "wood.dark", "block": "dark_oak_planks" }
            ]
        }"#;
        let index = parse(src).unwrap();
        assert!(index.lookup_id("totally.unknown.token").is_none());
    }

    #[test]
    fn explicit_namespace_on_entry_overrides_catalog_default() {
        let src = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "token": "tech.cog", "block": "create:cogwheel" }
            ]
        }"#;
        let index = parse(src).unwrap();
        assert_eq!(index.lookup_id("tech.cog"), Some("create:cogwheel"));
    }

    #[test]
    fn duplicate_token_rejected_at_load_time() {
        let src = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "token": "floor.wood.broadleaf", "block": "oak_planks" },
                { "token": "floor.wood.broadleaf", "block": "spruce_planks" }
            ]
        }"#;
        let err = parse(src).unwrap_err();
        assert_eq!(
            err,
            MaterialsError::DuplicateMaterialEntry {
                token: "floor.wood.broadleaf".into(),
            },
        );
    }

    #[test]
    fn unsupported_schema_version_is_error() {
        let src = r#"{
            "schema_version": 999,
            "namespace": "minecraft",
            "entries": []
        }"#;
        let err = parse(src).unwrap_err();
        assert!(matches!(
            err,
            MaterialsError::UnsupportedSchemaVersion {
                got: 999,
                supported: SUPPORTED_MATERIALS_SCHEMA,
            }
        ));
    }

    #[test]
    fn empty_catalog_loads_with_zero_entries() {
        let src = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": []
        }"#;
        let index = parse(src).unwrap();
        assert!(index.is_empty());
        assert!(index.lookup_id("anything").is_none());
    }

    #[test]
    fn known_tokens_preserves_insertion_order() {
        let src = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "token": "a.b.c", "block": "stone" },
                { "token": "x.y.z", "block": "dirt" }
            ]
        }"#;
        let index = parse(src).unwrap();
        let order: Vec<&str> = index.known_tokens().collect();
        assert_eq!(order, vec!["a.b.c", "x.y.z"]);
    }
}
