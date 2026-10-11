//! Loads a [`RegistryPack`] from the packs embedded at compile time, or from a directory.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockIdSet, BlockState, TargetRegistry};
use cairn_lang_core::lock::HashHex;
use cairn_lang_core::resolve::{VersionOrder, compare_versions};
use cairn_lang_core::suggest::nearest_match;
use serde::de::DeserializeOwned;
use thiserror::Error;

use super::aliases::{AliasError, AliasIndex};
use super::blocks::{BlocksError, BlocksIndex};
use super::data_versions::{DataVersionEntry, DataVersionTable};
use super::hash::pack_hash;
use super::manifest::PackManifest;
use super::materials::{MaterialsError, MaterialsIndex};
use crate::data_version::{BedrockTarget, JavaTarget, UnsupportedTarget};

#[derive(Clone, Copy)]
struct BuiltinFiles {
    manifest: &'static str,
    data_versions: &'static str,
    materials: &'static str,
    blocks: &'static str,
    aliases: &'static str,
}

macro_rules! builtin_files {
    ($dir:literal) => {
        BuiltinFiles {
            manifest: include_str!(concat!("../../registry-data/", $dir, "/pack.json")),
            data_versions: include_str!(concat!(
                "../../registry-data/",
                $dir,
                "/data_versions.json"
            )),
            materials: include_str!(concat!("../../registry-data/", $dir, "/materials.json")),
            blocks: include_str!(concat!("../../registry-data/", $dir, "/blocks.json")),
            aliases: include_str!(concat!("../../registry-data/", $dir, "/aliases.json")),
        }
    };
}

const BUILTIN_JAVA: BuiltinFiles = builtin_files!("java");
const BUILTIN_BEDROCK: BuiltinFiles = builtin_files!("bedrock");

/// The highest manifest `schema_version` this build reads.
pub const SUPPORTED_MANIFEST_SCHEMA: u32 = 1;
/// The highest `data_versions.schema_version` this build reads; version 2 adds `targetable`.
pub const SUPPORTED_DATA_VERSIONS_SCHEMA: u32 = 2;

/// Where a [`RegistryPack`] came from, for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackSource {
    /// The pack embedded in the binary.
    Builtin,
    /// A pack directory on disk.
    Path(PathBuf),
}

/// A loaded and validated registry pack, built only by the loader.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RegistryPack {
    /// The manifest.
    pub manifest: PackManifest,
    /// The version table.
    pub data_versions: DataVersionTable,
    /// The materials catalog, empty without the component.
    pub materials: MaterialsIndex,
    /// The block-id tables; empty without the component, which turns id checks off.
    pub blocks: BlocksIndex,
    /// The alias groups, empty without the component.
    pub aliases: AliasIndex,
    /// The pack hash the lockfile records as `inputs.registry_pack_hash`.
    pub bytes_hash: HashHex,
    /// Where the pack came from.
    pub source: PackSource,
}

/// A registry pack seen through the one version a compile pinned, whose ids it checks against.
#[derive(Debug, Clone)]
pub struct PackView<'a> {
    materials: &'a MaterialsIndex,
    /// Edition-wide; `ids` narrows a group to the spellings this target declares.
    aliases: &'a AliasIndex,
    mc_version: Option<&'a str>,
    /// The pinned version's sorted ids; `None` unpinned or without a `blocks` component.
    ids: Option<&'a [String]>,
    /// The target as a diagnostic names it, such as `"bedrock 1.21.60"` or `"bedrock"`.
    label: String,
}

impl TargetRegistry for PackView<'_> {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        self.materials
            .lookup_id_for(token, self.mc_version)
            .map(BlockState::bare)
    }

    fn known_tokens(&self) -> Vec<String> {
        self.materials.tokens().map(str::to_owned).collect()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        self.ids.map(|ids| BlockIdSet::new(&self.label, ids))
    }

    fn aliases_for(&self, id: &str) -> Vec<String> {
        // Unfiltered, a group would suggest spellings from other versions and editions.
        let Some(ids) = self.ids else {
            return Vec::new();
        };
        self.aliases
            .spellings_of(id)
            .iter()
            .filter(|spelling| {
                ids.binary_search_by(|known| known.as_str().cmp(spelling.as_str()))
                    .is_ok()
            })
            .cloned()
            .collect()
    }
}

