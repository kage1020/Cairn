//! `build.cairn.lock` YAML schema.
//!
//! Field order in [`Lockfile`] is deliberate: `serde_yml` writes structs in
//! declaration order, so this matches the sample in
//! `spec/versioning-editions.md` §10.6 byte-for-byte. Changing the order
//! breaks downstream tools that grep the lockfile, so it is also
//! exercised by an AC.

use serde::{Deserialize, Serialize};

use super::hash::HashHex;
use crate::ids::{PlaceId, SiteName, WalkwayEndpoint};

/// The lockfile schema this build reads and writes.
///
/// Version 1 is the shape that shipped before the number was recorded, so a
/// document without the field is that version rather than an unknown one —
/// refusing those would mean refusing files this compiler wrote. A document
/// declaring anything higher is refused rather than read as if the field
/// names still meant the same thing.
pub const LOCK_SCHEMA_VERSION: u32 = 1;

/// Default for a document written before the version was recorded.
fn assume_unversioned_schema() -> u32 {
    LOCK_SCHEMA_VERSION
}

/// The whole lockfile, as written to `build.cairn.lock`.
///
/// `verified: true` is the default after a successful compile; a future
/// `--no-verify` workflow will flip it to `false` and the same struct
/// shape will roundtrip without changes.
///
/// Every struct in this file denies unknown fields. A lockfile is a claim
/// about what was built, and one carrying keys the reader silently ignores
/// is a document whose meaning depends on who is reading it — a tampered
/// file used to deserialise as `Ok` with `verified: true` alongside
/// whatever else it liked. Tampering is not confined to the top level, so
/// neither is the check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    /// Which revision of this schema the document is written in. First
    /// field so a reader can decide whether it understands the rest without
    /// parsing it. See [`LOCK_SCHEMA_VERSION`].
    #[serde(default = "assume_unversioned_schema")]
    pub lock_schema_version: u32,
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
    /// cottage / themed-tower builds that pre-date the site surface.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<LockPlacement>,
    /// Per-`connect` walkway records. Each entry pins both ports, the
    /// world-space origin / dims of the walkway block array, and the
    /// canonical path material so the lockfile alone is enough to
    /// reproduce the strip without re-running the resolver. Empty for
    /// sources that declare no `connect` lines, preserving lockfile
    /// shape parity with cottage / themed-tower builds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub walkways: Vec<LockWalkway>,
}

/// One `place` line resolved into absolute world-space coordinates.
///
/// Carries enough provenance (site / def / theme) to reproduce the
/// coordinate chain without re-walking the source: a downstream consumer can
/// rebuild the village layout straight from the lockfile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockPlacement {
    /// `site` name the placement belongs to (bare, without the `site::`
    /// IR-key prefix).
    pub site: SiteName,
    /// `place id=` value.
    pub id: PlaceId,
    /// `place use=` target.
    pub def: String,
    /// The theme that governed this placement's materials.
    ///
    /// Usually the `place theme=` target verbatim, but not always: under a
    /// `--edition` pin the resolver binds that edition's variant of the
    /// named theme, which can be a different name (`W_THEME_VARIANT_REBOUND`
    /// reports it). What the artifact was built from is what is recorded.
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

/// One `connect from.port to to.port` row resolved into a walkway
/// strip's world-space origin, dims, and path material.
///
/// Mirrors [`LockPlacement`]'s shape so `(site, id, def, theme,
/// origin, dims)` becomes `(site, from, to, path_material, origin,
/// dims)` — same five disk axes, just oriented around the two ports
/// the row connects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockWalkway {
    /// `site` name the walkway belongs to (bare, no `site::` prefix).
    pub site: SiteName,
    /// `from` endpoint as a `{ place, port }` pair. Stored as a
    /// structured object rather than a `"PLACE.PORT"` string so a port
    /// id is never re-parsed from a `.`-separated token — the type
    /// boundary catches the silent disaster the string form risked.
    pub from: WalkwayEndpoint,
    /// `to` endpoint, same shape as [`Self::from`].
    pub to: WalkwayEndpoint,
    /// Canonical Minecraft id of the path material laid into the
    /// walkway (e.g. `minecraft:gravel`).
    pub path_material: String,
    /// Absolute `(x, y, z)` origin in world voxels.
    pub origin: [i32; 3],
    /// Voxel extents of the walkway block array.
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct MemberSensitivity {
    /// Member id from the lowered IR.
    pub id: String,
    /// Human-readable reason, mirroring `cairn info`.
    pub reason: String,
}
