//! `build.cairn.lock` YAML schema. Fields are written in declaration order, which matches the
//! sample in `spec/versioning-editions` "Provenance and lock".

use serde::{Deserialize, Serialize};

use super::hash::HashHex;
use crate::edition::Edition;
use crate::ids::{IdError, PlaceId, PortId, SiteName, WalkwayEndpoint};

/// The lockfile schema this build reads and writes; a document without the field is version 1.
pub const LOCK_SCHEMA_VERSION: u32 = 1;

/// The literal `1`, not [`LOCK_SCHEMA_VERSION`]: an unversioned document stays version 1 forever.
fn assume_unversioned_schema() -> u32 {
    1
}

/// The version alone, read before `deny_unknown_fields` can call a newer format malformed.
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

/// What a lockfile valid but for an identifier the current rule refuses says about its build.
#[derive(Debug, Clone, PartialEq)]
pub struct RefusedIdentifierLock {
    /// The first refused identifier in field order, which need not be document order.
    pub error: IdError,
    /// The target the lock was verified for.
    pub target: LockTarget,
    /// The members the lock flagged as version-sensitive.
    pub member_version_sensitivity: Vec<MemberSensitivity>,
}

/// [`Lockfile`] with its seven identifier fields read as plain strings and nothing else loosened.
/// The tests' `mirror` stops compiling until a field added to a strict struct is added here.
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

/// The first identifier the rule refuses, in field order; the caller gates the schema version.
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
/// It and every type it nests deny unknown fields, so a tampered document cannot read as verified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    /// Schema revision, first so a reader can check it before the rest.
    #[serde(default = "assume_unversioned_schema")]
    pub lock_schema_version: u32,
    /// sha256 over the raw `.crn` bytes.
    pub source_hash: HashHex,
    /// The Cairn release `CalVer` that produced the lockfile.
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
mod tests {
    use serde_norway::Value;

    use super::super::{LockError, Lockfile};
    use super::*;

    /// Destructured without `..`, so a field added to a strict struct fails to compile here.
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

    fn sample() -> Lockfile {
        Lockfile {
            lock_schema_version: LOCK_SCHEMA_VERSION,
            source_hash: HashHex::zero(),
            cairn_version: "2026.10.0".to_owned(),
            target: LockTarget {
                edition: Edition::Java,
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
                "{path}: the strict read refuses the id",
            );
            let found = Lockfile::refused_identifier(&body)
                .unwrap_or_else(|err| panic!("{path}: the rest is valid, got {err}"))
                .unwrap_or_else(|| panic!("{path}: the refused id was not seen"));
            assert_eq!(found.error, refused("a/b", '/'), "{path}");
            assert_eq!(found.target, sample().target, "{path}");
            assert_eq!(
                found.member_version_sensitivity,
                sample().member_version_sensitivity,
                "{path}",
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

    type Corruption = fn(&mut Value);

    #[test]
    fn a_lock_broken_besides_its_identifiers_is_not_reported_as_refused() {
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