/// A registry pack that could not be read or failed validation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RegistryError {
    /// The manifest failed to deserialise.
    #[error("registry pack manifest: {source}")]
    Manifest {
        /// The serde error.
        #[source]
        source: serde_json::Error,
    },
    /// A component file failed to deserialise.
    #[error("registry pack `{file}`: {source}")]
    File {
        /// The component, such as `"data_versions"`.
        file: String,
        /// The serde error.
        #[source]
        source: serde_json::Error,
    },
    /// Reading a pack directory failed.
    #[error("registry pack i/o at `{path}`: {source}")]
    Io {
        /// The path.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The manifest or `data_versions` declared a schema newer than this build reads.
    #[error(
        "unsupported registry pack {kind} schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// `"manifest"` or `"data_versions"`.
        kind: &'static str,
        /// The declared version.
        got: u32,
        /// The highest version this build reads.
        supported: u32,
    },
    /// `data_versions.versions` was empty.
    #[error("registry pack `data_versions.versions` is empty")]
    EmptyVersionTable,
    /// `data_versions.latest` named no targetable row.
    #[error("registry pack `data_versions.latest` = `{latest}` is not a targetable version")]
    LatestNotInTable {
        /// The `latest` value.
        latest: String,
    },
    /// No row of `data_versions.versions` is targetable.
    #[error("registry pack `data_versions.versions` has no targetable version")]
    NoTargetableVersion,
    /// The version table's rows do not ascend by both `data_version` and label.
    #[error("registry pack `data_versions.versions` is not a usable ordering: {reason}")]
    VersionOrderBroken {
        /// Which rows disagree, and about what.
        reason: String,
    },
    /// The manifest's edition is not the one the pack was loaded as.
    #[error("registry pack edition `{got:?}` does not match expected `{expected:?}`")]
    EditionMismatch {
        /// The expected edition.
        expected: Edition,
        /// The manifest's edition.
        got: Edition,
    },
    /// The pack's `materials` catalog failed to validate.
    #[error("registry pack `materials`: {source}")]
    Materials {
        /// The validation error.
        #[source]
        source: MaterialsError,
    },
    /// A `materials` override named a version outside `data_versions`.
    #[error(
        "registry pack materials token `{token}` overrides `{mc_version}`, which `data_versions` does not declare"
    )]
    MaterialOverrideUnknownVersion {
        /// The token carrying the override.
        token: String,
        /// The version it named.
        mc_version: String,
    },
    /// The pack's `blocks` table failed to validate.
    #[error("registry pack `blocks`: {source}")]
    Blocks {
        /// The validation error.
        #[source]
        source: BlocksError,
    },
    /// The pack's `aliases` catalog failed to validate.
    #[error("registry pack `aliases`: {source}")]
    Aliases {
        /// The validation error.
        #[source]
        source: AliasError,
    },
    /// An alias group names no id any version of the pack's `blocks` declares, so it never answers.
    #[error(
        "registry pack aliases group [{spellings}] names no id any version of `blocks` declares"
    )]
    AliasGroupUnanswerable {
        /// The group's spellings, comma-joined.
        spellings: String,
    },
    /// `blocks` lacks a table for a targetable version, or has one for a version not in the table.
    #[error(
        "registry pack `blocks` and `data_versions` disagree: no id table for [{missing}], and an id table for unsupported [{extra}]"
    )]
    BlocksVersionMismatch {
        /// Versions only `data_versions` declares, comma-joined.
        missing: String,
        /// Versions only `blocks` declares, comma-joined.
        extra: String,
    },
}

impl From<MaterialsError> for RegistryError {
    fn from(source: MaterialsError) -> Self {
        Self::Materials { source }
    }
}

impl From<BlocksError> for RegistryError {
    fn from(source: BlocksError) -> Self {
        Self::Blocks { source }
    }
}

impl From<AliasError> for RegistryError {
    fn from(source: AliasError) -> Self {
        Self::Aliases { source }
    }
}

/// The targetable row `mc_version` names, as the row spells it. `1.21.0` names `1.21`
/// (`spec/versioning-editions` "The target is a compile-time parameter").
fn targetable_row_for(table: &DataVersionTable, mc_version: &str) -> Option<(String, i32)> {
    table
        .versions
        .iter()
        .filter(|e| e.targetable)
        .find(|e| compare_versions(&e.mc_version, mc_version).is_eq())
        .map(|e| (e.mc_version.clone(), e.data_version))
}

