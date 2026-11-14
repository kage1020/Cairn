//! The paintings component of a registry pack: each targetable version's painting variants, with
//! the size each hangs at and the way the edition's entity data spells it, which a `painting`'s
//! `variant=` is checked against (`spec/entities` "Hanging entities").

use indexmap::IndexMap;
use serde::Deserialize;

use super::data_versions::DataVersionTable;
use super::namespaced;

/// The highest `paintings.schema_version` this build reads.
pub const SUPPORTED_PAINTINGS_SCHEMA: u32 = 1;

/// The largest side of a painting the loader takes, in blocks.
pub const MAX_PAINTING_SIDE: u32 = 16;

/// The body of `paintings.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaintingsCatalog {
    /// The catalog's schema version.
    pub schema_version: u32,
    /// The namespace a Java spelling takes.
    pub namespace: String,
    /// Where the rows came from, for a human reader; the compiler ignores it.
    #[serde(default)]
    pub source: Option<String>,
    /// The tables, each the paintings a set of versions shares.
    pub tables: Vec<PaintingTable>,
}

/// One `tables` row of a [`PaintingsCatalog`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaintingTable {
    /// The targetable versions that have every painting of the table.
    pub versions: Vec<String>,
    /// The paintings.
    pub paintings: Vec<PaintingRow>,
}

/// One painting of a [`PaintingTable`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaintingRow {
    /// The name a `variant=` writes, without a namespace.
    pub variant: String,
    /// Its width in blocks.
    pub width: u32,
    /// Its height in blocks.
    pub height: u32,
    /// The way the edition's entity data spells it; the namespaced name when absent.
    #[serde(default)]
    pub spelling: Option<String>,
}

/// One painting as a version has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Painting {
    /// The way the edition's entity data spells it.
    pub spelling: String,
    /// Its width in blocks.
    pub width: u32,
    /// Its height in blocks.
    pub height: u32,
}

/// Validated paintings, by version and then by name, in table order.
#[derive(Debug, Clone, Default)]
pub struct PaintingIndex {
    versions: IndexMap<String, IndexMap<String, Painting>>,
}

impl PaintingIndex {
    /// An index with no table, for a pack without a `paintings` component.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Validates `catalog` against the versions `data_versions` can target.
    ///
    /// # Errors
    ///
    /// [`PaintingsError`] for the first fault in the catalog.
    pub fn from_catalog(
        catalog: PaintingsCatalog,
        data_versions: &DataVersionTable,
    ) -> Result<Self, PaintingsError> {
        if catalog.schema_version > SUPPORTED_PAINTINGS_SCHEMA {
            return Err(PaintingsError::UnsupportedSchemaVersion {
                got: catalog.schema_version,
                supported: SUPPORTED_PAINTINGS_SCHEMA,
            });
        }
        let targetable: Vec<&str> = data_versions
            .versions
            .iter()
            .filter(|row| row.targetable)
            .map(|row| row.mc_version.as_str())
            .collect();
        let mut versions: IndexMap<String, IndexMap<String, Painting>> = targetable
            .iter()
            .map(|version| ((*version).to_owned(), IndexMap::new()))
            .collect();
        for table in catalog.tables {
            if table.versions.is_empty() || table.paintings.is_empty() {
                return Err(PaintingsError::EmptyTable {
                    versions: table.versions.join(", "),
                });
            }
            for version in &table.versions {
                let Some(held) = versions.get_mut(version) else {
                    return Err(PaintingsError::UntargetableVersion {
                        version: version.clone(),
                    });
                };
                for row in &table.paintings {
                    let painting = painting_of(&catalog.namespace, row)?;
                    if held.insert(row.variant.clone(), painting).is_some() {
                        return Err(PaintingsError::DuplicateVariant {
                            variant: row.variant.clone(),
                            version: version.clone(),
                        });
                    }
                }
            }
        }
        if let Some((version, _)) = versions.iter().find(|(_, held)| held.is_empty()) {
            return Err(PaintingsError::VersionWithoutPaintings {
                version: version.clone(),
            });
        }
        Ok(Self { versions })
    }

