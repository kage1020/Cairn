//! Load a [`RegistryPack`] from the bundled built-in or from a directory.
//!
//! The built-in pack is embedded at compile time via `include_str!` so the
//! Cairn binary never has to find a data file at runtime. [`load_from_dir`]
//! reads a pack directory through the same validation.

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

/// One embedded pack's files, included at compile time so the Cairn
/// binary never has to find a data file at runtime.
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

/// Highest manifest `schema_version` this Cairn build understands.
pub const SUPPORTED_MANIFEST_SCHEMA: u32 = 1;
/// Highest `data_versions.schema_version` this Cairn build understands.
///
/// Version 2 adds the `targetable` column. A `schema_version: 1` table
/// still loads, every row targetable; a v1 *reader* must refuse a v2 table,
/// or it would offer the ordering rows as `--target` values.
pub const SUPPORTED_DATA_VERSIONS_SCHEMA: u32 = 2;

/// Where a [`RegistryPack`] came from. Surfaced in diagnostics so a user
/// can tell which directory was read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackSource {
    /// The pack bundled with the Cairn binary via `include_str!`.
    Builtin,
    /// A pack loaded from a directory on disk.
    Path(PathBuf),
}

/// A loaded and validated registry pack.
///
/// `#[non_exhaustive]` so no caller outside this crate can assemble one
/// that skipped the loader's validators (the `data_versions` table, the
/// per-version block tables, the material overrides), which every lookup
/// below assumes. The fields stay readable.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RegistryPack {
    /// Top-level manifest.
    pub manifest: PackManifest,
    /// `(mc_version, data_version)` table.
    pub data_versions: DataVersionTable,
    /// Abstract material catalog. Empty when the pack's manifest declares
    /// no `materials` component.
    pub materials: MaterialsIndex,
    /// Per-version block-id tables. Empty when the pack omits the
    /// component, which turns id validation off rather than making every
    /// id invalid.
    pub blocks: BlocksIndex,
    /// Groups of spellings one block has worn. Empty when the pack omits
    /// the component, which leaves a refused id answered by the typo
    /// search alone.
    pub aliases: AliasIndex,
    /// `sha256:<hex>` over the manifest + component bytes in declared order.
    /// Lands in the lockfile under `inputs.registry_pack_hash`.
    pub bytes_hash: HashHex,
    /// Provenance of the pack.
    pub source: PackSource,
}

