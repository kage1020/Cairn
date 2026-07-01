//! Load a [`RegistryPack`] from the bundled built-in or from a directory.
//!
//! The built-in pack is embedded at compile time via `include_str!` so the
//! Cairn binary never has to find a data file at runtime. The on-disk
//! loader exists for tests in this module and for a future
//! `--registry-pack <dir>` CLI flag.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cairn_lang_core::lock::HashHex;
use cairn_lang_core::suggest::nearest_match;
use thiserror::Error;

use super::data_versions::DataVersionTable;
use super::hash::pack_hash;
use super::manifest::{PackEdition, PackManifest};
use super::materials::{MaterialsCatalog, MaterialsError, MaterialsIndex};

/// Built-in `pack.json` bytes, statically embedded at compile time.
const BUILTIN_JAVA_MANIFEST: &str = include_str!("../../registry-data/java/pack.json");
/// Built-in `data_versions.json` bytes, statically embedded at compile time.
const BUILTIN_JAVA_DATA_VERSIONS: &str =
    include_str!("../../registry-data/java/data_versions.json");
/// Built-in `materials.json` bytes, statically embedded at compile time.
const BUILTIN_JAVA_MATERIALS: &str = include_str!("../../registry-data/java/materials.json");

/// Highest manifest `schema_version` this Cairn build understands.
pub const SUPPORTED_MANIFEST_SCHEMA: u32 = 1;
/// Highest `data_versions.schema_version` this Cairn build understands.
pub const SUPPORTED_DATA_VERSIONS_SCHEMA: u32 = 1;

/// Where a [`RegistryPack`] came from. Surfaced in diagnostics so a
/// `--registry-pack` user can tell at a glance which directory was read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackSource {
    /// The pack bundled with the Cairn binary via `include_str!`.
    Builtin,
    /// A pack loaded from a directory on disk.
    Path(PathBuf),
}

/// A loaded and validated registry pack.
#[derive(Debug, Clone)]
pub struct RegistryPack {
    /// Top-level manifest.
    pub manifest: PackManifest,
    /// `(mc_version, data_version)` table.
    pub data_versions: DataVersionTable,
    /// Abstract material catalog. Empty when the pack omits the component
    /// (older packs, or a `--registry-pack` directory that has not been
    /// ported to PR2's schema).
    pub materials: MaterialsIndex,
    /// `sha256:<hex>` over the manifest + component bytes in declared order.
    /// Lands in the lockfile under `inputs.registry_pack_hash`.
    pub bytes_hash: HashHex,
    /// Provenance of the pack.
    pub source: PackSource,
}

/// Errors raised while reading or validating a registry pack.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RegistryError {
    /// Manifest JSON failed to deserialise.
    #[error("registry pack manifest: {source}")]
    Manifest {
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// A component file's JSON failed to deserialise.
    #[error("registry pack `{file}`: {source}")]
    File {
        /// Component name (e.g. `"data_versions"`).
        file: String,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// Filesystem I/O failed while loading a pack from disk.
    #[error("registry pack i/o at `{path}`: {source}")]
    Io {
        /// Offending path.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Manifest or component file declared a schema version this build
    /// does not understand. Returning a structured error lets the CLI
    /// tell the user which upgrade lane they need rather than failing
    /// with an opaque "missing field" message.
    #[error(
        "unsupported registry pack {kind} schema_version {got}; this Cairn supports up to {supported}"
    )]
    UnsupportedSchemaVersion {
        /// Which schema (`"manifest"` or `"data_versions"`).
        kind: &'static str,
        /// Version the pack declared.
        got: u32,
        /// Highest version this build supports.
        supported: u32,
    },
    /// `data_versions.versions` was empty — the table never has anything
    /// to resolve against, so we refuse it at load time rather than emit
    /// a confusing "unsupported target" later.
    #[error("registry pack `data_versions.versions` is empty")]
    EmptyVersionTable,
    /// `data_versions.latest` did not appear in `versions`. A dangling
    /// `latest` would resolve `--target latest` to nothing, which is a
    /// pack-author bug rather than a user error.
    #[error("registry pack `data_versions.latest` = `{latest}` not in versions")]
    LatestNotInTable {
        /// Verbatim `latest` value.
        latest: String,
    },
    /// The manifest declared an edition that does not match the slot the
    /// pack was loaded into (e.g. a Bedrock pack handed to the Java
    /// loader). Catches a misfiled `--registry-pack` directory before any
    /// downstream code looks at the data.
    #[error("registry pack edition `{got:?}` does not match expected `{expected:?}`")]
    EditionMismatch {
        /// Expected edition.
        expected: PackEdition,
        /// Edition the manifest declared.
        got: PackEdition,
    },
    /// The pack's `materials` catalog failed to validate.
    #[error("registry pack `materials`: {source}")]
    Materials {
        /// Underlying validation error from the materials catalog.
        #[source]
        source: MaterialsError,
    },
}

