//! AC L1–L8 for the lockfile module.

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_core::lock::{
    HashHex, HashParseError, LOCK_SCHEMA_VERSION, LockEdition, LockError, LockInputs,
    LockPlacement, LockTarget, LockWalkway, Lockfile, MemberSensitivity, hash_resolved_ir,
    hash_source,
};
use cairn_lang_core::{PlaceId, PortId, SiteName, WalkwayEndpoint};
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
        },
    );
    BlockArrayIr {
        structures,
        placements: IndexMap::new(),
        walkways: IndexMap::new(),
        diagnostics: vec![],
    }
}

#[test]
fn l1_hash_source_empty_input_matches_known_sha256() {
    // AC L1: well-known sha256 of the empty input.
    let h = hash_source("");
    assert_eq!(
        h.as_str(),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn l2_hash_source_is_deterministic() {
    // AC L2: same input → same hash, across calls.
    let a = hash_source("cottage example body\n");
    let b = hash_source("cottage example body\n");
    assert_eq!(a, b);
    let c = hash_source("cottage example body");
    assert_ne!(a, c, "trailing newline changes the hash");
}

#[test]
fn l3_hash_resolved_ir_is_deterministic() {
    // AC L3: same IR → same hash, across calls.
    let ir = unit_ir(Palette::new_with_air());
    let h1 = hash_resolved_ir(&ir).expect("hash");
    let h2 = hash_resolved_ir(&ir).expect("hash");
    assert_eq!(h1, h2);
}

#[test]
fn l4_hash_resolved_ir_reflects_palette_order() {
    // AC L4: palette insertion order is observable through the hash.
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

fn sample_lockfile() -> Lockfile {
    Lockfile {
        lock_schema_version: LOCK_SCHEMA_VERSION,
        source_hash: HashHex::from_bytes(b"cottage"),
        cairn_version: "2026.09".to_owned(),
        target: LockTarget {
            edition: LockEdition::Java,
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
fn l5_lockfile_roundtrips_through_yaml() {
    // AC L5: serialise → deserialise → equal.
    let lf = sample_lockfile();
    let body = serde_norway::to_string(&lf).expect("encode");
    let parsed: Lockfile = serde_norway::from_str(&body).expect("decode");
    assert_eq!(lf, parsed);
}

#[test]
fn l6_lockfile_yaml_key_order_matches_spec() {
    // AC L6: top-level keys appear in the order spec §10.6 prints,
    // `lock_schema_version` first. The AC moved deliberately: a reader that
    // has to decide whether it understands the document at all cannot be
    // asked to parse the rest of it first, so the version leads and the
    // spec sample was updated to match in both languages.
    let body = serde_norway::to_string(&sample_lockfile()).expect("encode");
    let keys: Vec<&str> = body
        .lines()
        .filter_map(|line| {
            if line.starts_with(char::is_whitespace) || line.starts_with('-') {
                None
            } else {
                line.split(':').next()
            }
        })
        .collect();
    let expected = vec![
        "lock_schema_version",
        "source_hash",
        "cairn_version",
        "target",
        "inputs",
        "resolved_ir_hash",
        "verified",
        "member_version_sensitivity",
    ];
    assert_eq!(keys, expected);
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
    // A lockfile carrying a non-empty `walkways:` section must
    // serialise and deserialise back to the same value — without this
    // the new section could silently lose fields under a future struct
    // rename and the `sample_lockfile()` round-trip (which leaves
    // `walkways: vec![]` and so skips the section entirely) would not
    // catch it.
    let lf = sample_lockfile_with_walkways();
    let body = serde_norway::to_string(&lf).expect("encode");
    let parsed: Lockfile = serde_norway::from_str(&body).expect("decode");
    assert_eq!(lf, parsed);
    assert_eq!(parsed.walkways.len(), 1);
    // Assert through the typed structure rather than a substring of the
    // YAML body so an indentation tweak in `serde_norway` cannot turn a
    // green test into a silent regression on the wire shape.
    let w = &parsed.walkways[0];
    assert_eq!(w.from.place.as_str(), "home1");
    assert_eq!(w.from.port.as_str(), "entry");
    assert_eq!(w.to.place.as_str(), "home2");
    assert_eq!(w.to.port.as_str(), "entry");
}

#[test]
fn lockfile_walkways_yaml_snapshot() {
    // Pin the full wire shape of a walkway-bearing lockfile (key order,
    // nested `from`/`to` objects, `dims` as a 3-element list) so a
    // future struct reshuffle is caught here before downstream tooling
    // that greps the lockfile breaks. The `sample_lockfile()` snapshot
    // (`l8_*`) covers the walkway-less case; this is its companion.
    let body = serde_norway::to_string(&sample_lockfile_with_walkways()).expect("encode");
    insta::assert_snapshot!(body);
}

#[test]
fn lockfile_yaml_rejects_legacy_string_walkway_endpoint() {
    // Pre-newtype lockfiles encoded `from`/`to` as a single
    // `"PLACE.PORT"` joined string. A `#[serde(untagged)]` fallback
    // that quietly accepted the legacy form would re-open the
    // silent-disaster the newtype split was meant to close, so the
    // reject must stay loud. Construct the offending YAML by hand so
    // the test does not depend on whatever the current encoder emits.
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
        msg.contains("place") || msg.contains("expected") || msg.contains("invalid"),
        "expected a structural deserialise error pointing at the missing `place`/`port` \
         fields, got: {msg}",
    );
}

#[test]
fn l7_hash_zero_matches_canonical_string() {
    // AC L7: zero hash is the spec-defined sentinel.
    assert_eq!(HashHex::zero().as_str(), HashHex::ZERO_STR);
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
    // Lockfiles written by hand may use uppercase. We round-trip the
    // exact bytes given so the user's choice survives `read → write`.
    let body = "A".repeat(64);
    let parsed = HashHex::parse(&format!("sha256:{body}")).expect("uppercase ok");
    assert_eq!(parsed.as_str(), format!("sha256:{body}"));
}

#[test]
fn lockfile_yaml_with_bad_edition_is_rejected() {
    // `LockEdition` is a closed enum on the wire too, so a typo in the
    // edition cannot ride along as a valid lockfile.
    let body = format!(
        "source_hash: {zero}\ncairn_version: '2026.09'\ntarget:\n  edition: jav\n  mc_version: '1.21.4'\n  data_version: 4189\ninputs:\n  registry_pack_hash: {zero}\n  constraint_catalog_hash: {zero}\nresolved_ir_hash: {zero}\nverified: true\nmember_version_sensitivity: []\n",
        zero = HashHex::ZERO_STR,
    );
    let err = serde_norway::from_str::<Lockfile>(&body).expect_err("typo edition");
    assert!(
        err.to_string().contains("jav") || err.to_string().contains("variant"),
        "expected variant error, got {err}",
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
        err.to_string().contains("prefix"),
        "expected hash prefix error, got {err}",
    );
}

#[test]
fn the_schema_version_is_the_first_thing_a_reader_sees() {
    // A consumer that has to decide whether it understands the document at
    // all should not have to parse the rest of it first, and the field is
    // useless for recognising an older format if it can move.
    let body = sample_lockfile().to_yaml().expect("encode");
    assert!(
        body.starts_with(&format!("lock_schema_version: {LOCK_SCHEMA_VERSION}\n")),
        "lockfile does not open with the schema version: {body:?}",
    );
}

#[test]
fn a_document_without_the_field_is_the_version_that_had_none() {
    // Every lockfile written before the field existed is this shape. It is
    // the schema this build reads, so absence means version 1 rather than
    // "unknown" — the alternative is refusing to read files the compiler
    // itself wrote.
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
    // The whole point of recording the version: a later format that reuses
    // these field names must not be read as if it were this one. Before the
    // field existed such a document deserialised as `Ok` with
    // `verified: true`.
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

/// A lockfile with every container populated, so a test can tamper with
/// each one. `sample_lockfile` leaves `placements` and `walkways` empty,
/// which is why the endpoint containers never appeared in the YAML to be
/// tampered with.
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

/// Insert `attacker_controlled: yes` into the mapping at `path`, where
/// each step is a key and a numeric step indexes a sequence.
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
    // A lockfile is a claim about what was built; a document carrying keys
    // the reader ignores is a document whose meaning depends on who is
    // reading. Tampering is not confined to the top level, so neither is
    // the check — and serde does not cascade `deny_unknown_fields`, so
    // every container has to be listed here or the claim is only true of
    // the ones that are. `WalkwayEndpoint` is the reason this is a table:
    // it is the one lockfile container declared outside `lock::schema`,
    // and it sits two levels down.
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
    // The realistic shape of a later format: a higher version *and* a key
    // this build has never heard of. Read after the strict parse, the
    // unknown key would be reported first and the reader told its document
    // is malformed — when the truth is that this build is too old. That is
    // the whole reason the version leads the document.
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
    // Not cosmetic. A file with no final newline makes `git diff` report
    // `\ No newline at end of file` on every change, `cat` run the next
    // file's first line onto this one's last, and a `>>` append corrupt the
    // document.
    //
    // Both shapes are exercised because the trailing field is the one at
    // risk: `member_version_sensitivity` is written last, so an encoder
    // that closes an empty flow sequence (`[]`) without a break would end
    // the whole document at `]`.
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
fn l8_sample_lockfile_yaml_snapshot() {
    // AC L8: stable YAML snapshot, so a future struct/field reshuffle is
    // caught before downstream tooling that greps the lockfile breaks.
    let body = serde_norway::to_string(&sample_lockfile()).expect("encode");
    insta::assert_snapshot!(body);
}
