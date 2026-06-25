//! `build.cairn.lock` YAML schema.
//!
//! Field order in [`Lockfile`] is deliberate: `serde_yml` writes structs in
//! declaration order, so this matches the sample in
//! `spec/versioning-editions.md` §10.6 byte-for-byte. Changing the order
//! breaks downstream tools that grep the lockfile, so it is also
//! exercised by an AC.

use serde::{Deserialize, Serialize};

use super::hash::HashHex;

/// The whole lockfile, as written to `build.cairn.lock`.
///
/// `verified: true` is the default after a successful compile; a future
/// `--no-verify` workflow will flip it to `false` and the same struct
/// shape will roundtrip without changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lockfile {
    /// sha256 over the raw `.crn` bytes.
    pub source_hash: HashHex,
    /// The Cairn release `CalVer` that produced the lockfile.
    pub cairn_version: String,
    /// Target edition + Minecraft version + `DataVersion`.
    pub target: LockTarget,
    /// Registry / catalog input hashes. Zero until those inputs are wired.
    pub inputs: LockInputs,
    /// sha256 over the lowered block-array IR. The core of reproducibility.
    pub resolved_ir_hash: HashHex,
    /// `true` for a successful build; reserved for a later opt-out flow.
    pub verified: bool,
    /// Members whose meaning may drift across a Minecraft version bump.
    /// Empty until the constraint catalog ingest lands.
    pub member_version_sensitivity: Vec<MemberSensitivity>,
    /// Per-`place` site coordinates resolved from the `at=` / `east_of=` /
    /// `north_of=` constraint chain. Empty for sources that declare no
    /// `site` block, which keeps the lockfile shape byte-identical to
    /// pre-M3-PR3 cottage / themed-tower builds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<LockPlacement>,
}

/// One `place` line resolved into absolute world-space coordinates.
///
/// Carries enough provenance (site / def / theme) to reproduce the
/// coordinate chain without re-walking the source: a downstream consumer can
/// rebuild the village layout straight from the lockfile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockPlacement {
    /// `site` name the placement belongs to (bare, without the `site::`
    /// IR-key prefix).
    pub site: String,
    /// `place id=` value.
    pub id: String,
    /// `place use=` target.
    pub def: String,
    /// `place theme=` target.
    pub theme: String,
    /// Absolute `(x, y, z)` origin in world voxels. Stored as `[i32; 3]` so
    /// `north_of` placements (negative `z`) round-trip without saturation.
    pub origin: [i32; 3],
    /// Voxel extents of the per-place [`super::super::block_array::BlockArray`]
    /// at the time of the build. Captured here so the lockfile alone is
    /// enough to compute the next-placement offset for an incremental
    /// re-resolve, without rehydrating every block array.
    pub dims: [u32; 3],
}

/// Backend edition the lockfile pins to.
///
/// A closed enum (rather than `String`) so a typo (`"jav"`,
/// `"BEDROCK"`) cannot ride along as a valid lockfile. The serde
/// representation is the lowercase identifier (`"java"` / `"bedrock"`),
/// matching the human-facing CLI flag values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockEdition {
    /// Java Edition.
    Java,
    /// Bedrock Edition.
    Bedrock,
}

impl LockEdition {
    /// Lowercase identifier (`"java"` / `"bedrock"`). Same string used in
    /// the YAML output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LockEdition::Java => "java",
            LockEdition::Bedrock => "bedrock",
        }
    }
}

/// `(edition, mc_version, data_version)` triple.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockTarget {
    /// Backend edition.
    pub edition: LockEdition,
    /// Human-facing Minecraft version, e.g. `"1.21.4"`.
    pub mc_version: String,
    /// Java `DataVersion` integer or Bedrock `block_version` once that
    /// backend lands.
    pub data_version: i32,
}

/// External-input hashes the build depends on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockInputs {
    /// sha256 of the registry pack that resolved the ids. Zero until the
    /// registry pack ingest is wired.
    pub registry_pack_hash: HashHex,
    /// sha256 of the constraint catalog. Zero until that catalog lands.
    pub constraint_catalog_hash: HashHex,
}

impl LockInputs {
    /// All-zero inputs (no registry / catalog yet).
    #[must_use]
    pub fn zero() -> Self {
        Self {
            registry_pack_hash: HashHex::zero(),
            constraint_catalog_hash: HashHex::zero(),
        }
    }
}

/// Member id flagged as sensitive to a Minecraft version boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemberSensitivity {
    /// Member id from the lowered IR.
    pub id: String,
    /// Human-readable reason, mirroring `cairn info`.
    pub reason: String,
}