impl From<MaterialsError> for RegistryError {
    fn from(source: MaterialsError) -> Self {
        Self::Materials { source }
    }
}

/// Resolved Minecraft target used by the Java backend.
///
/// Held by [`crate::data_version::JavaTarget`] for backwards compatibility
/// with existing callers; this module just provides the function that
/// builds one from a [`DataVersionTable`].
fn entry_for(
    table: &DataVersionTable,
    mc_version: &str,
) -> Option<crate::data_version::JavaTarget> {
    table
        .versions
        .iter()
        .find(|e| e.mc_version == mc_version)
        .map(|e| crate::data_version::JavaTarget {
            mc_version: e.mc_version.clone(),
            data_version: e.data_version,
        })
}

impl RegistryPack {
    /// Resolve a CLI `--target` value against this pack's
    /// `DataVersionTable`. The literal `"latest"` aliases the row named
    /// by `DataVersionTable::latest`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::data_version::UnsupportedTarget`] when the
    /// requested string is neither the `"latest"` alias nor an exact
    /// `mc_version` match.
    ///
    /// # Panics
    ///
    /// Panics if the pack passed [`validate_data_versions`] but its
    /// `latest` field nonetheless does not point at a row in `versions`.
    /// That branch is dead by construction: validation rejects exactly
    /// this case at load time, and `RegistryPack` cannot be constructed
    /// without going through validation.
    pub fn resolve_java_target(
        &self,
        requested: &str,
    ) -> Result<crate::data_version::JavaTarget, crate::data_version::UnsupportedTarget> {
        if requested == "latest" {
            // `latest` was validated at load time against `versions`, so
            // the lookup here cannot miss.
            return Ok(entry_for(&self.data_versions, &self.data_versions.latest)
                .expect("latest validated at load time"));
        }
        entry_for(&self.data_versions, requested).ok_or_else(|| {
            crate::data_version::UnsupportedTarget {
                requested: requested.to_owned(),
                suggestion: self.suggestion_for(requested),
                supported: self.supported_list(),
            }
        })
    }

    /// Closest supported version for the matching field of
    /// [`crate::data_version::UnsupportedTarget`], or `None` when no version
    /// is close enough. Candidate pool is every `mc_version` plus the
    /// `"latest"` alias, since the alias is just as legitimate a `--target`
    /// value as any version string.
    fn suggestion_for(&self, requested: &str) -> Option<String> {
        let pool = self
            .data_versions
            .versions
            .iter()
            .map(|e| e.mc_version.as_str())
            .chain(std::iter::once("latest"));
        nearest_match(requested, pool).map(str::to_owned)
    }

    /// Human-readable list of `mc_version` strings the pack supports,
    /// followed by the `"latest"` alias. Used in
    /// [`crate::data_version::UnsupportedTarget`]'s error message.
    #[must_use]
    pub fn supported_list(&self) -> String {
        let mut entries: Vec<String> = self
            .data_versions
            .versions
            .iter()
            .map(|e| e.mc_version.clone())
            .collect();
        entries.push("latest".to_owned());
        entries.join(", ")
    }
}

