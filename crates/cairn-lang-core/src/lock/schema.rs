//! The lockfile's YAML schema. Fields are written in declaration order, which matches the
//! sample in `spec/versioning-editions` "Provenance and lock".

use serde::de::{Error as _, Unexpected};
use serde::{Deserialize, Deserializer, Serialize};

use super::hash::HashHex;
use crate::calver::parse_language_version;
use crate::edition::Edition;
use crate::ids::{IdError, PlaceId, PortId, SiteName, WalkwayEndpoint};

/// The lockfile schema this build reads and writes; a document without the field is version 1.
pub const LOCK_SCHEMA_VERSION: u32 = 1;

/// The literal `1`, not [`LOCK_SCHEMA_VERSION`]: an unversioned document stays version 1 forever.
fn assume_unversioned_schema() -> u32 {
    1
}

/// A `lock_schema_version`, refusing `0`, which names no revision.
fn schema_revision<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version == 0 {
        return Err(D::Error::invalid_value(
            Unexpected::Unsigned(0),
            &"a `lock_schema_version` from 1, the first revision",
        ));
    }
    Ok(version)
}

/// A `cairn_version`, which is a release as `cairn --version` prints it.
fn release_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let version = String::deserialize(deserializer)?;
    if parse_language_version(&version).is_err() {
        return Err(D::Error::invalid_value(
            Unexpected::Str(&version),
            &"a `cairn_version` naming a Cairn release, `YYYY.M` or `YYYY.M.PATCH`",
        ));
    }
    Ok(version)
}

/// An `mc_version`, which `cairn info` prints inside one line of its report.
fn version_label<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let label = String::deserialize(deserializer)?;
    if label
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err(D::Error::invalid_value(
            Unexpected::Str(&label),
            &"an `mc_version` without control characters or line breaks",
        ));
    }
    Ok(label)
}

/// The version alone, read before `deny_unknown_fields` can call a newer format malformed.
#[derive(Deserialize)]
struct SchemaVersionProbe {
    #[serde(
        default = "assume_unversioned_schema",
        deserialize_with = "schema_revision"
    )]
    lock_schema_version: u32,
}

/// The schema revision `body` declares, whatever else it contains.
pub(super) fn declared_schema_version(body: &str) -> Result<u32, serde_norway::Error> {
    let probe: SchemaVersionProbe = serde_norway::from_str(body)?;
    Ok(probe.lock_schema_version)
}

/// What a lockfile valid but for an identifier the current rule refuses says about its build.
#[derive(Debug, Clone, PartialEq)]
pub struct RefusedIdentifierLock {
    /// The first refused identifier in field order, which need not be document order.
    pub error: IdError,
    /// sha256 over the raw `.crn` bytes the lock was written for.
    pub source_hash: HashHex,
    /// The Cairn release that wrote the lock.
    pub cairn_version: String,
    /// The target the lock records.
    pub target: LockTarget,
    /// The lock's `verified` field.
    pub verified: bool,
    /// The members the lock flagged as version-sensitive.
    pub member_version_sensitivity: Vec<MemberSensitivity>,
}

/// [`Lockfile`] with its seven identifier fields read as plain strings and nothing else loosened.
/// The tests' `mirror` stops compiling until a field added to a strict struct is added here.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockfileProbe {
    #[serde(
        default = "assume_unversioned_schema",
        deserialize_with = "schema_revision"
    )]
    lock_schema_version: u32,
    source_hash: HashHex,
    #[serde(deserialize_with = "release_version")]
    cairn_version: String,
    target: LockTarget,
    inputs: LockInputs,
    resolved_ir_hash: HashHex,
    verified: bool,
    member_version_sensitivity: Vec<MemberSensitivity>,
    #[serde(default)]
    placements: Vec<PlacementProbe>,
    #[serde(default)]
    walkways: Vec<WalkwayProbe>,
}

/// [`LockPlacement`], with `site` and `id` read as plain strings.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlacementProbe {
    site: String,
    id: String,
    def: String,
    theme: String,
    origin: [i32; 3],
    dims: [u32; 3],
}

/// [`LockWalkway`], with `site` and both endpoints read as plain strings.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct WalkwayProbe {
    site: String,
    from: EndpointProbe,
    to: EndpointProbe,
    path_material: String,
    origin: [i32; 3],
    dims: [u32; 3],
}

/// [`WalkwayEndpoint`], with both halves read as plain strings.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointProbe {
    place: String,
    port: String,
}