    /// Whether the index holds no table.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    /// The painting `variant` names at `version`, or `None` when that version has none of it.
    #[must_use]
    pub fn at(&self, version: &str, variant: &str) -> Option<&Painting> {
        self.versions.get(version)?.get(variant)
    }

    /// The paintings `version` has, by name, in table order; empty for a version without one.
    pub fn variants_at(&self, version: &str) -> impl Iterator<Item = &str> {
        self.versions
            .get(version)
            .into_iter()
            .flat_map(|held| held.keys().map(String::as_str))
    }

    /// Every version's painting `variant` names, in version order, with the version.
    pub fn everywhere<'a>(
        &'a self,
        variant: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a Painting)> + 'a {
        self.versions
            .iter()
            .filter_map(move |(version, held)| Some((version.as_str(), held.get(variant)?)))
    }

    /// Every name some version has, each once, in version and then table order.
    #[must_use]
    pub fn every_variant(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for name in self.versions.values().flat_map(IndexMap::keys) {
            if !names.contains(&name.as_str()) {
                names.push(name);
            }
        }
        names
    }
}

/// `row` as a [`Painting`], or the fault in it.
fn painting_of(namespace: &str, row: &PaintingRow) -> Result<Painting, PaintingsError> {
    if row.variant.is_empty() || row.variant.contains(':') {
        return Err(PaintingsError::BadVariant {
            variant: row.variant.clone(),
        });
    }
    for (side, value) in [("width", row.width), ("height", row.height)] {
        if !(1..=MAX_PAINTING_SIDE).contains(&value) {
            return Err(PaintingsError::BadSize {
                variant: row.variant.clone(),
                side,
                value,
            });
        }
    }
    let spelling = match &row.spelling {
        Some(spelling) if spelling.trim().is_empty() => {
            return Err(PaintingsError::EmptySpelling {
                variant: row.variant.clone(),
            });
        }
        Some(spelling) => spelling.clone(),
        None => namespaced(namespace, &row.variant),
    };
    Ok(Painting {
        spelling,
        width: row.width,
        height: row.height,
    })
}

/// A fault in a [`PaintingsCatalog`], always the pack author's.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum PaintingsError {
    /// The catalog's `schema_version` is newer than this build reads.
    #[error(
        "unsupported registry pack paintings schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// A table names no version or no painting.
    #[error("registry pack paintings table [{versions}] names no version or no painting")]
    EmptyTable {
        /// The table's versions, comma-joined.
        versions: String,
    },
    /// A table names a version `data_versions` cannot target.
    #[error(
        "registry pack paintings names `{version}`, which is no targetable row of `data_versions`"
    )]
    UntargetableVersion {
        /// The version.
        version: String,
    },
    /// A version has one painting twice.
    #[error("registry pack paintings gives `{variant}` twice at `{version}`")]
    DuplicateVariant {
        /// The painting.
        variant: String,
        /// The version.
        version: String,
    },
    /// A targetable version has no painting, so every `variant=` would be refused there.
    #[error("registry pack paintings gives targetable `{version}` no painting")]
    VersionWithoutPaintings {
        /// The version.
        version: String,
    },
    /// A name is empty or carries a namespace, which a `variant=` never writes.
    #[error("registry pack paintings names `{variant}`, which is not a bare name")]
    BadVariant {
        /// The name.
        variant: String,
    },
    /// A side is not a whole number of blocks from 1 to [`MAX_PAINTING_SIDE`].
    #[error(
        "registry pack paintings gives `{variant}` a {side} of {value}; a side is 1 to 16 blocks"
    )]
    BadSize {
        /// The painting.
        variant: String,
        /// `"width"` or `"height"`.
        side: &'static str,
        /// The value.
        value: u32,
    },
    /// A spelling is empty.
    #[error("registry pack paintings gives `{variant}` an empty spelling")]
    EmptySpelling {
        /// The painting.
        variant: String,
    },
}

#[cfg(test)]
mod tests;