/// Built-in Java pack, parsed once per process. Lookups against
/// [`crate::data_version`] go through this, so the static JSON is loaded
/// at most once even when many `cairn compile` runs happen inside a single
/// `cargo test` invocation.
///
/// # Panics
///
/// Panics if the embedded JSON fails to parse or validate. That is a
/// release-process invariant — the bundled files are part of the binary
/// — so a failure here means the build artefact itself is broken; the
/// `expect` surfaces that rather than papering it over.
pub fn builtin_java() -> &'static RegistryPack {
    static PACK: OnceLock<RegistryPack> = OnceLock::new();
    PACK.get_or_init(|| {
        load_builtin_java()
            .expect("built-in registry pack failed to load — this is a build invariant")
    })
}

/// Parse the built-in Java pack from its embedded bytes.
///
/// # Errors
///
/// Returns [`RegistryError`] if the embedded JSON fails to deserialise,
/// declares an unsupported schema, or fails validation. In practice all
/// of these would mean the bundled `data/registry/java/*.json` files have
/// been corrupted, which is a release-process bug.
pub fn load_builtin_java() -> Result<RegistryPack, RegistryError> {
    let manifest = parse_manifest(BUILTIN_JAVA_MANIFEST)?;
    validate_manifest(&manifest, PackEdition::Java)?;
    let data_versions = parse_data_versions(BUILTIN_JAVA_DATA_VERSIONS)?;
    validate_data_versions(&data_versions)?;
    let materials = if manifest.files.materials.is_some() {
        let catalog = parse_materials(BUILTIN_JAVA_MATERIALS)?;
        MaterialsIndex::from_catalog(catalog)?
    } else {
        MaterialsIndex::empty()
    };
    let mut components: Vec<(&str, &[u8])> = vec![(
        manifest.files.data_versions.as_str(),
        BUILTIN_JAVA_DATA_VERSIONS.as_bytes(),
    )];
    if let Some(name) = manifest.files.materials.as_deref() {
        components.push((name, BUILTIN_JAVA_MATERIALS.as_bytes()));
    }
    let bytes_hash = pack_hash(BUILTIN_JAVA_MANIFEST.as_bytes(), &components);
    Ok(RegistryPack {
        manifest,
        data_versions,
        materials,
        bytes_hash,
        source: PackSource::Builtin,
    })
}

/// Load a pack from a directory laid out the same way as the built-in
/// (`pack.json` + the files it names).
///
/// # Errors
///
/// Returns [`RegistryError`] on I/O failure, JSON parse failure, unsupported
/// schema versions, or validation failure of the loaded data.
pub fn load_from_dir(dir: &Path) -> Result<RegistryPack, RegistryError> {
    load_from_dir_inner(dir, PackEdition::Java)
}

fn load_from_dir_inner(
    dir: &Path,
    expected_edition: PackEdition,
) -> Result<RegistryPack, RegistryError> {
    let manifest_path = dir.join("pack.json");
    let manifest_bytes = std::fs::read(&manifest_path).map_err(|source| RegistryError::Io {
        path: manifest_path.clone(),
        source,
    })?;
    let manifest_text = std::str::from_utf8(&manifest_bytes).map_err(|err| RegistryError::Io {
        path: manifest_path.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, err),
    })?;
    let manifest = parse_manifest(manifest_text)?;
    validate_manifest(&manifest, expected_edition)?;

    let data_versions_path = dir.join(&manifest.files.data_versions);
    let data_versions_bytes =
        std::fs::read(&data_versions_path).map_err(|source| RegistryError::Io {
            path: data_versions_path.clone(),
            source,
        })?;
    let data_versions_text =
        std::str::from_utf8(&data_versions_bytes).map_err(|err| RegistryError::Io {
            path: data_versions_path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, err),
        })?;
    let data_versions = parse_data_versions(data_versions_text)?;
    validate_data_versions(&data_versions)?;

    let (materials, materials_bytes) = match manifest.files.materials.as_deref() {
        Some(name) => {
            let path = dir.join(name);
            let bytes = std::fs::read(&path).map_err(|source| RegistryError::Io {
                path: path.clone(),
                source,
            })?;
            let text = std::str::from_utf8(&bytes).map_err(|err| RegistryError::Io {
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, err),
            })?;
            let catalog = parse_materials(text)?;
            (MaterialsIndex::from_catalog(catalog)?, Some(bytes))
        }
        None => (MaterialsIndex::empty(), None),
    };

    let mut components: Vec<(&str, &[u8])> = vec![(
        manifest.files.data_versions.as_str(),
        data_versions_bytes.as_slice(),
    )];
    if let (Some(name), Some(bytes)) = (
        manifest.files.materials.as_deref(),
        materials_bytes.as_deref(),
    ) {
        components.push((name, bytes));
    }
    let bytes_hash = pack_hash(&manifest_bytes, &components);
    Ok(RegistryPack {
        manifest,
        data_versions,
        materials,
        bytes_hash,
        source: PackSource::Path(dir.to_path_buf()),
    })
}