/// The first identifier the rule refuses, in field order, with what the rest of `body` records;
/// the caller gates the schema version.
pub(super) fn refused_identifier(
    body: &str,
) -> Result<Option<RefusedIdentifierLock>, serde_norway::Error> {
    let probe: LockfileProbe = serde_norway::from_str(body)?;
    let placements = probe.placements.iter().flat_map(|p| {
        [
            SiteName::new(p.site.as_str()).err(),
            PlaceId::new(p.id.as_str()).err(),
        ]
    });
    let walkways = probe.walkways.iter().flat_map(|w| {
        [
            SiteName::new(w.site.as_str()).err(),
            PlaceId::new(w.from.place.as_str()).err(),
            PortId::new(w.from.port.as_str()).err(),
            PlaceId::new(w.to.place.as_str()).err(),
            PortId::new(w.to.port.as_str()).err(),
        ]
    });
    let Some(error) = placements.chain(walkways).flatten().next() else {
        return Ok(None);
    };
    Ok(Some(RefusedIdentifierLock {
        error,
        source_hash: probe.source_hash,
        cairn_version: probe.cairn_version,
        target: probe.target,
        verified: probe.verified,
        member_version_sensitivity: probe.member_version_sensitivity,
    }))
}

/// The whole lockfile document.
///
/// It and every type it nests deny unknown fields, so a tampered document cannot read as verified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    /// Schema revision, first so a reader can check it before the rest.
    #[serde(
        default = "assume_unversioned_schema",
        deserialize_with = "schema_revision"
    )]
    pub lock_schema_version: u32,
    /// sha256 over the raw `.crn` bytes.
    pub source_hash: HashHex,
    /// The Cairn release `CalVer` that produced the lockfile.
    #[serde(deserialize_with = "release_version")]
    pub cairn_version: String,
    /// Target edition, Minecraft version and data version.
    pub target: LockTarget,
    /// Registry and catalog input hashes, zero while those inputs are not hashed.
    pub inputs: LockInputs,
    /// sha256 over the lowered block-array IR.
    pub resolved_ir_hash: HashHex,
    /// `true` for a successful build.
    pub verified: bool,
    /// Members whose meaning may drift across a Minecraft version bump.
    pub member_version_sensitivity: Vec<MemberSensitivity>,
    /// Resolved coordinates per `place` row; omitted when the source has no `site` block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<LockPlacement>,
    /// Resolved walkway per `connect` row; omitted when the source has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub walkways: Vec<LockWalkway>,
}

/// One `place` row resolved into absolute world-space coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockPlacement {
    /// The `site` name, without the `site::` key prefix.
    pub site: SiteName,
    /// `place id=` value.
    pub id: PlaceId,
    /// `place use=` target.
    pub def: String,
    /// The theme bound, which under an `--edition` pin can be a variant of the `theme=` name.
    pub theme: String,
    /// Absolute `(x, y, z)` origin in world voxels.
    pub origin: [i32; 3],
    /// Voxel extents of the placement's block array, roof `overhang=` included.
    pub dims: [u32; 3],
}

/// One `connect` row resolved into a walkway strip's world-space origin, dims and path material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockWalkway {
    /// The `site` name, without the `site::` key prefix.
    pub site: SiteName,
    /// `from` endpoint.
    pub from: WalkwayEndpoint,
    /// `to` endpoint.
    pub to: WalkwayEndpoint,
    /// Canonical id of the path material, e.g. `minecraft:gravel`.
    pub path_material: String,
    /// Absolute `(x, y, z)` origin in world voxels.
    pub origin: [i32; 3],
    /// Voxel extents of the walkway block array.
    pub dims: [u32; 3],
}

/// `(edition, mc_version, data_version)` triple.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockTarget {
    /// Backend edition.
    pub edition: Edition,
    /// Human-facing Minecraft version, e.g. `"1.21.4"`.
    #[serde(deserialize_with = "version_label")]
    pub mc_version: String,
    /// Java `DataVersion`, or Bedrock block-palette `version`.
    pub data_version: i32,
}

/// External-input hashes the build depends on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockInputs {
    /// sha256 of the registry pack that resolved the ids.
    pub registry_pack_hash: HashHex,
    /// sha256 of the constraint catalog.
    pub constraint_catalog_hash: HashHex,
}

impl LockInputs {
    /// All-zero inputs.
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

#[cfg(test)]
mod tests;
