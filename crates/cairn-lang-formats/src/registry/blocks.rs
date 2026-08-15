//! Per-version block-id table component of a [`crate::registry::RegistryPack`].
//!
//! Answers one question: *does this block id exist in the target the compile
//! is pinned to?* Spec versioning-editions §10.4 makes an unknown id a hard
//! error, and until this component landed the check was structural only
//! ("the id has exactly one `:`"), so `@totally_not_a_block` rode all the way
//! into a written `.mcstructure`.
//!
//! The on-disk shape follows §10.3's folding rule — "fold versions with
//! `inherits + diffs`" — because the alternative, one full list per version,
//! repeats a thousand shared ids three times and hides the interesting part.
//! What a reader wants from `blocks.json` is exactly the diff: Bedrock
//! 1.21.40 is where `stonebrick` became `stone_bricks` and `light_block`
//! became `light_block_0` … `light_block_15`, and its `removed` list is
//! that flattening wave written out in full.
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
    /// component; [`Self::ids_for`] then always misses, and the lowering
    /// pass reads that as "this pack cannot refute an id" rather than as
    /// "no id is valid".
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
mod tests {
    use super::*;

    fn parse(src: &str) -> Result<BlocksIndex, BlocksError> {
        let catalog: BlocksCatalog =
            serde_json::from_str(src).expect("test catalog parses as JSON");
        BlocksIndex::from_catalog(catalog)
    }

