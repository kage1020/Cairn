use serde_norway::Value;

use super::super::{LockError, Lockfile, LockfileRead};
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

/// What [`sample`] records, refused for `error`.
fn refused_sample(error: IdError) -> RefusedIdentifierLock {
    let Lockfile {
        source_hash,
        cairn_version,
        target,
        verified,
        member_version_sensitivity,
        ..
    } = sample();
    RefusedIdentifierLock {
        error,
        source_hash,
        cairn_version,
        target,
        verified,
        member_version_sensitivity,
    }
}

/// `body` as [`LockfileRead::from_body`] classifies it: `Ok(None)` for a valid lock, the refused
/// lock for one valid but for an identifier, and the error otherwise.
fn classified(body: &str) -> Result<Option<RefusedIdentifierLock>, LockError> {
    match LockfileRead::from_body(body) {
        LockfileRead::Valid(_) => Ok(None),
        LockfileRead::RefusedIdentifier(refused) => Ok(Some(refused)),
        LockfileRead::NewerSchema { found, supported } => {
            Err(LockError::UnsupportedSchemaVersion { found, supported })
        }
        LockfileRead::Invalid(err) => Err(err),
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
    assert_eq!(classified(&body).expect("valid"), None);
}

#[test]
fn each_identifier_position_is_read_and_checked() {
    for path in POSITIONS {
        let body = doctored(&[(path, "a/b")]);
        assert!(
            Lockfile::from_yaml(&body).is_err(),
            "{path}: the strict read refuses the id",
        );
        let found = classified(&body)
            .unwrap_or_else(|err| panic!("{path}: the rest is valid, got {err}"))
            .unwrap_or_else(|| panic!("{path}: the refused id was not seen"));
        assert_eq!(found, refused_sample(refused("a/b", '/')), "{path}");
    }
}

#[test]
fn a_refused_lock_carries_the_verified_and_cairn_version_it_records() {
    let mut doc = sample_doc();
    *at(&mut doc, "placements.0.id") = Value::String("a/b".to_owned());
    *at(&mut doc, "verified") = Value::Bool(false);
    *at(&mut doc, "cairn_version") = Value::String("2026.9.4".to_owned());
    let found = classified(&encode(&doc))
        .expect("valid but for its id")
        .expect("the refused id was seen");
    assert_eq!(
        found,
        RefusedIdentifierLock {
            verified: false,
            cairn_version: "2026.9.4".to_owned(),
            ..refused_sample(refused("a/b", '/'))
        },
    );
}

#[test]
fn a_place_spelling_a_windows_device_is_refused_where_the_lock_records_a_place() {
    for path in [
        "placements.0.id",
        "walkways.0.from.place",
        "walkways.0.to.place",
    ] {
        let body = doctored(&[(path, "Con")]);
        assert!(
            Lockfile::from_yaml(&body).is_err(),
            "{path}: the strict read refuses the id",
        );
        let found = classified(&body)
            .unwrap_or_else(|err| panic!("{path}: the rest is valid, got {err}"))
            .unwrap_or_else(|| panic!("{path}: the refused id was not seen"));
        assert_eq!(
            found.error,
            IdError::ReservedDeviceName {
                ident: "Con".to_owned()
            },
            "{path}",
        );
        assert_eq!(found.target, sample().target, "{path}");
    }
}

#[test]
fn a_site_or_port_spelling_a_windows_device_reads_as_valid() {
    for path in [
        "placements.0.site",
        "walkways.0.site",
        "walkways.0.from.port",
        "walkways.0.to.port",
    ] {
        let body = doctored(&[(path, "Con")]);
        assert!(Lockfile::from_yaml(&body).is_ok(), "{path}");
        assert_eq!(classified(&body).expect("valid"), None, "{path}");
    }
}

#[test]
fn the_first_refused_identifier_in_field_order_is_the_one_reported() {
    for (earlier, first) in POSITIONS.iter().enumerate() {
        for (later, second) in POSITIONS.iter().enumerate().skip(earlier + 1) {
            let first_id = format!("first{earlier}/x");
            let second_id = format!("second{later}*x");
            let body = doctored(&[(first, &first_id), (second, &second_id)]);
            let found = classified(&body)
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
    let found = classified(&body)
        .expect("valid but for its ids")
        .expect("refused ids recorded");
    assert_eq!(found.error, refused("p/1", '/'));
}

#[test]
fn a_body_that_is_not_yaml_is_an_error_not_an_answer() {
    let err = classified("placements: [").expect_err("not YAML");
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
    let err = classified(&encode(&doc)).expect_err("no target");
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
    let err = classified(&encode(&doc)).expect_err("version 2");
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
    let cases: [(&str, Corruption); 14] = [
        ("schema version 0", |doc| {
            *at(doc, "lock_schema_version") = Value::Number(0.into());
        }),
        ("cairn_version not a release", |doc| {
            *at(doc, "cairn_version") = Value::String("2026.10.0-dev".to_owned());
        }),
        ("mc_version breaking its line", |doc| {
            *at(doc, "target.mc_version") = Value::String("1.20.4\nfake: row".to_owned());
        }),
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
        let result = classified(&encode(&doc));
        assert!(
            result.is_err(),
            "{label}: a broken lock with a refused id must not read as refused, got {result:?}",
        );
    }
}
