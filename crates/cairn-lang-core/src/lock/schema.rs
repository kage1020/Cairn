//! `build.cairn.lock` YAML schema.
//!
//! Field order in [`Lockfile`] is deliberate: `serde_norway` writes structs in
//! declaration order, so this matches the sample in
//! `spec/versioning-editions` "Provenance and lock" byte-for-byte. Changing the
//! order breaks downstream tools that grep the lockfile, so it is also
//! exercised by an AC.

use serde::{Deserialize, Serialize};

use super::hash::HashHex;
use crate::ids::{IdError, PlaceId, PortId, SiteName, WalkwayEndpoint};

/// The lockfile schema this build reads and writes.
///
/// Version 1 is the shape that shipped before the number was recorded, so a
/// document without the field is that version rather than an unknown one —
/// refusing those would mean refusing files this compiler wrote. A document
/// declaring anything higher is refused rather than read as if the field
/// names still meant the same thing.
pub const LOCK_SCHEMA_VERSION: u32 = 1;

/// Default for a document written before the version was recorded.
///
/// The literal `1`, not [`LOCK_SCHEMA_VERSION`]: the meaning being pinned
/// is "the shape that shipped before the field existed", which stays
/// version 1 forever. Tracking the constant would silently re-read every
/// v1 document as v2 the day the constant moves.
fn assume_unversioned_schema() -> u32 {
    1
}

/// Just the leading field, read without the strict field check.
///
/// `deny_unknown_fields` fails *during* deserialisation, so a realistic
/// later format — one that adds a key — would be rejected as malformed
/// YAML before its version was ever compared, and its author told the
/// document was broken when the truth is that this build is too old.
/// Reading the version on its own is what makes [`Lockfile`]'s first
/// field mean what its doc says: a reader decides whether it understands
/// the rest before parsing the rest.
#[derive(Deserialize)]
struct SchemaVersionProbe {
    #[serde(default = "assume_unversioned_schema")]
    lock_schema_version: u32,
}

/// The schema revision `body` declares, whatever else it contains.
pub(super) fn declared_schema_version(body: &str) -> Result<u32, serde_norway::Error> {
    let probe: SchemaVersionProbe = serde_norway::from_str(body)?;
    Ok(probe.lock_schema_version)
}

/// What a lockfile still says about its build when it records an identifier
/// this build refuses.
///
/// Returned by [`super::Lockfile::refused_identifier`]. The identifier rule
/// has tightened over releases, so a lock an earlier Cairn wrote can name a
/// place, port or site the current rule refuses. It is returned only for a
/// document that is a valid lockfile in every other respect — every field
/// present, every value in its domain, no key the schema does not declare,
/// a schema version this build reads — so the strict read failed on the
/// identifier rule and on nothing else. The target it was verified for is
/// still worth comparing against, which is why this carries it.
#[derive(Debug, Clone, PartialEq)]
pub struct RefusedIdentifierLock {
    /// The first recorded identifier the current rule refuses, in the order
    /// [`super::Lockfile::refused_identifier`] states, and why. Not
    /// necessarily the one the strict read reported, which is the first in
    /// document order.
    pub error: IdError,
    /// The target the lock was verified for.
    pub target: LockTarget,
    /// The members the lock flagged as version-sensitive.
    pub member_version_sensitivity: Vec<MemberSensitivity>,
}

/// [`Lockfile`] with its seven identifier fields read as plain strings, so
/// the identifier rule does not stop the read, and nothing else loosened.
///
/// Every other field has the type, the default and the unknown-key refusal
/// [`Lockfile`] gives it, nested types included, so a document this reads
/// is one the strict read would take but for its identifiers. That is what
/// lets [`RefusedIdentifierLock`] call the rest of the document valid: a
/// missing field, a malformed hash or a stray key fails here exactly as it
/// fails there. The schema version is gated before this is read, as
/// [`super::Lockfile::from_yaml`] gates it. `mirrors_the_strict_schema`
/// below destructures each strict struct field by field, so a field added
/// there fails to compile until it is added here.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockfileProbe {
    #[serde(default = "assume_unversioned_schema")]
    lock_schema_version: u32,
    source_hash: HashHex,
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

/// Read `body` through [`LockfileProbe`] and report the first identifier
/// the rule refuses, in the order [`super::Lockfile::refused_identifier`]
/// states.
///
/// `Ok(None)` when the document is valid and every identifier passes;
/// `Err` when the document is invalid for a reason other than its
/// identifiers, carrying that reason. The schema version is the caller's to
/// gate, before this runs.
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
        target: probe.target,
        member_version_sensitivity: probe.member_version_sensitivity,
    }))
}