    const TWO_VERSIONS: &str = r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": ["stonebrick", "cobblestone"] },
        "diffs": [
            { "mc_version": "1.1", "inherits": "1.0",
              "added": ["stone_bricks"], "removed": ["stonebrick"] }
        ]
    }"#;

    #[test]
    fn base_is_namespaced_and_sorted() {
        let index = parse(TWO_VERSIONS).expect("catalog");
        assert_eq!(
            index.ids_for("1.0"),
            Some(
                [
                    "minecraft:cobblestone".to_owned(),
                    "minecraft:stonebrick".to_owned()
                ]
                .as_slice()
            ),
        );
    }

    #[test]
    fn a_diff_applies_its_rename_to_that_version_only() {
        let index = parse(TWO_VERSIONS).expect("catalog");
        let old = index.ids_for("1.0").expect("base version");
        let new = index.ids_for("1.1").expect("diffed version");
        assert!(old.contains(&"minecraft:stonebrick".to_owned()));
        assert!(!old.contains(&"minecraft:stone_bricks".to_owned()));
        assert!(new.contains(&"minecraft:stone_bricks".to_owned()));
        assert!(!new.contains(&"minecraft:stonebrick".to_owned()));
    }

    #[test]
    fn versions_are_listed_in_resolution_order() {
        let index = parse(TWO_VERSIONS).expect("catalog");
        assert_eq!(index.versions().collect::<Vec<_>>(), vec!["1.0", "1.1"]);
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn a_version_the_catalog_never_names_has_no_table() {
        let index = parse(TWO_VERSIONS).expect("catalog");
        assert!(index.ids_for("1.2").is_none());
    }

    #[test]
    fn an_entry_may_carry_its_own_namespace() {
        let index = parse(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": ["create:cogwheel"] }
        }"#,
        )
        .expect("catalog");
        assert_eq!(
            index.ids_for("1.0"),
            Some(["create:cogwheel".to_owned()].as_slice()),
        );
    }

    #[test]
    fn the_empty_index_describes_no_version() {
        let index = BlocksIndex::empty();
        assert!(index.is_empty());
        assert!(index.ids_for("1.0").is_none());
    }

    #[test]
    fn an_unsupported_schema_version_is_refused() {
        let err = parse(
            r#"{
            "schema_version": 999,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": ["stone"] }
        }"#,
        )
        .expect_err("unsupported schema");
        assert_eq!(
            err,
            BlocksError::UnsupportedSchemaVersion {
                got: 999,
                supported: SUPPORTED_BLOCKS_SCHEMA,
            },
        );
    }

    #[test]
    fn an_empty_base_is_refused() {
        let err = parse(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": [] }
        }"#,
        )
        .expect_err("empty base");
        assert_eq!(
            err,
            BlocksError::EmptyBase {
                version: "1.0".to_owned(),
            },
        );
    }

    #[test]
    fn a_repeated_id_in_the_base_is_refused() {
        let err = parse(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": ["stone", "stone"] }
        }"#,
        )
        .expect_err("duplicate id");
        assert_eq!(
            err,
            BlocksError::DuplicateBlock {
                version: "1.0".to_owned(),
                id: "minecraft:stone".to_owned(),
            },
        );
    }

    #[test]
    fn a_repeated_version_is_refused() {
        let err = parse(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": ["stone"] },
            "diffs": [
                { "mc_version": "1.0", "inherits": "1.0", "added": ["dirt"] }
            ]
        }"#,
        )
        .expect_err("duplicate version");
        assert_eq!(
            err,
            BlocksError::DuplicateVersion {
                version: "1.0".to_owned(),
            },
        );
    }

    #[test]
    fn a_diff_that_inherits_a_later_version_is_refused() {
        let err = parse(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": ["stone"] },
            "diffs": [
                { "mc_version": "1.2", "inherits": "1.1", "added": ["dirt"] },
                { "mc_version": "1.1", "inherits": "1.0", "added": ["gravel"] }
            ]
        }"#,
        )
        .expect_err("forward inherits");
        assert_eq!(
            err,
            BlocksError::UnknownInherits {
                version: "1.2".to_owned(),
                inherits: "1.1".to_owned(),
            },
        );
    }

    #[test]
    fn removing_an_id_the_parent_lacks_is_refused() {
        let err = parse(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": ["stone"] },
            "diffs": [
                { "mc_version": "1.1", "inherits": "1.0", "removed": ["granite"] }
            ]
        }"#,
        )
        .expect_err("removed not in parent");
        assert_eq!(
            err,
            BlocksError::RemovedNotInParent {
                version: "1.1".to_owned(),
                inherits: "1.0".to_owned(),
                id: "minecraft:granite".to_owned(),
            },
        );
    }

    #[test]
    fn adding_an_id_the_parent_already_has_is_refused() {
        let err = parse(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": ["stone"] },
            "diffs": [
                { "mc_version": "1.1", "inherits": "1.0", "added": ["stone"] }
            ]
        }"#,
        )
        .expect_err("added already in parent");
        assert_eq!(
            err,
            BlocksError::AddedAlreadyInParent {
                version: "1.1".to_owned(),
                inherits: "1.0".to_owned(),
                id: "minecraft:stone".to_owned(),
            },
        );
    }

    #[test]
    fn a_removal_and_a_re_add_of_the_same_id_both_apply() {
        // The removal runs first by construction, so a diff may re-add an
        // id it just removed — that is how a pack expresses "same name,
        // new meaning" without needing a third list.
        let index = parse(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": ["stone"] },
            "diffs": [
                { "mc_version": "1.1", "inherits": "1.0",
                  "added": ["stone"], "removed": ["stone"] }
            ]
        }"#,
        )
        .expect("catalog");
        assert_eq!(
            index.ids_for("1.1"),
            Some(["minecraft:stone".to_owned()].as_slice()),
        );
    }

    #[test]
    fn a_diff_that_removes_everything_is_refused() {
        let err = parse(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": ["stone"] },
            "diffs": [
                { "mc_version": "1.1", "inherits": "1.0", "removed": ["stone"] }
            ]
        }"#,
        )
        .expect_err("folds to nothing");
        assert_eq!(
            err,
            BlocksError::EmptyTable {
                version: "1.1".to_owned(),
            },
        );
    }

    #[test]
    fn a_chain_of_diffs_folds_through_its_middle_version() {
        let index = parse(
            r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "base": { "mc_version": "1.0", "blocks": ["stone"] },
            "diffs": [
                { "mc_version": "1.1", "inherits": "1.0", "added": ["dirt"] },
                { "mc_version": "1.2", "inherits": "1.1", "added": ["gravel"] }
            ]
        }"#,
        )
        .expect("catalog");
        let last = index.ids_for("1.2").expect("last version");
        assert_eq!(
            last,
            [
                "minecraft:dirt".to_owned(),
                "minecraft:gravel".to_owned(),
                "minecraft:stone".to_owned(),
            ],
        );
    }
}
