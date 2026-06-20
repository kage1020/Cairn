//! AC L1–L8 for the lockfile module.

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_core::lock::{
    HashHex, LockInputs, LockTarget, Lockfile, MemberSensitivity, hash_resolved_ir, hash_source,
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
        h.0,
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
    let h1 = hash_resolved_ir(&ir);
    let h2 = hash_resolved_ir(&ir);
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

    let h1 = hash_resolved_ir(&unit_ir(p1));
    let h2 = hash_resolved_ir(&unit_ir(p2));
    assert_ne!(h1, h2);
}

fn sample_lockfile() -> Lockfile {
    Lockfile {
        source_hash: HashHex::from_bytes(b"cottage"),
        cairn_version: "2026.09".to_owned(),
        target: LockTarget {
            edition: "java".to_owned(),
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
    assert_eq!(HashHex::zero().0, HashHex::ZERO_STR);
}

#[test]
fn l8_sample_lockfile_yaml_snapshot() {
    // AC L8: stable YAML snapshot, so a future struct/field reshuffle is
    // caught before downstream tooling that greps the lockfile breaks.
    let body = serde_yml::to_string(&sample_lockfile()).expect("encode");
    insta::assert_snapshot!(body);
}