/// The whole lockfile, as written to `build.cairn.lock`.
///
/// `verified: true` is the default after a successful compile; a future
/// `--no-verify` workflow will flip it to `false` and the same struct
/// shape will roundtrip without changes.
///
/// This struct, every type it nests, and the probe that mirrors them for
/// [`super::Lockfile::refused_identifier`] deny unknown fields. The one
/// reader in this file that does not is `SchemaVersionProbe`, which reads
/// the version alone to decide whether to read the rest. A lockfile is a
/// claim about what was built, and one carrying keys the reader silently
/// ignores is a document whose meaning depends on who is reading it — a
/// tampered file used to deserialise as `Ok` with `verified: true` alongside
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
    /// at the time of the build, roof `overhang=` included. An `east_of`
    /// row's origin can be computed from the lockfile alone, since it
    /// reads only the prior placement's `dims.x`; a `north_of` row's
    /// cannot, since it steps back by the new placement's own `dims.z`,
    /// which is known only once that body is lowered.
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

#[cfg(test)]
mod tests {
    use serde_norway::Value;

    use super::super::{LockError, Lockfile};
    use super::*;

    /// What a strict document reads as through [`LockfileProbe`]: every
    /// field carried over, the identifiers as their strings.
    ///
    /// Each strict struct is destructured field by field, with no `..`, and
    /// each probe is built the same way. A field added to `Lockfile`,
    /// `LockPlacement`, `LockWalkway` or `WalkwayEndpoint` is a compile
    /// error here until the probe has it too, which is what keeps an added
    /// identifier field from being missed by the probe in silence.
    fn mirror(lock: Lockfile) -> LockfileProbe {
        let Lockfile {
            lock_schema_version,
            source_hash,
            cairn_version,
            target,
            inputs,
            resolved_ir_hash,
            verified,
            member_version_sensitivity,
            placements,
            walkways,
        } = lock;
        LockfileProbe {
            lock_schema_version,
            source_hash,
            cairn_version,
            target,
            inputs,
            resolved_ir_hash,
            verified,
            member_version_sensitivity,
            placements: placements.into_iter().map(mirror_placement).collect(),
            walkways: walkways.into_iter().map(mirror_walkway).collect(),
        }
    }

    fn mirror_placement(placement: LockPlacement) -> PlacementProbe {
        let LockPlacement {
            site,
            id,
            def,
            theme,
            origin,
            dims,
        } = placement;
        PlacementProbe {
            site: site.into_inner(),
            id: id.into_inner(),
            def,
            theme,
            origin,
            dims,
        }
    }

    fn mirror_walkway(walkway: LockWalkway) -> WalkwayProbe {
        let LockWalkway {
            site,
            from,
            to,
            path_material,
            origin,
            dims,
        } = walkway;
        WalkwayProbe {
            site: site.into_inner(),
            from: mirror_endpoint(from),
            to: mirror_endpoint(to),
            path_material,
            origin,
            dims,
        }
    }

    fn mirror_endpoint(endpoint: WalkwayEndpoint) -> EndpointProbe {
        let WalkwayEndpoint { place, port } = endpoint;
        EndpointProbe {
            place: place.into_inner(),
            port: port.into_inner(),
        }
    }

    fn endpoint(place: &str, port: &str) -> WalkwayEndpoint {
        WalkwayEndpoint {
            place: PlaceId::new(place).expect("place"),
            port: PortId::new(port).expect("port"),
        }
    }

    /// A valid lock with one of everything the probe reads, every
    /// identifier distinct, and a version-sensitive member so an empty
    /// list recovered by mistake shows.
    fn sample() -> Lockfile {
        Lockfile {
            lock_schema_version: LOCK_SCHEMA_VERSION,
            source_hash: HashHex::zero(),
            cairn_version: "2026.10.0".to_owned(),
            target: LockTarget {
                edition: LockEdition::Java,
                mc_version: "1.20.4".to_owned(),
                data_version: 3700,
            },
            inputs: LockInputs::zero(),
            resolved_ir_hash: HashHex::zero(),
            verified: true,
            member_version_sensitivity: vec![MemberSensitivity {
                id: "yard_water".to_owned(),
                reason: "cauldron split at 1.17".to_owned(),
            }],
            placements: vec![LockPlacement {
                site: SiteName::new("hamlet").expect("site"),
                id: PlaceId::new("home1").expect("id"),
                def: "cottage".to_owned(),
                theme: "medieval".to_owned(),
                origin: [0, 0, -5],
                dims: [5, 4, 5],
            }],
            walkways: vec![LockWalkway {
                site: SiteName::new("lane").expect("site"),
                from: endpoint("home1", "entry"),
                to: endpoint("home2", "porch"),
                path_material: "minecraft:gravel".to_owned(),
                origin: [5, 0, 0],
                dims: [4, 1, 1],
            }],
        }
    }