impl RegistryPack {
    /// The `(mc_version, data_version)` row `requested` names, `"latest"` included.
    ///
    /// # Panics
    ///
    /// When the pack is not of `edition`, whose integer would look valid and be wrong.
    fn resolve_target_row(
        &self,
        edition: Edition,
        requested: &str,
    ) -> Result<(String, i32), UnsupportedTarget> {
        assert_eq!(
            self.manifest.edition, edition,
            "{edition:?} target resolution against a non-{edition:?} pack is a caller bug",
        );
        if requested == "latest" {
            return Ok(
                targetable_row_for(&self.data_versions, &self.data_versions.latest)
                    .expect("latest validated at load time"),
            );
        }
        targetable_row_for(&self.data_versions, requested).ok_or_else(|| UnsupportedTarget {
            edition: self.manifest.edition,
            requested: requested.to_owned(),
            suggestion: self.suggestion_for(requested),
            supported: self.supported_list(),
        })
    }

    /// Resolves a `--target` value to a Java target.
    ///
    /// # Errors
    ///
    /// [`UnsupportedTarget`] when `requested` is neither `"latest"` nor a targetable version.
    ///
    /// # Panics
    ///
    /// When the pack is not a Java pack.
    pub fn resolve_java_target(&self, requested: &str) -> Result<JavaTarget, UnsupportedTarget> {
        let (mc_version, data_version) = self.resolve_target_row(Edition::Java, requested)?;
        Ok(JavaTarget {
            mc_version,
            data_version,
        })
    }

    /// Resolves a `--target` value to a Bedrock target.
    ///
    /// # Errors
    ///
    /// [`UnsupportedTarget`] when `requested` is neither `"latest"` nor a targetable version.
    ///
    /// # Panics
    ///
    /// When the pack is not a Bedrock pack.
    pub fn resolve_bedrock_target(
        &self,
        requested: &str,
    ) -> Result<BedrockTarget, UnsupportedTarget> {
        let (mc_version, block_version) = self.resolve_target_row(Edition::Bedrock, requested)?;
        Ok(BedrockTarget {
            mc_version,
            block_version,
        })
    }

    /// The targetable version or `"latest"` closest to `requested`, if any is close enough.
    fn suggestion_for(&self, requested: &str) -> Option<String> {
        let pool = self
            .targetable()
            .into_iter()
            .map(|e| e.mc_version.as_str())
            .chain(std::iter::once("latest"));
        nearest_match(requested, pool).map(str::to_owned)
    }

    /// The rows `--target` may name, ascending by `data_version`.
    #[must_use]
    pub fn targetable(&self) -> Vec<&DataVersionEntry> {
        let mut rows: Vec<&DataVersionEntry> = self
            .data_versions
            .versions
            .iter()
            .filter(|entry| entry.targetable)
            .collect();
        rows.sort_by_key(|entry| entry.data_version);
        rows
    }

    /// Every row, targetable or not, as the ordering an `@requires` floor is placed against
    /// (`spec/versioning-editions` "The target is a compile-time parameter").
    #[must_use]
    pub fn version_order(&self) -> VersionOrder {
        VersionOrder::new(
            self.data_versions
                .versions
                .iter()
                .map(|entry| (entry.mc_version.clone(), i64::from(entry.data_version))),
        )
    }

    /// The view block-array lowering consults for `mc_version`; `None` skips the id check.
    ///
    /// # Panics
    ///
    /// When the pack has block tables but none, or an unsorted one, for `mc_version`.
    #[must_use]
    pub fn view<'a>(&'a self, mc_version: Option<&'a str>) -> PackView<'a> {
        let ids = mc_version.and_then(|version| {
            let ids = self.blocks.ids_for(version);
            // INVARIANT(blocks-cover-versions): the loader refuses tables that miss a version.
            // Asserted in release too, since a skipped id check looks like a clean build.
            assert!(
                ids.is_some() || self.blocks.is_empty(),
                "pack `{}` has block tables but none for `{version}`",
                self.manifest.name,
            );
            // `BlockIdSet::contains` binary-searches.
            assert!(
                ids.is_none_or(<[String]>::is_sorted),
                "pack `{}` block table for `{version}` is not sorted",
                self.manifest.name,
            );
            ids
        });
        PackView {
            materials: &self.materials,
            aliases: &self.aliases,
            mc_version,
            ids,
            label: match mc_version {
                Some(version) => format!("{} {version}", self.manifest.edition.as_str()),
                None => self.manifest.edition.as_str().to_owned(),
            },
        }
    }