/// One registry pack, seen through the target a single compile pinned.
///
/// This is the type block-array lowering actually talks to. It exists
/// because "the pack" is edition-wide while a compile is pinned to one
/// version, and the id table is the part that differs between them: Bedrock
/// 1.21.0 spells stone bricks `stonebrick` and 1.21.40 spells it
/// `stone_bricks`, so a pack-wide answer to "does this id exist" would be
/// wrong for one of them.
#[derive(Debug, Clone)]
pub struct PackView<'a> {
    /// The pack's abstract-token catalog.
    materials: &'a MaterialsIndex,
    /// The pack's alias groups. Edition-wide and version-free by design —
    /// the pinned version's id table below is what narrows a group to the
    /// spellings this target actually declares.
    aliases: &'a AliasIndex,
    /// The pinned version, or `None` when the run pinned none. Read by
    /// both halves of the view: a token may be respelled at this version,
    /// and the id table belongs to it.
    mc_version: Option<&'a str>,
    /// Sorted ids for the pinned version, or `None` when nothing is pinned
    /// (or the pack ships no `blocks` component).
    ids: Option<&'a [String]>,
    /// How the pinned target reads in a diagnostic — `"bedrock 1.21.60"`,
    /// or just `"bedrock"` when no version is pinned.
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
        // Unpinned, there is no table to filter the group by, and an
        // unfiltered group would suggest spellings from other versions and
        // the other edition. Unpinned commands raise no `E_UNKNOWN_ID`.
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
    /// does not understand, reported as such rather than as an opaque
    /// "missing field".
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
    /// `data_versions.latest` did not name a **targetable** row of
    /// `versions`. A dangling `latest` would resolve `--target latest` to
    /// nothing; one naming an ordering row would resolve it to a version
    /// the pack cannot build for. Both are pack-author bugs rather than
    /// user errors.
    #[error("registry pack `data_versions.latest` = `{latest}` is not a targetable version")]
    LatestNotInTable {
        /// Verbatim `latest` value.
        latest: String,
    },
    /// Every row of `data_versions.versions` is an ordering row, so the
    /// pack can order a floor and build for nothing. Refused here rather
    /// than at the first `--target`, which would report every version the
    /// table names as unsupported and none as valid.
    #[error("registry pack `data_versions.versions` has no targetable version")]
    NoTargetableVersion,
    /// The version table's rows are not ordered the way every lookup in
    /// this module assumes. Holds the row pair and which of the two
    /// properties they break.
    #[error("registry pack `data_versions.versions` is not a usable ordering: {reason}")]
    VersionOrderBroken {
        /// Which rows disagree, and about what.
        reason: String,
    },
    /// The manifest declared an edition that does not match the slot the
    /// pack was loaded into (e.g. a Bedrock pack handed to the Java
    /// loader), before any downstream code looks at the data.
    #[error("registry pack edition `{got:?}` does not match expected `{expected:?}`")]
    EditionMismatch {
        /// Expected edition.
        expected: Edition,
        /// Edition the manifest declared.
        got: Edition,
    },
    /// The pack's `materials` catalog failed to validate.
    #[error("registry pack `materials`: {source}")]
    Materials {
        /// Underlying validation error from the materials catalog.
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
        /// Underlying validation error from the blocks table.
        #[source]
        source: BlocksError,
    },
    /// The pack's `aliases` catalog failed to validate.
    #[error("registry pack `aliases`: {source}")]
    Aliases {
        /// Underlying validation error from the alias catalog.
        #[source]
        source: AliasError,
    },
    /// An alias group named no id any version of the pack's own `blocks`
    /// tables declares.
    ///
    /// Such a group can never answer: a lookup keeps the members the
    /// pinned target declares, and this one has none in any target. It is
    /// what a misspelt spelling looks like from the loader's side.
    #[error(
        "registry pack aliases group [{spellings}] names no id any version of `blocks` declares"
    )]
    AliasGroupUnanswerable {
        /// The group's spellings, comma-joined.
        spellings: String,
    },
    /// The `blocks` component and the `data_versions` table describe
    /// different sets of versions.
    ///
    /// A supported target with no id table would compile without any id
    /// checked, which looks exactly like a clean build; an id table for a
    /// target `--target` cannot name is dead data that nobody will notice
    /// has gone stale. Both are pack-author mistakes, so the pack is
    /// refused instead of half-checked.
    #[error(
        "registry pack `blocks` and `data_versions` disagree: no id table for [{missing}], and an id table for unsupported [{extra}]"
    )]
    BlocksVersionMismatch {
        /// Versions `data_versions` declares that `blocks` does not,
        /// comma-joined.
        missing: String,
        /// Versions `blocks` declares that `data_versions` does not,
        /// comma-joined.
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

/// The buildable row a `--target` value names, as the owned
/// `(mc_version, data_version)` pair the per-edition target types are
/// built from.
///
/// Only `targetable` rows are candidates. The table also carries the
/// releases the pack can *order against* but has no block data for, and
/// resolving `--target` to one of those would pin a compile to a version
/// whose id table is absent — which turns the `E_UNKNOWN_ID` check off
/// rather than running it, the silent-substitution hazard
/// `spec/versioning-editions` "Fail-loud and minimum-version inference"
/// forbids.
///
/// A row is named by any label equal to its own under the comparison
/// [`VersionOrder::key_of`](cairn_lang_core::resolve::VersionOrder::key_of)
/// uses, which states the rule in full; the two lookups must agree. `1.21.0`
/// names Java's `1.21`, while `1.21.4` is not Bedrock's `1.21.40`.
/// [`compare_versions`] is not the ordering key, but `.is_eq()` asks it no
/// ordering question. `spec/versioning-editions` "The target is a
/// compile-time parameter" defines the same version this way for
/// `--target`, `@requires` and `@intended_targets` alike.
///
/// The pair carries the row's own label, so the target and the lockfile
/// name the version the way the table does. `validate_version_order`
/// refuses a table carrying two spellings of one version, so at most one
/// row matches.
fn targetable_row_for(table: &DataVersionTable, mc_version: &str) -> Option<(String, i32)> {
    table
        .versions
        .iter()
        .filter(|e| e.targetable)
        .find(|e| compare_versions(&e.mc_version, mc_version).is_eq())
        .map(|e| (e.mc_version.clone(), e.data_version))
}

impl RegistryPack {
    /// Resolve a CLI `--target` value against this pack's version table,
    /// returning the raw `(mc_version, data_version)` row. The literal
    /// `"latest"` aliases the row named by `DataVersionTable::latest`; the
    /// per-edition wrappers stamp the pair into their edition's target
    /// type.
    ///
    /// # Errors
    ///
    /// Returns [`UnsupportedTarget`] when the requested string is neither
    /// the `"latest"` alias nor a label equal to a targetable `mc_version`
    /// under the comparison
    /// [`VersionOrder::key_of`](cairn_lang_core::resolve::VersionOrder::key_of)
    /// uses.
    ///
    /// # Panics
    ///
    /// Panics when the pack is not of `edition`. Both editions share the
    /// `data_version` column with different meanings, so resolving against
    /// the wrong pack would return a plausible-but-wrong integer, the
    /// silent substitution `spec/versioning-editions` "Fail-loud and
    /// minimum-version inference" rules out. A full `assert!` rather than a
    /// `debug_assert!`, since resolution runs once per compile.
    ///
    /// Also panics if `latest` names no targetable row, which
    /// [`validate_data_versions`] rules out at load time.
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

    /// Resolve a CLI `--target` value into a Java backend target.
    ///
    /// # Errors
    ///
    /// Returns [`UnsupportedTarget`] when the requested string is neither
    /// the `"latest"` alias nor a label equal to a targetable `mc_version`
    /// under the comparison
    /// [`VersionOrder::key_of`](cairn_lang_core::resolve::VersionOrder::key_of)
    /// uses.
    ///
    /// # Panics
    ///
    /// Panics if called on a non-Java pack — resolving against the wrong
    /// edition's `data_version` column would return a meaningful-but-wrong
    /// integer, so it is a hard caller-bug guard rather than a recoverable
    /// error.
    pub fn resolve_java_target(&self, requested: &str) -> Result<JavaTarget, UnsupportedTarget> {
        let (mc_version, data_version) = self.resolve_target_row(Edition::Java, requested)?;
        Ok(JavaTarget {
            mc_version,
            data_version,
        })
    }

    /// Resolve a CLI `--target` value into a Bedrock backend target. The
    /// pack's `data_version` column carries the `.mcstructure` block
    /// palette `version` integer for Bedrock packs.
    ///
    /// # Errors
    ///
    /// Returns [`UnsupportedTarget`] when the requested string is neither
    /// the `"latest"` alias nor a label equal to a targetable `mc_version`
    /// under the comparison
    /// [`VersionOrder::key_of`](cairn_lang_core::resolve::VersionOrder::key_of)
    /// uses.
    ///
    /// # Panics
    ///
    /// Panics if called on a non-Bedrock pack, for the same caller-bug
    /// reason as [`Self::resolve_java_target`].
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

    /// Closest supported version for the matching field of
    /// [`crate::data_version::UnsupportedTarget`], or `None` when no version
    /// is close enough. Candidate pool is every targetable `mc_version` plus
    /// the `"latest"` alias, since the alias is just as legitimate a
    /// `--target` value as any version string. The rows the pack only orders
    /// against are left out: no spelling of one resolves.
    fn suggestion_for(&self, requested: &str) -> Option<String> {
        let pool = self
            .targetable()
            .into_iter()
            .map(|e| e.mc_version.as_str())
            .chain(std::iter::once("latest"));
        nearest_match(requested, pool).map(str::to_owned)
    }

    /// The rows `--target` may name, ascending by `data_version`.
    ///
    /// The buildable rows, not every row: the table also carries the
    /// releases the pack can order an `@requires` floor against but has no
    /// block table for, and offering one of those as a `--target` would pin
    /// a compile to a version whose id check cannot run.
    ///
    /// Sorted rather than taken as written, so a list shown to a reader
    /// does not follow the order a pack's JSON happens to list its rows in.
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

    /// The whole version table as the ordering
    /// `spec/versioning-editions` "The target is a compile-time parameter"
    /// makes canonical, which is what an `@requires` floor is placed
    /// against.
    ///
    /// **Every** row, including the ones `--target` may not name: building
    /// for a version and ordering a floor against it are different
    /// questions, and whether a floor can be placed at all depends on where
    /// every release sits.
    #[must_use]
    pub fn version_order(&self) -> VersionOrder {
        VersionOrder::new(
            self.data_versions
                .versions
                .iter()
                .map(|entry| (entry.mc_version.clone(), i64::from(entry.data_version))),
        )
    }

    /// The view of this pack that block-array lowering consults.
    ///
    /// `mc_version` is the target the run pinned, or `None` for a run that
    /// pinned none, whose target did not resolve, or whose `@requires`
    /// floors refuse it; none of those writes an artifact. `None` leaves
    /// [`TargetRegistry::block_ids`] empty, so id validation is skipped
    /// rather than run against a version the caller never chose or one the
    /// source disowns.
    ///
    /// # Panics
    ///
    /// Panics when the pack has block tables but none for `mc_version`, or
    /// when the one it has is not sorted. Both are impossible for a pack
    /// that came through the loader (`validate_blocks_cover_versions`
    /// refuses the first and `BlocksIndex` builds sorted), and the
    /// alternative is a compile that checks no ids while looking clean.
    #[must_use]
    pub fn view<'a>(&'a self, mc_version: Option<&'a str>) -> PackView<'a> {
        let ids = mc_version.and_then(|version| {
            let ids = self.blocks.ids_for(version);
            // INVARIANT(blocks-cover-versions): `validate_blocks_cover_versions`
            // refuses a pack whose `blocks` and `data_versions` name
            // different versions, and every caller resolves `mc_version`
            // through `data_versions`. A full `assert!`: a silently skipped
            // id check must not reach a release build, and this costs one
            // lookup per compile.
            assert!(
                ids.is_some() || self.blocks.is_empty(),
                "pack `{}` has block tables but none for `{version}`",
                self.manifest.name,
            );
            // `BlockIdSet::contains` binary-searches, so an unsorted table
            // would report real ids as missing; checked once here rather
            // than per lookup.
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

    /// Human-readable list of `mc_version` strings the pack supports,
    /// followed by the `"latest"` alias. Used in
    /// [`crate::data_version::UnsupportedTarget`]'s error message.
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
/// Panics if the embedded JSON fails to parse or validate, which means the
/// build artefact itself is broken.
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

/// Resolve a `--target` string against the built-in registry pack. The
/// literal `"latest"` aliases the row named by `data_versions.latest`,
/// matching the working assumption that "no explicit `--target`" means
/// "the newest supported".
///
/// # Errors
///
/// Returns [`UnsupportedTarget`] when the requested string is neither a
/// label equal to a targetable `mc_version` under the comparison
/// [`VersionOrder::key_of`](cairn_lang_core::resolve::VersionOrder::key_of)
/// uses, nor the `"latest"` alias.
pub fn resolve_java_target(requested: &str) -> Result<JavaTarget, UnsupportedTarget> {
    builtin_pack(Edition::Java).resolve_java_target(requested)
}

/// Resolve a `--target` string against the built-in Bedrock registry pack.
/// The literal `"latest"` aliases the row named by `data_versions.latest`,
/// matching [`resolve_java_target`].
///
/// # Errors
///
/// Returns [`UnsupportedTarget`] when the requested string is neither a
/// label equal to a targetable `mc_version` under the comparison
/// [`VersionOrder::key_of`](cairn_lang_core::resolve::VersionOrder::key_of)
/// uses, nor the `"latest"` alias.
pub fn resolve_bedrock_target(requested: &str) -> Result<BedrockTarget, UnsupportedTarget> {
    builtin_pack(Edition::Bedrock).resolve_bedrock_target(requested)
}

/// Parse the built-in pack for `edition` from its embedded bytes, for a caller that wants the
/// error rather than [`builtin_pack`]'s panic.
///
/// # Errors
///
/// Returns [`RegistryError`] if the embedded JSON fails to deserialise,
/// declares an unsupported schema, or fails validation. Any of these means
/// the bundled `registry-data` files are corrupt.
pub fn load_builtin_pack(edition: Edition) -> Result<RegistryPack, RegistryError> {
    let files = match edition {
        Edition::Java => BUILTIN_JAVA,
        Edition::Bedrock => BUILTIN_BEDROCK,
    };
    load_pack(&files, edition)
}

/// Load a pack from a directory laid out the same way as the built-in
/// (`pack.json` + the files it names).
///
/// # Errors
///
/// Returns [`RegistryError`] on I/O failure, JSON parse failure, unsupported
/// schema versions, or validation failure of the loaded data.
pub fn load_from_dir(dir: &Path) -> Result<RegistryPack, RegistryError> {
    load_pack(&PackDir(dir), Edition::Java)
}

/// The manifest and the four component files it can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Component {
    Manifest,
    DataVersions,
    Materials,
    Blocks,
    Aliases,
}

impl Component {
    /// The error a malformed component is reported as: the manifest has
    /// its own variant, the four files it names share one.
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

/// Where [`load_pack`] reads each file from. The embedded packs and a
/// directory on disk share one parse → validate → hash pipeline through
/// this, so a validation rule added for one can never silently miss the
/// other.
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

/// Read, parse and index one optional component, or hand back its empty
/// index when the manifest names no file for it. The text comes back too,
/// because the pack hash covers it.
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

/// Refuse an alias group none of the pack's own block tables can answer
/// with.
///
/// A group is resolved by keeping the members the pinned target declares,
/// so a group naming no id any version has is dead in every target — the
/// shape a misspelt spelling takes. A pack with no `blocks` component is
/// exempt for the same reason it is exempt from
/// [`validate_blocks_cover_versions`]: there is nothing to check against,
/// and reading that as "no group is answerable" would refuse every pack
/// that ships aliases without tables.
///
/// The rule is "some id in the group", not "every id": a group exists
/// precisely to hold the spellings this edition does *not* use beside the
/// ones it does, and a Java pack listing Bedrock's `standing_sign` is the
/// component working, not a mistake.
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

/// Refuse a materials override naming a version the pack does not support.
///
/// An override for `1.21.41` when the table declares `1.21.40` never fires:
/// the token keeps resolving to its default id on every version, which is
/// the exact silent-wrong-id outcome the overrides exist to prevent.
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

/// Refuse a pack whose `blocks` component and `data_versions` table do not
/// describe the same versions.
///
/// A pack with no `blocks` component at all is exempt: that is the
/// documented "this pack cannot check ids" mode, not a disagreement.
///
/// The `missing` half compares the **targetable** versions only: an
/// ordering row is never pinned, so it has no id table to be missing. The
/// `extra` half compares every row, because an id table for a version the
/// table does not name at all is still dead data.
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

/// The two properties every answer `VersionOrder` gives rests on.
///
/// **Keys are unique and ascending.** `DataVersion` is the ordering key,
/// so two rows sharing one are two versions the compiler cannot tell
/// apart, and a row out of order would make "the first row" something
/// other than the oldest release.
///
/// **Labels in one numbering order the same way by text as by key.** This
/// is the property that makes placing a floor *outside* the table's span
/// sound: that answer is reached by comparing the floor's label against
/// the first and last rows' labels, so the text has to agree with the keys
/// about which rows those are. One numbering scheme does not imply it (two
/// dotted decimals whose keys run the other way break it), so it is
/// checked rather than assumed.
///
/// Labels are compared with `cairn_lang_core`'s label comparison, so two
/// spellings of one version (`1.21` and `1.21.0`) count as a duplicate:
/// the lookup treats them as the same row, and a table carrying both
/// would evaluate a floor against whichever came first.
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
