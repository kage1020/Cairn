//! A lockfile is a reproducibility claim: deterministic hashes, a YAML round trip of one shape,
//! and a reader that refuses any document it cannot vouch for.

use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use std::path::PathBuf;

use cairn_lang_core::lock::{
    HashHex, HashParseError, LOCK_SCHEMA_VERSION, LockError, LockInputs, LockPlacement,
    LockReading, LockTarget, LockWalkway, Lockfile, MemberSensitivity, hash_resolved_ir,
    hash_source,
};
use cairn_lang_core::{Edition, PlaceId, PortId, SiteName, WalkwayEndpoint, lower, parse, resolve};
use indexmap::IndexMap;

fn unit_ir(palette: Palette) -> BlockArrayIr {
    let dims = Dims { x: 1, y: 1, z: 1 };
    let mut structures: IndexMap<String, BlockArray> = IndexMap::new();
    structures.insert(
        "struct::unit".to_owned(),
        BlockArray {
            dims,
            palette,
            voxels: vec![PaletteIndex::AIR],
            block_entities: vec![],
            entities: vec![],
            source_scope: "struct::unit".to_owned(),
            derived: Vec::new(),
        },
    );
    BlockArrayIr {
        structures,
        placements: IndexMap::new(),
        walkways: IndexMap::new(),
        members: IndexMap::new(),
        diagnostics: vec![],
    }
}