    /// The targetable versions, then `"latest"`, comma-joined for an error message.
    #[must_use]
    pub fn supported_list(&self) -> String {
        let mut entries: Vec<&str> = self
            .targetable()
            .into_iter()
            .map(|e| e.mc_version.as_str())
            .collect();
        entries.push("latest");
        entries.join(", ")
    }
}

/// The built-in pack for `edition`, parsed once per process.
///
/// # Panics
///
/// When the embedded pack fails to load, which means the build itself is broken.
pub fn builtin_pack(edition: Edition) -> &'static RegistryPack {
    static JAVA: OnceLock<RegistryPack> = OnceLock::new();
    static BEDROCK: OnceLock<RegistryPack> = OnceLock::new();
    let cell = match edition {
        Edition::Java => &JAVA,
        Edition::Bedrock => &BEDROCK,
    };
    cell.get_or_init(|| {
        load_builtin_pack(edition)
            .expect("built-in registry pack failed to load — this is a build invariant")
    })
}

/// Resolves a `--target` value against the built-in Java pack.
///
/// # Errors
///
/// [`UnsupportedTarget`] when `requested` is neither `"latest"` nor a targetable version.
pub fn resolve_java_target(requested: &str) -> Result<JavaTarget, UnsupportedTarget> {
    builtin_pack(Edition::Java).resolve_java_target(requested)
}

/// Resolves a `--target` value against the built-in Bedrock pack.
///
/// # Errors
///
/// [`UnsupportedTarget`] when `requested` is neither `"latest"` nor a targetable version.
pub fn resolve_bedrock_target(requested: &str) -> Result<BedrockTarget, UnsupportedTarget> {
    builtin_pack(Edition::Bedrock).resolve_bedrock_target(requested)
}

/// Parses the built-in pack for `edition`, returning the error [`builtin_pack`] panics on.
///
/// # Errors
///
/// [`RegistryError`] when the embedded files are corrupt.
pub fn load_builtin_pack(edition: Edition) -> Result<RegistryPack, RegistryError> {
    let files = match edition {
        Edition::Java => BUILTIN_JAVA,
        Edition::Bedrock => BUILTIN_BEDROCK,
    };
    load_pack(&files, edition)
}

/// Loads a Java pack from a directory holding `pack.json` and the files it names.
///
/// # Errors
///
/// [`RegistryError`] when a file cannot be read, parsed or validated.
pub fn load_from_dir(dir: &Path) -> Result<RegistryPack, RegistryError> {
    load_pack(&PackDir(dir), Edition::Java)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Component {
    Manifest,
    DataVersions,
    Materials,
    Blocks,
    Aliases,
}

impl Component {
    fn parse_error(self, source: serde_json::Error) -> RegistryError {
        let file = match self {
            Component::Manifest => return RegistryError::Manifest { source },
            Component::DataVersions => "data_versions",
            Component::Materials => "materials",
            Component::Blocks => "blocks",
            Component::Aliases => "aliases",
        };
        RegistryError::File {
            file: file.to_owned(),
            source,
        }
    }
}

/// Where [`load_pack`] reads each file from, so embedded and on-disk packs share one validation.
trait ComponentSource {
    /// Text of `component`, which the manifest names `file`.
    fn read_text(&self, component: Component, file: &str) -> Result<Cow<'_, str>, RegistryError>;

    /// The [`PackSource`] a pack read from here reports.
    fn provenance(&self) -> PackSource;
}

impl ComponentSource for BuiltinFiles {
    fn read_text(&self, component: Component, _file: &str) -> Result<Cow<'_, str>, RegistryError> {
        Ok(Cow::Borrowed(match component {
            Component::Manifest => self.manifest,
            Component::DataVersions => self.data_versions,
            Component::Materials => self.materials,
            Component::Blocks => self.blocks,
            Component::Aliases => self.aliases,
        }))
    }

    fn provenance(&self) -> PackSource {
        PackSource::Builtin
    }
}