    /// The seven identifier positions, in the order the probe checks them.
    const POSITIONS: [&str; 7] = [
        "placements.0.site",
        "placements.0.id",
        "walkways.0.site",
        "walkways.0.from.place",
        "walkways.0.from.port",
        "walkways.0.to.place",
        "walkways.0.to.port",
    ];

    fn at<'a>(doc: &'a mut Value, path: &str) -> &'a mut Value {
        path.split('.')
            .fold(doc, |node, segment| match segment.parse::<usize>() {
                Ok(index) => &mut node[index],
                Err(_) => &mut node[segment],
            })
    }

    fn sample_doc() -> Value {
        serde_norway::from_str(&sample().to_yaml().expect("encode")).expect("decode")
    }

    fn encode(doc: &Value) -> String {
        serde_norway::to_string(doc).expect("encode")
    }

    /// The sample with each `(path, value)` written over it.
    fn doctored(edits: &[(&str, &str)]) -> String {
        let mut doc = sample_doc();
        for (path, value) in edits {
            *at(&mut doc, path) = Value::String((*value).to_owned());
        }
        encode(&doc)
    }

    fn refused(ident: &str, ch: char) -> IdError {
        IdError::ForbiddenChar {
            ident: ident.to_owned(),
            ch,
        }
    }

    #[test]
    fn mirrors_the_strict_schema() {
        // What a valid document reads as through the probe is what the
        // strict read gives, field for field.
        let body = sample().to_yaml().expect("encode");
        let probe: LockfileProbe = serde_norway::from_str(&body).expect("the probe reads it");
        assert_eq!(probe, mirror(sample()));
        assert_eq!(Lockfile::from_yaml(&body).expect("strict"), sample());
    }

    #[test]
    fn a_valid_lock_records_no_refused_identifier() {
        let body = sample().to_yaml().expect("encode");
        assert_eq!(Lockfile::refused_identifier(&body).expect("valid"), None);
    }

    #[test]
    fn each_identifier_position_is_read_and_checked() {
        for path in POSITIONS {
            let body = doctored(&[(path, "a/b")]);
            assert!(
                Lockfile::from_yaml(&body).is_err(),
                "{path}: the strict read refuses the id, which is why the probe exists",
            );
            let found = Lockfile::refused_identifier(&body)
                .unwrap_or_else(|err| panic!("{path}: the rest is valid, got {err}"))
                .unwrap_or_else(|| panic!("{path}: the refused id was not seen"));
            assert_eq!(found.error, refused("a/b", '/'), "{path}");
            assert_eq!(found.target, sample().target, "{path}");
            assert_eq!(
                found.member_version_sensitivity,
                sample().member_version_sensitivity,
                "{path}: what the lock recorded, not a default",
            );
        }
    }

    #[test]
    fn the_first_refused_identifier_in_field_order_is_the_one_reported() {
        for (earlier, first) in POSITIONS.iter().enumerate() {
            for (later, second) in POSITIONS.iter().enumerate().skip(earlier + 1) {
                let first_id = format!("first{earlier}/x");
                let second_id = format!("second{later}*x");
                let body = doctored(&[(first, &first_id), (second, &second_id)]);
                let found = Lockfile::refused_identifier(&body)
                    .expect("valid but for its ids")
                    .expect("refused ids recorded");
                assert_eq!(
                    found.error,
                    refused(&first_id, '/'),
                    "`{first}` is checked before `{second}`",
                );
            }
        }
    }

    #[test]
    fn field_order_is_not_document_order() {
        // A document that writes `walkways:` first: the strict read stops at
        // the walkway's id, and the probe still reports the placement's,
        // which is what the public doc says.
        let mut doc = sample_doc();
        *at(&mut doc, "placements.0.id") = Value::String("p/1".to_owned());
        *at(&mut doc, "walkways.0.site") = Value::String("w*1".to_owned());
        let map = doc.as_mapping_mut().expect("a mapping");
        let placements = map.remove("placements").expect("placements");
        map.insert(Value::String("placements".to_owned()), placements);
        let body = encode(&doc);
        assert!(
            body.find("walkways:") < body.find("placements:"),
            "the fixture reorders: {body}"
        );

        let strict = Lockfile::from_yaml(&body).expect_err("ids refused");
        assert!(strict.to_string().contains("w*1"), "{strict}");
        let found = Lockfile::refused_identifier(&body)
            .expect("valid but for its ids")
            .expect("refused ids recorded");
        assert_eq!(found.error, refused("p/1", '/'));
    }

    #[test]
    fn a_body_that_is_not_yaml_is_an_error_not_an_answer() {
        let err = Lockfile::refused_identifier("placements: [").expect_err("not YAML");
        assert!(matches!(err, LockError::Yaml(_)), "{err}");
    }

    #[test]
    fn an_unreadable_target_is_the_reason_given_not_the_identifier() {
        // The strict read meets the refused id first and reports it; the
        // reason this document cannot be read is the missing target, and
        // that is what comes back.
        let mut doc = sample_doc();
        *at(&mut doc, "placements.0.id") = Value::String("sub/home1".to_owned());
        doc.as_mapping_mut()
            .expect("a mapping")
            .remove("target")
            .expect("target");
        let err = Lockfile::refused_identifier(&encode(&doc)).expect_err("no target");
        let LockError::Yaml(cause) = &err else {
            panic!("expected a YAML error, got {err}");
        };
        assert!(cause.to_string().contains("target"), "{cause}");
        assert!(!cause.to_string().contains("sub/home1"), "{cause}");
    }

    #[test]
    fn a_later_schema_version_is_refused_before_the_identifiers_are_read() {
        let mut doc = sample_doc();
        *at(&mut doc, "placements.0.id") = Value::String("sub/home1".to_owned());
        *at(&mut doc, "lock_schema_version") = Value::Number(2.into());
        let err = Lockfile::refused_identifier(&encode(&doc)).expect_err("version 2");
        assert!(
            matches!(
                err,
                LockError::UnsupportedSchemaVersion {
                    found: 2,
                    supported: LOCK_SCHEMA_VERSION
                }
            ),
            "{err}",
        );
    }

    /// One way to break a lock that has nothing to do with its identifiers.
    type Corruption = fn(&mut Value);

    #[test]
    fn a_lock_broken_besides_its_identifiers_is_not_reported_as_refused() {
        // Each corruption on its own fails the strict read. With a refused id
        // beside it, the probe has to fail too: one refused id must not be
        // enough to call a broken document valid.
        fn unknown(node: &mut Value) {
            node.as_mapping_mut()
                .expect("a mapping")
                .insert(Value::String("tampered".to_owned()), Value::Bool(true));
        }
        let cases: [(&str, Corruption); 11] = [
            ("missing member_version_sensitivity", |doc| {
                doc.as_mapping_mut()
                    .expect("a mapping")
                    .remove("member_version_sensitivity");
            }),
            ("missing placements[0].def", |doc| {
                at(doc, "placements.0")
                    .as_mapping_mut()
                    .expect("a mapping")
                    .remove("def");
            }),
            ("malformed source_hash", |doc| {
                *at(doc, "source_hash") = Value::String("NOTAHASH:".to_owned());
            }),
            ("malformed inputs hash", |doc| {
                *at(doc, "inputs.registry_pack_hash") = Value::String("sha256:zz".to_owned());
            }),
            ("schema version as a string", |doc| {
                *at(doc, "lock_schema_version") = Value::String("1".to_owned());
            }),
            ("verified not a bool", |doc| {
                *at(doc, "verified") = Value::Number(3.into());
            }),
            ("unknown top-level key", |doc| unknown(doc)),
            ("unknown key under target", |doc| unknown(at(doc, "target"))),
            ("unknown key under a placement", |doc| {
                unknown(at(doc, "placements.0"));
            }),
            ("unknown key under a walkway endpoint", |doc| {
                unknown(at(doc, "walkways.0.from"));
            }),
            ("unknown key under a sensitive member", |doc| {
                unknown(at(doc, "member_version_sensitivity.0"));
            }),
        ];
        for (label, corrupt) in cases {
            let mut alone = sample_doc();
            corrupt(&mut alone);
            assert!(
                Lockfile::from_yaml(&encode(&alone)).is_err(),
                "{label}: the corruption alone must fail the strict read",
            );

            let mut doc = sample_doc();
            corrupt(&mut doc);
            *at(&mut doc, "placements.0.id") = Value::String("sub/home1".to_owned());
            let result = Lockfile::refused_identifier(&encode(&doc));
            assert!(
                result.is_err(),
                "{label}: a broken lock with a refused id must not read as refused, got {result:?}",
            );
        }
    }
}
