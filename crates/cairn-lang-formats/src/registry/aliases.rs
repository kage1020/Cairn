//! Alias-group component of a [`crate::registry::RegistryPack`].
//!
//! Answers the question a distance search cannot: *what does this target
//! call the block I just named?* `E_UNKNOWN_ID` suggests a replacement by
//! Damerau-Levenshtein distance over the target's own block table, which
//! catches a typo (`oak_plank` → `oak_planks`) and nothing else. A rename
//! is not a typo — Bedrock spells Java's `light` `light_block_0` …
//! `light_block_15`, eight edits away — so the search has no candidate to
//! offer for the case an author is far more likely to hit, and
//! `spec/versioning-editions` "Fail-loud and minimum-version inference" asks
//! an error to return "the closed set of candidates valid in the target"
//! rather than fall silent.
//!
//! # What a row is keyed on
//!
//! One row is a **group of spellings**: the names one block has worn,
//! across editions and across one edition's own range, including the
//! several ids a single old spelling split into.
//!
//! ```json
//! { "spellings": ["light", "light_block", "light_block_0", "light_block_1"] }
//! ```
//!
//! Nothing in the row says which spelling belongs to which
//! `(edition, version)`, and that is deliberate: the `blocks` component
//! already knows, per version, which ids exist. Resolution is therefore
//! "take the group, keep the members the pinned target declares" — one
//! table that answers Java → Bedrock (`oak_sign` → `standing_sign`) and
//! Bedrock 1.21.0 → 1.21.40 (`stonebrick` → `stone_bricks`) with the same
//! rows, because the version scoping is not in them.
//!
//! The cost of that key is the case it cannot express: a spelling two
//! editions both declare, meaning different blocks. Bedrock's `snow` is
//! Java's `snow_block` and Java's `snow` is Bedrock's `snow_layer`, so the
//! two pairs would have to share a row keyed on `snow` alone and would
//! then answer each other's question. Such a pair gets no row at all;
//! saying nothing is what this component does everywhere it has nothing to
//! say, and the distance search still runs behind it.

use indexmap::IndexMap;
use serde::Deserialize;

use super::namespaced;

/// Highest `aliases.schema_version` this Cairn build understands.
pub const SUPPORTED_ALIASES_SCHEMA: u32 = 1;

/// On-disk `aliases.json` body.
#[derive(Debug, Clone, Deserialize)]
pub struct AliasCatalog {
    /// Schema version of the catalog itself.
    pub schema_version: u32,
    /// Default id namespace, applied to every spelling that does not carry
    /// its own `namespace:` prefix — the same rule
    /// [`crate::registry::BlocksCatalog::namespace`] states for the block
    /// tables these rows are resolved against.
    pub namespace: String,
    /// The groups, in declared order. A suggestion lists the members a
    /// target declares in this order, so the file's order is the order an
    /// author reads.
    pub groups: Vec<AliasGroup>,
}

/// One `groups` row of an [`AliasCatalog`].
#[derive(Debug, Clone, Deserialize)]
pub struct AliasGroup {
    /// Every spelling of the block, namespace optional. Two or more: a
    /// group of one names nothing it could be confused with.
    pub spellings: Vec<String>,
}

/// Validated, lookup-ready alias groups.
#[derive(Debug, Clone)]
pub struct AliasIndex {
    /// Groups in declared order, each fully namespaced and in the order the
    /// file wrote them.
    groups: Vec<Vec<String>>,
    /// `spelling → index into groups`. Every spelling belongs to exactly
    /// one group, which is what makes the lookup a single answer rather
    /// than a merge of several.
    by_spelling: IndexMap<String, usize>,
}