/// A pack directory: `pack.json` plus the files it names.
struct PackDir<'a>(&'a Path);

impl ComponentSource for PackDir<'_> {
    fn read_text(&self, _component: Component, file: &str) -> Result<Cow<'_, str>, RegistryError> {
        let path = self.0.join(file);
        let bytes = std::fs::read(&path).map_err(|source| RegistryError::Io {
            path: path.clone(),
            source,
        })?;
        String::from_utf8(bytes)
            .map(Cow::Owned)
            .map_err(|err| RegistryError::Io {
                path,
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, err.utf8_error()),
            })
    }

    fn provenance(&self) -> PackSource {
        PackSource::Path(self.0.to_path_buf())
    }
}

/// Parse, validate and hash one pack from `source`.
fn load_pack(
    source: &impl ComponentSource,
    expected_edition: Edition,
) -> Result<RegistryPack, RegistryError> {
    let manifest_text = source.read_text(Component::Manifest, "pack.json")?;
    let manifest: PackManifest = parse_component(&manifest_text, Component::Manifest)?;
    validate_manifest(&manifest, expected_edition)?;

    let data_versions_text =
        source.read_text(Component::DataVersions, &manifest.files.data_versions)?;
    let data_versions: DataVersionTable =
        parse_component(&data_versions_text, Component::DataVersions)?;
    validate_data_versions(&data_versions)?;

    let (materials, materials_text) = optional_component(
        source,
        Component::Materials,
        manifest.files.materials.as_deref(),
        MaterialsIndex::from_catalog,
        MaterialsIndex::empty,
    )?;
    let (blocks, blocks_text) = optional_component(
        source,
        Component::Blocks,
        manifest.files.blocks.as_deref(),
        BlocksIndex::from_catalog,
        BlocksIndex::empty,
    )?;
    let (aliases, aliases_text) = optional_component(
        source,
        Component::Aliases,
        manifest.files.aliases.as_deref(),
        AliasIndex::from_catalog,
        AliasIndex::empty,
    )?;
    validate_material_overrides(&materials, &data_versions)?;
    validate_blocks_cover_versions(&blocks, &data_versions)?;
    validate_aliases_answerable(&aliases, &blocks)?;

    let mut components: Vec<(&str, &[u8])> = vec![(
        manifest.files.data_versions.as_str(),
        data_versions_text.as_bytes(),
    )];
    for (file, text) in [
        (&manifest.files.materials, &materials_text),
        (&manifest.files.blocks, &blocks_text),
        (&manifest.files.aliases, &aliases_text),
    ] {
        if let (Some(file), Some(text)) = (file, text) {
            components.push((file.as_str(), text.as_bytes()));
        }
    }
    let bytes_hash = pack_hash(manifest_text.as_bytes(), &components);
    Ok(RegistryPack {
        manifest,
        data_versions,
        materials,
        blocks,
        aliases,
        bytes_hash,
        source: source.provenance(),
    })
}

