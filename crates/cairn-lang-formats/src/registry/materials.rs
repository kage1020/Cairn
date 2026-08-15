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
//! The block-array lowering pass reaches this catalog through
//! [`crate::registry::PackView`], which pairs it with the block-id table
//! for the version a compile pinned. The trait that pairing satisfies
//! lives in `cairn_lang_core::block_array`, so `core` never depends on
//! `formats`.

use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

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
    /// Versions that spell the same material differently.
    ///
    /// A material token is edition-neutral, but an id is not always stable
    /// across one edition's own supported range: Bedrock 1.21.0 spells
    /// stone bricks `stonebrick` and 1.21.40 spells it `stone_bricks`, so
    /// a single `block` for `floor.stone.smooth` is wrong at one end of
    /// the range whichever spelling it picks.
    #[serde(default)]
    pub overrides: Vec<MaterialOverride>,
}

/// One `overrides` row of a [`MaterialEntry`].
#[derive(Debug, Clone, Deserialize)]
pub struct MaterialOverride {
    /// Version this spelling applies to. Exact match — a pack lists one row
    /// per affected version rather than a range, because the versions it
    /// supports are a short closed list and a range would need an ordering
    /// the catalog does not otherwise depend on.
    pub mc_version: String,
    /// The id for that version, namespaced on the same terms as
    /// [`MaterialEntry::block`].
    pub block: String,
}

/// Validated, lookup-ready abstract-material catalog.
#[derive(Debug, Clone)]
pub struct MaterialsIndex {
    /// `token → mapping`, in insertion order. `IndexMap` is here so the
    /// suggestion-pool order matches the declared order — the
    /// `nearest_match` tie-break is "first-seen wins", and the on-disk order
    /// is the most readable bias for that.
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
        let namespace = catalog.namespace.as_str();
        let mut by_token: IndexMap<String, MaterialMapping> =
            IndexMap::with_capacity(catalog.entries.len());
        for entry in catalog.entries {
            // Reject the duplicate *before* mutating the map so a rejected
            // pack never observes an inconsistent intermediate state; the
            // resulting error reads as "the second declaration of token X
            // was refused", which is what the pack author needs to fix.
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

    /// Look up an abstract token, ignoring any per-version override.
    ///
    /// This is the answer for a caller with no version in hand. A compile
    /// has one and uses [`Self::lookup_id_for`]; the two differ only for a
    /// token the pack respells somewhere inside its range.
    #[must_use]
    pub fn lookup_id(&self, token: &str) -> Option<&str> {
        self.by_token.get(token).map(|m| m.default.as_str())
    }

    /// Look up an abstract token as `mc_version` spells it, falling back to
    /// the default when that version declares no override — or when no
    /// version is pinned at all.
    #[must_use]
    pub fn lookup_id_for(&self, token: &str, mc_version: Option<&str>) -> Option<&str> {
        let mapping = self.by_token.get(token)?;
        let overridden = mc_version.and_then(|v| mapping.by_version.get(v));
        Some(overridden.unwrap_or(&mapping.default).as_str())
    }

    /// Every `(token, mc_version)` pair that carries an override, in
    /// declared order. The loader uses it to refuse an override naming a
    /// version the pack's `data_versions` table does not declare.
    pub fn overrides(&self) -> impl Iterator<Item = (&str, &str)> {
        self.by_token.iter().flat_map(|(token, mapping)| {
            mapping
                .by_version
                .keys()
                .map(move |version| (token.as_str(), version.as_str()))
        })
    }

    /// Every `(token, mc_version, id)` triple this catalog can resolve to,
    /// with `mc_version` `None` for the default. Used by the pack tests
    /// that hold every mapping against the block tables.
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

    /// Iterate every declared token in insertion order without allocating.
    ///
    /// The `&dyn`-dispatched suggestion path
    /// (`cairn_lang_core::block_array::TargetRegistry::known_tokens`) has
    /// to hand back an owned `Vec` because a borrowing iterator cannot
    /// cross the dyn boundary; callers holding a concrete
    /// [`MaterialsIndex`] use this instead and pay for no clone.
    pub fn tokens(&self) -> impl Iterator<Item = &str> {
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
    /// One entry declared two overrides for the same version. As with a
    /// duplicate token, the silent outcome is "the last row wins", which is
    /// invisible to the pack author.
    #[error(
        "registry pack materials token `{token}` declares more than one override for `{mc_version}`"
    )]
    DuplicateMaterialOverride {
        /// Verbatim token text from the catalog.
        token: String,
        /// The version declared twice.
        mc_version: String,
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
        assert_eq!(
            index.lookup_id("wall.stone.cobble"),
            Some("minecraft:cobblestone")
        );
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
    fn duplicate_override_for_one_version_rejected_at_load_time() {
        // Same reason as a duplicate token: the silent outcome is "the last
        // row wins", which the pack author cannot see from the file.
        let src = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "token": "floor.stone.smooth", "block": "stone_bricks",
                  "overrides": [
                    { "mc_version": "1.21.0", "block": "stonebrick" },
                    { "mc_version": "1.21.0", "block": "stone" }
                  ] }
            ]
        }"#;
        let err = parse(src).unwrap_err();
        assert_eq!(
            err,
            MaterialsError::DuplicateMaterialOverride {
                token: "floor.stone.smooth".into(),
                mc_version: "1.21.0".into(),
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
    fn tokens_preserves_insertion_order() {
        let src = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "token": "a.b.c", "block": "stone" },
                { "token": "x.y.z", "block": "dirt" }
            ]
        }"#;
        let index = parse(src).unwrap();
        let order: Vec<&str> = index.tokens().collect();
        assert_eq!(order, vec!["a.b.c", "x.y.z"]);
    }
}