impl AliasIndex {
    /// Index with no groups, used when a pack omits the `aliases`
    /// component. [`Self::spellings_of`] answers with an empty slice rather
    /// than `None` because the caller's fallback, the distance search, is
    /// the same either way.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            groups: Vec::new(),
            by_spelling: IndexMap::new(),
        }
    }

    /// Fold a parsed [`AliasCatalog`] into a lookup index.
    ///
    /// # Errors
    ///
    /// Returns [`AliasError`] for an unsupported `schema_version`, a
    /// catalog declaring no groups at all, a group of fewer than two
    /// spellings, a spelling repeated inside one group, or a spelling
    /// claimed by two groups.
    pub fn from_catalog(catalog: AliasCatalog) -> Result<Self, AliasError> {
        if catalog.schema_version > SUPPORTED_ALIASES_SCHEMA {
            return Err(AliasError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_ALIASES_SCHEMA,
            });
        }
        if catalog.groups.is_empty() {
            return Err(AliasError::NoGroups);
        }

        let namespace = catalog.namespace.as_str();
        let mut groups: Vec<Vec<String>> = Vec::with_capacity(catalog.groups.len());
        let mut by_spelling: IndexMap<String, usize> = IndexMap::new();
        for group in catalog.groups {
            let at = groups.len();
            let mut spellings: Vec<String> = Vec::with_capacity(group.spellings.len());
            for spelling in group.spellings {
                let id = namespaced(namespace, &spelling);
                // A repeat within this group and a spelling another group
                // claims need different edits, so they are reported apart.
                match by_spelling.get(&id) {
                    Some(&owner) if owner == at => {
                        return Err(AliasError::DuplicateSpelling { id });
                    }
                    Some(_) => return Err(AliasError::SpellingInTwoGroups { id }),
                    None => {}
                }
                by_spelling.insert(id.clone(), at);
                spellings.push(id);
            }
            if spellings.len() < 2 {
                return Err(AliasError::GroupTooSmall {
                    spellings: spellings.join(", "),
                });
            }
            groups.push(spellings);
        }
        Ok(Self {
            groups,
            by_spelling,
        })
    }

    /// Every spelling of the block `id` names, `id` included, in declared
    /// order — empty when no group claims it.
    ///
    /// `id` itself is kept: callers filter the group by their target's
    /// table anyway, and dropping it would make the answer depend on which
    /// caller asked.
    #[must_use]
    pub fn spellings_of(&self, id: &str) -> &[String] {
        self.by_spelling
            .get(id)
            .map_or(&[], |&at| self.groups[at].as_slice())
    }

    /// Every group, in declared order. The loader walks these to refuse a
    /// group no version of the pack's own block tables can answer with.
    pub fn groups(&self) -> impl Iterator<Item = &[String]> {
        self.groups.iter().map(Vec::as_slice)
    }

    /// Number of groups declared.
    #[must_use]
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// `true` when the index carries no group at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// Errors raised while folding an [`AliasCatalog`].
///
/// Every variant is a pack-author mistake. None of them can be reached by
/// something an author of a `.crn` file wrote.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum AliasError {
    /// Catalog declared a `schema_version` this build does not understand.
    #[error(
        "unsupported registry pack aliases schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// Version the catalog declared.
        got: u32,
        /// Highest version this build supports.
        supported: u32,
    },
    /// The catalog declared no groups. A pack that names the component and
    /// then fills it with nothing reads as "renames are answered here" at
    /// every call site, and answers none of them; leaving the component
    /// out says the same thing where a reader can see it.
    #[error("registry pack aliases declares no groups")]
    NoGroups,
    /// A group carried fewer than two spellings. One spelling is a group
    /// that can only ever suggest the id the caller already has, which is
    /// the answer [`crate::registry::BlocksIndex`] gave before it asked.
    #[error("registry pack aliases group [{spellings}] has fewer than two spellings")]
    GroupTooSmall {
        /// The spellings it did carry, comma-joined. Empty for an empty
        /// group, which is the one case that cannot name itself.
        spellings: String,
    },
    /// One group listed the same spelling twice.
    #[error("registry pack aliases group declares `{id}` more than once")]
    DuplicateSpelling {
        /// Verbatim id, after namespacing.
        id: String,
    },
    /// Two groups claimed the same spelling, so a lookup for it has two
    /// answers and no rule for choosing. Almost always two halves of one
    /// family written as separate rows; merging them is the fix.
    #[error("registry pack aliases declares `{id}` in two groups")]
    SpellingInTwoGroups {
        /// The spelling both groups claimed.
        id: String,
    },
}

#[cfg(test)]
mod tests;