/// An optional component's index and text, or its empty index when the manifest names none.
fn optional_component<'s, C, I, E>(
    source: &'s impl ComponentSource,
    component: Component,
    file: Option<&str>,
    index: impl FnOnce(C) -> Result<I, E>,
    empty: impl FnOnce() -> I,
) -> Result<(I, Option<Cow<'s, str>>), RegistryError>
where
    C: DeserializeOwned,
    RegistryError: From<E>,
{
    let Some(file) = file else {
        return Ok((empty(), None));
    };
    let text = source.read_text(component, file)?;
    let catalog: C = parse_component(&text, component)?;
    Ok((index(catalog)?, Some(text)))
}

fn parse_component<T: DeserializeOwned>(
    text: &str,
    component: Component,
) -> Result<T, RegistryError> {
    serde_json::from_str(text).map_err(|source| component.parse_error(source))
}

/// Refuses an alias group no version declares a single spelling of. One is enough, since a group
/// exists to hold the spellings this edition does not use.
fn validate_aliases_answerable(
    aliases: &AliasIndex,
    blocks: &BlocksIndex,
) -> Result<(), RegistryError> {
    if blocks.is_empty() {
        return Ok(());
    }
    for group in aliases.groups() {
        if group
            .iter()
            .any(|id| blocks.declared_by_some_version(id) == Some(true))
        {
            continue;
        }
        return Err(RegistryError::AliasGroupUnanswerable {
            spellings: group.join(", "),
        });
    }
    Ok(())
}

/// Refuses a materials override naming a version `data_versions` lacks, which would never fire.
fn validate_material_overrides(
    materials: &MaterialsIndex,
    data_versions: &DataVersionTable,
) -> Result<(), RegistryError> {
    for (token, mc_version) in materials.overrides() {
        if !data_versions
            .versions
            .iter()
            .any(|e| e.mc_version == mc_version)
        {
            return Err(RegistryError::MaterialOverrideUnknownVersion {
                token: token.to_owned(),
                mc_version: mc_version.to_owned(),
            });
        }
    }
    Ok(())
}

/// Refuses `blocks` tables that miss a targetable version, or cover one `data_versions` lacks.
fn validate_blocks_cover_versions(
    blocks: &BlocksIndex,
    data_versions: &DataVersionTable,
) -> Result<(), RegistryError> {
    if blocks.is_empty() {
        return Ok(());
    }
    let missing: Vec<&str> = data_versions
        .versions
        .iter()
        .filter(|e| e.targetable)
        .map(|e| e.mc_version.as_str())
        .filter(|v| blocks.ids_for(v).is_none())
        .collect();
    let extra: Vec<&str> = blocks
        .versions()
        .filter(|v| {
            !data_versions
                .versions
                .iter()
                .any(|e| e.mc_version.as_str() == *v)
        })
        .collect();
    if missing.is_empty() && extra.is_empty() {
        return Ok(());
    }
    Err(RegistryError::BlocksVersionMismatch {
        missing: missing.join(", "),
        extra: extra.join(", "),
    })
}

fn validate_manifest(manifest: &PackManifest, expected: Edition) -> Result<(), RegistryError> {
    if manifest.schema_version > SUPPORTED_MANIFEST_SCHEMA {
        return Err(RegistryError::UnsupportedSchemaVersion {
            kind: "manifest",
            got: manifest.schema_version,
            supported: SUPPORTED_MANIFEST_SCHEMA,
        });
    }
    if manifest.edition != expected {
        return Err(RegistryError::EditionMismatch {
            expected,
            got: manifest.edition,
        });
    }
    Ok(())
}

fn validate_data_versions(table: &DataVersionTable) -> Result<(), RegistryError> {
    if table.schema_version > SUPPORTED_DATA_VERSIONS_SCHEMA {
        return Err(RegistryError::UnsupportedSchemaVersion {
            kind: "data_versions",
            got: table.schema_version,
            supported: SUPPORTED_DATA_VERSIONS_SCHEMA,
        });
    }
    if table.versions.is_empty() {
        return Err(RegistryError::EmptyVersionTable);
    }
    if !table
        .versions
        .iter()
        .any(|e| e.targetable && e.mc_version == table.latest)
    {
        return Err(RegistryError::LatestNotInTable {
            latest: table.latest.clone(),
        });
    }
    if !table.versions.iter().any(|e| e.targetable) {
        return Err(RegistryError::NoTargetableVersion);
    }
    validate_version_order(table)?;
    Ok(())
}

/// Requires keys and labels both strictly ascending. A floor outside the table is placed by
/// label, so labels must agree with keys; `1.21` and `1.21.0` count as one label.
fn validate_version_order(table: &DataVersionTable) -> Result<(), RegistryError> {
    let mut previous: Option<&super::data_versions::DataVersionEntry> = None;
    for entry in &table.versions {
        if let Some(previous) = previous {
            if entry.data_version <= previous.data_version {
                return Err(RegistryError::VersionOrderBroken {
                    reason: format!(
                        "`{}` has data_version {} but follows `{}` with {}; keys must ascend and be unique",
                        entry.mc_version,
                        entry.data_version,
                        previous.mc_version,
                        previous.data_version,
                    ),
                });
            }
            if !compare_versions(&entry.mc_version, &previous.mc_version).is_gt() {
                return Err(RegistryError::VersionOrderBroken {
                    reason: format!(
                        "`{}` does not sort above `{}` by label, but its data_version is higher; placing a floor outside the table reads the labels",
                        entry.mc_version, previous.mc_version,
                    ),
                });
            }
        }
        previous = Some(entry);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