fn parse_manifest(s: &str) -> Result<PackManifest, RegistryError> {
    serde_json::from_str(s).map_err(|source| RegistryError::Manifest { source })
}

fn parse_data_versions(s: &str) -> Result<DataVersionTable, RegistryError> {
    serde_json::from_str(s).map_err(|source| RegistryError::File {
        file: "data_versions".to_owned(),
        source,
    })
}

fn parse_materials(s: &str) -> Result<MaterialsCatalog, RegistryError> {
    serde_json::from_str(s).map_err(|source| RegistryError::File {
        file: "materials".to_owned(),
        source,
    })
}

fn validate_manifest(manifest: &PackManifest, expected: PackEdition) -> Result<(), RegistryError> {
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
    if !table.versions.iter().any(|e| e.mc_version == table.latest) {
        return Err(RegistryError::LatestNotInTable {
            latest: table.latest.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pack(tmp: &Path, manifest_json: &str, data_versions_json: &str) {
        std::fs::create_dir_all(tmp).expect("mkdir");
        std::fs::write(tmp.join("pack.json"), manifest_json).expect("write manifest");
        std::fs::write(tmp.join("data_versions.json"), data_versions_json)
            .expect("write data_versions");
    }

    fn write_pack_with_materials(
        tmp: &Path,
        manifest_json: &str,
        data_versions_json: &str,
        materials_json: &str,
    ) {
        write_pack(tmp, manifest_json, data_versions_json);
        std::fs::write(tmp.join("materials.json"), materials_json).expect("write materials");
    }

    fn good_manifest() -> &'static str {
        r#"{
            "schema_version": 1,
            "edition": "java",
            "name": "test",
            "description": "test",
            "files": { "data_versions": "data_versions.json" }
        }"#
    }

    fn good_data_versions() -> &'static str {
        r#"{
            "schema_version": 1,
            "latest": "1.21.4",
            "versions": [
                { "mc_version": "1.20.4", "data_version": 3700 },
                { "mc_version": "1.21.4", "data_version": 4189 }
            ]
        }"#
    }

    #[test]
    fn load_builtin_java_succeeds() {
        let pack = load_builtin_java().expect("builtin pack");
        assert_eq!(pack.manifest.edition, PackEdition::Java);
        assert_eq!(pack.manifest.name, "cairn-builtin-java");
        assert_eq!(pack.source, PackSource::Builtin);
        assert!(!pack.data_versions.versions.is_empty());
    }

    #[test]
    fn builtin_pack_includes_materials_catalog() {
        let pack = load_builtin_java().expect("builtin pack");
        assert!(
            !pack.materials.is_empty(),
            "built-in pack ships a non-empty materials catalog",
        );
        assert_eq!(
            pack.materials.lookup_id("floor.wood.broadleaf"),
            Some("minecraft:oak_planks"),
        );
        assert_eq!(
            pack.materials.lookup_id("wall.stone.cobble"),
            Some("minecraft:cobblestone"),
        );
        assert_eq!(
            pack.materials.lookup_id("wood.dark"),
            Some("minecraft:dark_oak_planks"),
        );
        assert_eq!(
            pack.materials.lookup_id("roof.dark_wood"),
            Some("minecraft:dark_oak_stairs"),
        );
    }

    #[test]
    fn pack_without_materials_component_loads_with_empty_index() {
        let tmp = std::env::temp_dir().join("cairn-registry-no-materials");
        let _ = std::fs::remove_dir_all(&tmp);
        // `good_manifest()` already omits the `materials` key, so this
        // exercises the backwards-compatible path that older packs ride on.
        write_pack(&tmp, good_manifest(), good_data_versions());
        let pack = load_from_dir(&tmp).expect("load");
        assert!(pack.materials.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pack_with_materials_component_lifts_tokens() {
        let tmp = std::env::temp_dir().join("cairn-registry-materials");
        let _ = std::fs::remove_dir_all(&tmp);
        let manifest = r#"{
            "schema_version": 1,
            "edition": "java",
            "name": "test",
            "description": "test",
            "files": {
                "data_versions": "data_versions.json",
                "materials": "materials.json"
            }
        }"#;
        let materials = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "token": "wall.stone.cobble", "block": "cobblestone" }
            ]
        }"#;
        write_pack_with_materials(&tmp, manifest, good_data_versions(), materials);
        let pack = load_from_dir(&tmp).expect("load");
        assert_eq!(
            pack.materials.lookup_id("wall.stone.cobble"),
            Some("minecraft:cobblestone"),
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pack_with_duplicate_materials_token_is_error() {
        let tmp = std::env::temp_dir().join("cairn-registry-dup-materials");
        let _ = std::fs::remove_dir_all(&tmp);
        let manifest = r#"{
            "schema_version": 1,
            "edition": "java",
            "name": "test",
            "description": "test",
            "files": {
                "data_versions": "data_versions.json",
                "materials": "materials.json"
            }
        }"#;
        let materials = r#"{
            "schema_version": 1,
            "namespace": "minecraft",
            "entries": [
                { "token": "x.y", "block": "stone" },
                { "token": "x.y", "block": "dirt" }
            ]
        }"#;
        write_pack_with_materials(&tmp, manifest, good_data_versions(), materials);
        let err = load_from_dir(&tmp).expect_err("duplicate token");
        assert!(matches!(
            err,
            RegistryError::Materials {
                source: MaterialsError::DuplicateMaterialEntry { ref token },
            } if token == "x.y"
        ));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn builtin_pack_resolves_known_targets() {
        let pack = load_builtin_java().expect("builtin pack");
        let t = pack.resolve_java_target("1.20.4").expect("1.20.4");
        assert_eq!(t.mc_version, "1.20.4");
        assert_eq!(t.data_version, 3700);
        let t = pack.resolve_java_target("1.21").expect("1.21");
        assert_eq!(t.data_version, 3953);
        let t = pack.resolve_java_target("1.21.4").expect("1.21.4");
        assert_eq!(t.data_version, 4189);
    }

    #[test]
    fn builtin_pack_resolves_latest_alias() {
        let pack = load_builtin_java().expect("builtin pack");
        let latest = pack.resolve_java_target("latest").expect("latest");
        assert_eq!(latest.mc_version, pack.data_versions.latest);
    }

    #[test]
    fn builtin_pack_rejects_unknown_target() {
        let pack = load_builtin_java().expect("builtin pack");
        let err = pack
            .resolve_java_target("0.0.0")
            .expect_err("unknown target");
        assert_eq!(err.requested, "0.0.0");
        assert!(err.supported.contains("1.20.4"));
        assert!(err.supported.contains("latest"));
        // `0.0.0` is far from every supported version; the suggestion
        // field must stay None so the error reads as a plain "no match".
        assert!(err.suggestion.is_none());
    }

    #[test]
    fn builtin_pack_suggests_near_target() {
        // `1.21.5` is one substitution away from `1.21.4` — exactly the
        // kind of typo the suggest pass is here to catch.
        let pack = load_builtin_java().expect("builtin pack");
        let err = pack
            .resolve_java_target("1.21.5")
            .expect_err("unknown target");
        assert_eq!(err.suggestion.as_deref(), Some("1.21.4"));
    }

    #[test]
    fn pack_hash_is_deterministic() {
        let a = load_builtin_java().expect("a");
        let b = load_builtin_java().expect("b");
        assert_eq!(a.bytes_hash, b.bytes_hash);
    }

    #[test]
    fn load_from_dir_reports_missing_manifest() {
        let tmp = std::env::temp_dir().join("cairn-registry-load-missing");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("mkdir");
        let err = load_from_dir(&tmp).expect_err("missing manifest");
        assert!(matches!(err, RegistryError::Io { .. }));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_from_dir_reads_a_complete_pack() {
        let tmp = std::env::temp_dir().join("cairn-registry-load-complete");
        let _ = std::fs::remove_dir_all(&tmp);
        write_pack(&tmp, good_manifest(), good_data_versions());
        let pack = load_from_dir(&tmp).expect("load");
        assert!(matches!(pack.source, PackSource::Path(ref p) if p == &tmp));
        let t = pack.resolve_java_target("1.21.4").expect("resolve");
        assert_eq!(t.data_version, 4189);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unsupported_manifest_schema_version_is_error() {
        let tmp = std::env::temp_dir().join("cairn-registry-bad-manifest-schema");
        let _ = std::fs::remove_dir_all(&tmp);
        let bad = r#"{
            "schema_version": 999,
            "edition": "java",
            "name": "x",
            "description": "x",
            "files": { "data_versions": "data_versions.json" }
        }"#;
        write_pack(&tmp, bad, good_data_versions());
        let err = load_from_dir(&tmp).expect_err("bad schema");
        assert!(matches!(
            err,
            RegistryError::UnsupportedSchemaVersion {
                kind: "manifest",
                got: 999,
                ..
            }
        ));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unsupported_data_versions_schema_is_error() {
        let tmp = std::env::temp_dir().join("cairn-registry-bad-table-schema");
        let _ = std::fs::remove_dir_all(&tmp);
        let bad = r#"{
            "schema_version": 999,
            "latest": "1.21.4",
            "versions": [ { "mc_version": "1.21.4", "data_version": 4189 } ]
        }"#;
        write_pack(&tmp, good_manifest(), bad);
        let err = load_from_dir(&tmp).expect_err("bad schema");
        assert!(matches!(
            err,
            RegistryError::UnsupportedSchemaVersion {
                kind: "data_versions",
                got: 999,
                ..
            }
        ));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn empty_version_table_is_error() {
        let tmp = std::env::temp_dir().join("cairn-registry-empty");
        let _ = std::fs::remove_dir_all(&tmp);
        let empty = r#"{ "schema_version": 1, "latest": "1.21.4", "versions": [] }"#;
        write_pack(&tmp, good_manifest(), empty);
        let err = load_from_dir(&tmp).expect_err("empty");
        assert!(matches!(err, RegistryError::EmptyVersionTable));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn latest_not_in_table_is_error() {
        let tmp = std::env::temp_dir().join("cairn-registry-dangling-latest");
        let _ = std::fs::remove_dir_all(&tmp);
        let dangling = r#"{
            "schema_version": 1,
            "latest": "9.9.9",
            "versions": [ { "mc_version": "1.21.4", "data_version": 4189 } ]
        }"#;
        write_pack(&tmp, good_manifest(), dangling);
        let err = load_from_dir(&tmp).expect_err("dangling latest");
        assert!(matches!(err, RegistryError::LatestNotInTable { ref latest } if latest == "9.9.9"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bedrock_manifest_into_java_loader_is_edition_mismatch() {
        let tmp = std::env::temp_dir().join("cairn-registry-edition-mismatch");
        let _ = std::fs::remove_dir_all(&tmp);
        let bedrock = r#"{
            "schema_version": 1,
            "edition": "bedrock",
            "name": "x",
            "description": "x",
            "files": { "data_versions": "data_versions.json" }
        }"#;
        write_pack(&tmp, bedrock, good_data_versions());
        let err = load_from_dir(&tmp).expect_err("mismatch");
        assert!(matches!(
            err,
            RegistryError::EditionMismatch {
                expected: PackEdition::Java,
                got: PackEdition::Bedrock,
            }
        ));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn malformed_manifest_json_is_error() {
        let tmp = std::env::temp_dir().join("cairn-registry-bad-manifest-json");
        let _ = std::fs::remove_dir_all(&tmp);
        write_pack(&tmp, "{ not json", good_data_versions());
        let err = load_from_dir(&tmp).expect_err("bad json");
        assert!(matches!(err, RegistryError::Manifest { .. }));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pack_hash_snapshot() {
        let pack = load_builtin_java().expect("builtin pack");
        insta::assert_snapshot!("builtin_java_pack_hash", pack.bytes_hash.as_str());
    }
}
