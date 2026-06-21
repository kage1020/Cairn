//! AC L1–L8 for the lockfile module.

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_core::lock::{
    HashHex, HashParseError, LockEdition, LockInputs, LockTarget, Lockfile, MemberSensitivity,
    hash_resolved_ir, hash_source,
};
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
    }
}

#[test]
fn l5_lockfile_roundtrips_through_yaml() {
    // AC L5: serialise → deserialise → equal.
    let lf = sample_lockfile();
    let body = serde_yml::to_string(&lf).expect("encode");
    let parsed: Lockfile = serde_yml::from_str(&body).expect("decode");
    assert_eq!(lf, parsed);
}

#[test]
fn l6_lockfile_yaml_key_order_matches_spec() {
    // AC L6: top-level keys appear in the order spec §10.6 prints.
    let body = serde_yml::to_string(&sample_lockfile()).expect("encode");
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
    let err = serde_yml::from_str::<Lockfile>(&body).expect_err("typo edition");
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
    let err = serde_yml::from_str::<Lockfile>(&body).expect_err("bad hash");
    assert!(
        err.to_string().contains("prefix"),
        "expected hash prefix error, got {err}",
    );
}

#[test]
fn l8_sample_lockfile_yaml_snapshot() {
    // AC L8: stable YAML snapshot, so a future struct/field reshuffle is
    // caught before downstream tooling that greps the lockfile breaks.
    let body = serde_yml::to_string(&sample_lockfile()).expect("encode");
    insta::assert_snapshot!(body);
}