#[test]
fn hash_source_empty_input_matches_known_sha256() {
    let h = hash_source("");
    assert_eq!(
        h.as_str(),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn hash_source_is_deterministic() {
    let a = hash_source("cottage example body\n");
    let b = hash_source("cottage example body\n");
    assert_eq!(a, b);
    let c = hash_source("cottage example body");
    assert_ne!(a, c, "trailing newline changes the hash");
}

#[test]
fn hash_resolved_ir_is_deterministic() {
    let ir = unit_ir(Palette::new_with_air());
    let h1 = hash_resolved_ir(&ir).expect("hash");
    let h2 = hash_resolved_ir(&ir).expect("hash");
    assert_eq!(h1, h2);
}

#[test]
fn hash_resolved_ir_reflects_palette_order() {
    let mut p1 = Palette::new_with_air();
    p1.intern(BlockState::bare("minecraft:cobblestone"));
    p1.intern(BlockState::bare("minecraft:oak_planks"));

    let mut p2 = Palette::new_with_air();
    p2.intern(BlockState::bare("minecraft:oak_planks"));
    p2.intern(BlockState::bare("minecraft:cobblestone"));

    let h1 = hash_resolved_ir(&unit_ir(p1)).expect("hash");
    let h2 = hash_resolved_ir(&unit_ir(p2)).expect("hash");
    assert_ne!(h1, h2);
}

#[test]
fn hash_resolved_ir_reflects_member_addresses() {
    let src = "theme t:\n  slot wall -> @cobblestone\n\n\
               struct s size=3x3\n  walls mat_slot=wall height=1\n";
    let module = parse(src).expect("parses");
    let intent = lower(&module);
    let mut ir = lower_to_block_array(&intent, &resolve(&intent, None), None);
    let addressed = hash_resolved_ir(&ir).expect("hash");
    let member = &mut ir.members["struct::s"][0];
    assert_eq!(
        member.address.as_ref().map(ToString::to_string).as_deref(),
        Some("walls[0]"),
    );
    member.address = None;
    assert_ne!(hash_resolved_ir(&ir).expect("hash"), addressed);
}

fn sample_lockfile() -> Lockfile {
    Lockfile {
        lock_schema_version: LOCK_SCHEMA_VERSION,
        source_hash: HashHex::from_bytes(b"cottage"),
        cairn_version: "2026.09".to_owned(),
        target: LockTarget {
            edition: Edition::Java,
            mc_version: "1.21.4".to_owned(),
            data_version: 4189,
        },
        inputs: LockInputs::zero(),
        resolved_ir_hash: HashHex::from_bytes(b"ir"),
        verified: true,
        member_version_sensitivity: vec![MemberSensitivity {
            id: "yard_water".to_owned(),
            reason: "cauldron split at 1.17".to_owned(),
        }],
        placements: vec![],
        walkways: vec![],
    }
}

#[test]
fn lockfile_roundtrips_through_yaml() {
    let lf = sample_lockfile();
    let body = serde_norway::to_string(&lf).expect("encode");
    let parsed: Lockfile = serde_norway::from_str(&body).expect("decode");
    assert_eq!(lf, parsed);
}

fn sample_lockfile_with_walkways() -> Lockfile {
    Lockfile {
        lock_schema_version: LOCK_SCHEMA_VERSION,
        walkways: vec![LockWalkway {
            site: SiteName::new("hamlet").expect("site"),
            from: WalkwayEndpoint {
                place: PlaceId::new("home1").expect("place"),
                port: PortId::new("entry").expect("port"),
            },
            to: WalkwayEndpoint {
                place: PlaceId::new("home2").expect("place"),
                port: PortId::new("entry").expect("port"),
            },
            path_material: "minecraft:gravel".to_owned(),
            origin: [5, 0, 8],
            dims: [16, 1, 1],
        }],
        ..sample_lockfile()
    }
}

#[test]
fn lockfile_with_walkways_roundtrips_through_yaml() {
    // `sample_lockfile()` leaves `walkways` empty, so its round trip never writes the section.
    let lf = sample_lockfile_with_walkways();
    let body = serde_norway::to_string(&lf).expect("encode");
    let parsed: Lockfile = serde_norway::from_str(&body).expect("decode");
    assert_eq!(lf, parsed);
    assert_eq!(parsed.walkways.len(), 1);
    let w = &parsed.walkways[0];
    assert_eq!(w.from.place.as_str(), "home1");
    assert_eq!(w.from.port.as_str(), "entry");
    assert_eq!(w.to.place.as_str(), "home2");
    assert_eq!(w.to.port.as_str(), "entry");
}

#[test]
fn lockfile_walkways_yaml_snapshot() {
    let body = serde_norway::to_string(&sample_lockfile_with_walkways()).expect("encode");
    insta::assert_snapshot!(body);
}

#[test]
fn lockfile_yaml_rejects_a_dotted_string_walkway_endpoint() {
    let body = format!(
        concat!(
            "source_hash: {zero}\ncairn_version: '2026.09'\n",
            "target:\n  edition: java\n  mc_version: '1.21.4'\n  data_version: 4189\n",
            "inputs:\n  registry_pack_hash: {zero}\n  constraint_catalog_hash: {zero}\n",
            "resolved_ir_hash: {zero}\nverified: true\nmember_version_sensitivity: []\n",
            "walkways:\n",
            "  - site: hamlet\n",
            "    from: home1.entry\n",
            "    to: home2.entry\n",
            "    path_material: 'minecraft:gravel'\n",
            "    origin: [5, 0, 8]\n",
            "    dims: [16, 1, 1]\n",
        ),
        zero = HashHex::ZERO_STR,
    );
    let err =
        serde_norway::from_str::<Lockfile>(&body).expect_err("legacy string walkway endpoint");
    let msg = err.to_string();
    assert!(
        msg.starts_with(
            "walkways[0].from: invalid type: string \"home1.entry\", expected struct \
             WalkwayEndpoint"
        ),
        "expected the field, the string and the shape it should have been, got: {msg}",
    );
}

#[test]
fn hash_parse_rejects_missing_prefix() {
    let err = HashHex::parse("deadbeef").expect_err("missing prefix");
    assert_eq!(err, HashParseError::MissingPrefix);
}

#[test]
fn hash_parse_rejects_wrong_length() {
    let err = HashHex::parse("sha256:dead").expect_err("short body");
    assert!(matches!(err, HashParseError::WrongLength { got: 4 }));
}

#[test]
fn hash_parse_rejects_non_hex_char() {
    let mut body = "0".repeat(64);
    body.replace_range(3..4, "z");
    let err = HashHex::parse(&format!("sha256:{body}")).expect_err("non hex");
    assert!(matches!(err, HashParseError::NonHexChar { index: 3 }));
}

#[test]
fn hash_parse_accepts_uppercase_hex() {
    let body = "A".repeat(64);
    let parsed = HashHex::parse(&format!("sha256:{body}")).expect("uppercase ok");
    assert_eq!(parsed.as_str(), format!("sha256:{body}"));
}

#[test]
fn lockfile_yaml_with_bad_edition_is_rejected() {
    let body = format!(
        "source_hash: {zero}\ncairn_version: '2026.09'\ntarget:\n  edition: jav\n  mc_version: '1.21.4'\n  data_version: 4189\ninputs:\n  registry_pack_hash: {zero}\n  constraint_catalog_hash: {zero}\nresolved_ir_hash: {zero}\nverified: true\nmember_version_sensitivity: []\n",
        zero = HashHex::ZERO_STR,
    );
    let err = serde_norway::from_str::<Lockfile>(&body).expect_err("typo edition");
    assert!(
        err.to_string()
            .starts_with("target.edition: unknown variant `jav`, expected `java` or `bedrock`"),
        "expected the field, the typo and the editions there are, got {err}",
    );
}

#[test]
fn lockfile_yaml_with_bad_hash_is_rejected() {
    let body = format!(
        "source_hash: not-a-hash\ncairn_version: '2026.09'\ntarget:\n  edition: java\n  mc_version: '1.21.4'\n  data_version: 4189\ninputs:\n  registry_pack_hash: {zero}\n  constraint_catalog_hash: {zero}\nresolved_ir_hash: {zero}\nverified: true\nmember_version_sensitivity: []\n",
        zero = HashHex::ZERO_STR,
    );
    let err = serde_norway::from_str::<Lockfile>(&body).expect_err("bad hash");
    assert!(
        err.to_string()
            .starts_with("hash is missing the `sha256:` prefix"),
        "expected hash prefix error, got {err}",
    );
}

#[test]
fn the_schema_version_is_the_first_thing_a_reader_sees() {
    let body = sample_lockfile().to_yaml().expect("encode");
    assert!(
        body.starts_with(&format!("lock_schema_version: {LOCK_SCHEMA_VERSION}\n")),
        "lockfile does not open with the schema version: {body:?}",
    );
}

#[test]
fn a_document_without_the_field_is_the_version_that_had_none() {
    let mut lf = sample_lockfile();
    lf.lock_schema_version = LOCK_SCHEMA_VERSION;
    let with_field = lf.to_yaml().expect("encode");
    let mut without_field = String::new();
    for line in with_field
        .lines()
        .filter(|line| !line.starts_with("lock_schema_version:"))
    {
        without_field.push_str(line);
        without_field.push('\n');
    }
    let parsed = Lockfile::from_yaml(&without_field).expect("decode a pre-version document");
    assert_eq!(parsed.lock_schema_version, LOCK_SCHEMA_VERSION);
    assert_eq!(parsed, lf);
}

#[test]
fn a_schema_from_the_future_is_refused_by_name() {
    let body = sample_lockfile().to_yaml().expect("encode").replace(
        &format!("lock_schema_version: {LOCK_SCHEMA_VERSION}"),
        "lock_schema_version: 99",
    );
    let err = Lockfile::from_yaml(&body).expect_err("a newer schema");
    assert!(
        matches!(
            err,
            LockError::UnsupportedSchemaVersion { found: 99, supported } if supported == LOCK_SCHEMA_VERSION
        ),
        "unexpected error: {err}",
    );
    let message = err.to_string();
    assert!(
        message.contains("99") && message.contains(&LOCK_SCHEMA_VERSION.to_string()),
        "the message should name both versions: {message}",
    );
}

/// A lockfile with every container populated, so a test can tamper with each one.
fn lockfile_with_every_container() -> Lockfile {
    Lockfile {
        placements: vec![LockPlacement {
            site: SiteName::new("hamlet").expect("site"),
            id: PlaceId::new("home1").expect("place"),
            def: "cottage".to_owned(),
            theme: "medieval".to_owned(),
            origin: [0, 0, 0],
            dims: [9, 5, 7],
        }],
        ..sample_lockfile_with_walkways()
    }
}

/// `body` with `attacker_controlled: yes` in the mapping at `path`; a numeric step indexes a
/// sequence.
fn tamper_at(body: &str, path: &[&str]) -> String {
    let mut doc: serde_norway::Value = serde_norway::from_str(body).expect("fixture parses");
    let mut cursor = &mut doc;
    for step in path {
        cursor = match step.parse::<usize>() {
            Ok(index) => cursor
                .as_sequence_mut()
                .and_then(|seq| seq.get_mut(index))
                .unwrap_or_else(|| panic!("no sequence entry at {step}")),
            Err(_) => cursor
                .as_mapping_mut()
                .and_then(|map| map.get_mut(step))
                .unwrap_or_else(|| panic!("no mapping key {step}")),
        };
    }
    let mapping = cursor.as_mapping_mut().expect("target is a mapping");
    mapping.insert(
        serde_norway::Value::String("attacker_controlled".to_owned()),
        serde_norway::Value::String("yes".to_owned()),
    );
    serde_norway::to_string(&doc).expect("re-encode")
}

#[test]
fn a_key_the_schema_does_not_declare_is_refused_wherever_it_sits() {
    // serde does not cascade `deny_unknown_fields`, so every container is listed here.
    let base = lockfile_with_every_container()
        .to_yaml()
        .expect("encode fixture");
    Lockfile::from_yaml(&base).expect("the untampered document still reads");

    let containers: &[(&str, &[&str])] = &[
        ("Lockfile", &[]),
        ("LockTarget", &["target"]),
        ("LockInputs", &["inputs"]),
        ("MemberSensitivity", &["member_version_sensitivity", "0"]),
        ("LockPlacement", &["placements", "0"]),
        ("LockWalkway", &["walkways", "0"]),
        ("WalkwayEndpoint", &["walkways", "0", "from"]),
    ];
    for (label, path) in containers {
        let body = tamper_at(&base, path);
        assert_ne!(
            body, base,
            "{label}: the fixture did not tamper with anything"
        );
        let Err(err) = Lockfile::from_yaml(&body) else {
            panic!("an unknown key inside {label} was accepted");
        };
        let message = err.to_string();
        assert!(
            message.contains("attacker_controlled"),
            "{label}: the error should name the key it refused: {message}",
        );
    }
}

#[test]
fn a_newer_schema_is_recognised_even_when_it_brings_new_keys() {
    let base = sample_lockfile().to_yaml().expect("encode");
    let body = base.replace(
        &format!("lock_schema_version: {LOCK_SCHEMA_VERSION}"),
        "lock_schema_version: 99\nfuture_only_key: 7",
    );
    assert_ne!(body, base, "the fixture did not change anything");
    let err = Lockfile::from_yaml(&body).expect_err("a newer schema");
    assert!(
        matches!(err, LockError::UnsupportedSchemaVersion { found: 99, .. }),
        "a newer schema must be reported as newer, not as malformed: {err}",
    );
}

#[test]
fn an_encoded_lockfile_ends_with_a_newline() {
    // `member_version_sensitivity` is written last, so both its empty and full shapes are tried.
    let mut without = sample_lockfile();
    without.member_version_sensitivity.clear();
    for (label, lf) in [
        ("empty trailing sequence", without),
        ("populated trailing sequence", sample_lockfile()),
    ] {
        let body = lf.to_yaml().expect("encode");
        assert!(
            body.ends_with('\n'),
            "{label} does not end with a newline: {body:?}"
        );
        assert!(
            !body.ends_with("\n\n"),
            "{label} gained a blank line: {body:?}",
        );
    }
}

#[test]
fn sample_lockfile_yaml_snapshot() {
    let body = serde_norway::to_string(&sample_lockfile()).expect("encode");
    insta::assert_snapshot!(body);
}

const SOURCE: &str = "struct hut size=3x3\n";

/// An empty directory of the test's own under the integration tests' scratch space.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("lock-reading")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clear scratch dir");
    }
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// `body` written as a lock in `name`'s scratch directory, read for `source`.
fn reading(name: &str, body: &str, source: &str) -> LockReading {
    let path = scratch(name).join("s.crn.lock");
    std::fs::write(&path, body).expect("write lock");
    LockReading::of(&path, source)
}

/// [`lockfile_with_every_container`] written for [`SOURCE`].
fn lock_for_source() -> Lockfile {
    Lockfile {
        source_hash: hash_source(SOURCE),
        ..lockfile_with_every_container()
    }
}

fn read_as(source_unchanged: bool, refused_identifier: Option<&str>) -> LockReading {
    LockReading::Read {
        target: sample_lockfile().target,
        verified: true,
        source_unchanged,
        cairn_version: "2026.09".to_owned(),
        refused_identifier: refused_identifier.map(str::to_owned),
    }
}

#[test]
fn a_lock_reports_its_target_and_whether_the_source_still_hashes_to_it() {
    let body = lock_for_source().to_yaml().expect("encode");
    assert_eq!(reading("unchanged", &body, SOURCE), read_as(true, None));
    let edited = format!("{SOURCE}  floor mat_slot=a\n");
    assert_eq!(reading("changed", &body, &edited), read_as(false, None));
}

#[test]
fn a_lock_recording_verified_false_reads_as_not_verified() {
    let lock = Lockfile {
        verified: false,
        ..lock_for_source()
    };
    let body = lock.to_yaml().expect("encode");
    let LockReading::Read { verified, .. } = reading("unverified", &body, SOURCE) else {
        panic!("a lock the schema accepts is read");
    };
    assert!(!verified);
}

#[test]
fn no_file_at_the_path_reads_as_missing() {
    let path = scratch("missing").join("s.crn.lock");
    assert_eq!(LockReading::of(&path, SOURCE), LockReading::Missing);
}

#[test]
fn a_newer_schema_reads_as_newer_whatever_else_it_holds() {
    let body = lock_for_source().to_yaml().expect("encode").replace(
        &format!("lock_schema_version: {LOCK_SCHEMA_VERSION}"),
        "lock_schema_version: 99\nfuture_only_key: 7",
    );
    assert_eq!(
        reading("newer", &body, SOURCE),
        LockReading::NewerSchema {
            lock_schema_version: 99,
            supported: LOCK_SCHEMA_VERSION,
        },
    );
}

#[test]
fn a_document_the_schema_refuses_reads_as_unreadable_with_the_cause() {
    let body = tamper_at(&lock_for_source().to_yaml().expect("encode"), &["target"]);
    assert_eq!(
        reading("tampered", &body, SOURCE),
        LockReading::Unreadable {
            error: "lockfile YAML: target: unknown field `attacker_controlled`, expected one of \
                    `edition`, `mc_version`, `data_version` at line 8 column 3"
                .to_owned(),
        },
    );
}

#[test]
fn a_path_that_cannot_be_read_as_a_file_reads_as_unreadable() {
    let dir = scratch("directory");
    let read = LockReading::of(&dir, SOURCE);
    assert!(
        matches!(&read, LockReading::Unreadable { error } if error.starts_with("lockfile I/O: ")),
        "{read:?}",
    );
}

#[test]
fn a_lock_refused_only_for_an_identifier_still_reports_its_target() {
    let body = lock_for_source()
        .to_yaml()
        .expect("encode")
        .replace("site: hamlet", "site: a/b");
    let refused = "identifier `a/b` contains forbidden character `/`";
    assert_eq!(
        reading("refused", &body, SOURCE),
        read_as(true, Some(refused))
    );
    assert_eq!(
        reading("refused-changed", &body, ""),
        read_as(false, Some(refused))
    );
}

#[test]
fn a_reading_serialises_with_its_status_as_a_tag() {
    let json = |reading: &LockReading| serde_json::to_value(reading).expect("serialise");
    assert_eq!(
        json(&read_as(
            false,
            Some("identifier `a/b` contains forbidden character `/`")
        )),
        serde_json::json!({
            "status": "read",
            "target": { "edition": "java", "mc_version": "1.21.4", "data_version": 4189 },
            "verified": true,
            "source_unchanged": false,
            "cairn_version": "2026.09",
            "refused_identifier": "identifier `a/b` contains forbidden character `/`",
        }),
    );
    assert_eq!(
        json(&read_as(true, None)),
        serde_json::json!({
            "status": "read",
            "target": { "edition": "java", "mc_version": "1.21.4", "data_version": 4189 },
            "verified": true,
            "source_unchanged": true,
            "cairn_version": "2026.09",
        }),
    );
    assert_eq!(
        json(&LockReading::Missing),
        serde_json::json!({ "status": "missing" })
    );
    assert_eq!(
        json(&LockReading::NewerSchema {
            lock_schema_version: 2,
            supported: 1,
        }),
        serde_json::json!({ "status": "newer_schema", "lock_schema_version": 2, "supported": 1 }),
    );
    assert_eq!(
        json(&LockReading::Unreadable {
            error: "lockfile I/O: denied".to_owned(),
        }),
        serde_json::json!({ "status": "unreadable", "error": "lockfile I/O: denied" }),
    );
}
