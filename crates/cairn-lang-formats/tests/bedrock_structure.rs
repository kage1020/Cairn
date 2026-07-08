//! AC M1–M6 for the Bedrock `.mcstructure` backend.
//!
//! Format reference: wiki.bedrock.dev's "mcstructure" page — little-endian
//! uncompressed NBT, `block_indices` in two layers ordered with Z as the
//! fastest axis, palette entries of `{ name, states, version }`.

use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_formats::bedrock_structure::{
    BedrockStructureError, build_mcstructure_tag, write_mcstructure,
};
use cairn_lang_formats::data_version::{BedrockTarget, resolve_bedrock_target};
use cairn_lang_formats::java_structure::{OutputExt, output_filename};
use cairn_lang_nbt::tag::{Compound, Tag};

fn target_1_21_60() -> BedrockTarget {
    resolve_bedrock_target("1.21.60").expect("known target")
}

/// The `.mcstructure` block-palette `version` integer packs
/// `(major, minor, patch, revision)` one byte each, major in the high
/// byte. Deriving the expected value from the parts — rather than
/// restating the packed integer — pins that the JSON table and this
/// documented formula agree.
fn block_version(major: i32, minor: i32, patch: i32, revision: i32) -> i32 {
    (major << 24) | (minor << 16) | (patch << 8) | revision
}

/// Non-cubic 2×3×4 array with two marked voxels. Distinct per-axis sizes
/// make any axis-order mistake in the index math shift at least one of
/// the expected flat indices.
fn asymmetric_array() -> (BlockArray, PaletteIndex) {
    let mut palette = Palette::new_with_air();
    let planks = palette.intern(BlockState::bare("minecraft:oak_planks"));
    let dims = Dims { x: 2, y: 3, z: 4 };
    let mut voxels = vec![PaletteIndex::AIR; dims.volume()];
    voxels[dims.index(0, 1, 2).unwrap()] = planks;
    voxels[dims.index(1, 2, 3).unwrap()] = planks;
    (
        BlockArray {
            dims,
            palette,
            voxels,
            block_entities: vec![],
            entities: vec![],
            source_scope: "struct::asym".to_owned(),
        },
        planks,
    )
}

fn structure_compound(root: &Compound) -> &Compound {
    match root.entries.get("structure") {
        Some(Tag::Compound(c)) => c,
        other => panic!("structure is not a Compound: {other:?}"),
    }
}

#[test]
fn m1_root_carries_format_version_size_structure_and_origin() {
    // AC M1: root key set and scalar values.
    let (ba, _) = asymmetric_array();
    let root = build_mcstructure_tag(&ba, &target_1_21_60()).expect("build");
    let keys: Vec<&String> = root.entries.keys().collect();
    assert_eq!(
        keys,
        vec![
            "format_version",
            "size",
            "structure",
            "structure_world_origin"
        ]
    );
    assert_eq!(root.entries.get("format_version"), Some(&Tag::Int(1)));
    match &root.entries["size"] {
        Tag::List(l) => assert_eq!(l.items, vec![Tag::Int(2), Tag::Int(3), Tag::Int(4)]),
        other => panic!("size is not a List: {other:?}"),
    }
    match &root.entries["structure_world_origin"] {
        Tag::List(l) => assert_eq!(l.items, vec![Tag::Int(0), Tag::Int(0), Tag::Int(0)]),
        other => panic!("structure_world_origin is not a List: {other:?}"),
    }
}

#[test]
fn m2_block_indices_are_z_fastest_with_minus_one_second_layer() {
    // AC M2: layer 0 is the palette indices in (x, y, z) nesting with z
    // fastest — flat index (x * size_y + y) * size_z + z — and layer 1 is
    // volume × -1 (no waterlog layer authored).
    let (ba, planks) = asymmetric_array();
    let root = build_mcstructure_tag(&ba, &target_1_21_60()).expect("build");
    let structure = structure_compound(&root);
    let layers = match structure.entries.get("block_indices") {
        Some(Tag::List(l)) => l,
        other => panic!("block_indices is not a List: {other:?}"),
    };
    assert_eq!(layers.items.len(), 2);

    let block_layer = match &layers.items[0] {
        Tag::List(l) => &l.items,
        other => panic!("layer 0 is not a List: {other:?}"),
    };
    assert_eq!(block_layer.len(), 24);
    let mut expected = vec![Tag::Int(0); 24];
    // (x=0, y=1, z=2) → (0 * 3 + 1) * 4 + 2 = 6.
    expected[6] = Tag::Int(i32::from(planks.0));
    // (x=1, y=2, z=3) → (1 * 3 + 2) * 4 + 3 = 23.
    expected[23] = Tag::Int(i32::from(planks.0));
    assert_eq!(block_layer, &expected);

    let waterlog_layer = match &layers.items[1] {
        Tag::List(l) => &l.items,
        other => panic!("layer 1 is not a List: {other:?}"),
    };
    assert_eq!(waterlog_layer, &vec![Tag::Int(-1); 24]);
}

#[test]
fn m3_palette_entries_carry_name_empty_states_and_version() {
    // AC M3: `structure.palette.default.block_palette[i]` mirrors the IR
    // palette order, `states` is an empty compound (stateless palettes
    // only in this cut), and `version` is the target's block version.
    // `block_position_data` is present and empty, and `entities` is an
    // empty list.
    let (ba, _) = asymmetric_array();
    let target = target_1_21_60();
    // 1.21.60's wiki-confirmed block-palette marker is 1.21.60.33.
    assert_eq!(target.block_version, block_version(1, 21, 60, 33));
    let root = build_mcstructure_tag(&ba, &target).expect("build");
    let structure = structure_compound(&root);

    match structure.entries.get("entities") {
        Some(Tag::List(l)) => assert!(l.items.is_empty()),
        other => panic!("entities is not a List: {other:?}"),
    }

    let default = match structure.entries.get("palette") {
        Some(Tag::Compound(p)) => match p.entries.get("default") {
            Some(Tag::Compound(d)) => d,
            other => panic!("palette.default is not a Compound: {other:?}"),
        },
        other => panic!("palette is not a Compound: {other:?}"),
    };
    let entries = match default.entries.get("block_palette") {
        Some(Tag::List(l)) => &l.items,
        other => panic!("block_palette is not a List: {other:?}"),
    };
    assert_eq!(entries.len(), 2);
    let expected_names = ["minecraft:air", "minecraft:oak_planks"];
    for (entry, expected_name) in entries.iter().zip(expected_names) {
        let c = match entry {
            Tag::Compound(c) => c,
            other => panic!("palette entry is not a Compound: {other:?}"),
        };
        assert_eq!(
            c.entries.get("name"),
            Some(&Tag::String(expected_name.to_owned()))
        );
        assert_eq!(
            c.entries.get("states"),
            Some(&Tag::Compound(Compound::new()))
        );
        assert_eq!(
            c.entries.get("version"),
            Some(&Tag::Int(target.block_version))
        );
    }
    match default.entries.get("block_position_data") {
        Some(Tag::Compound(c)) => assert!(c.entries.is_empty()),
        other => panic!("block_position_data is not a Compound: {other:?}"),
    }
}

#[test]
fn m4_stateful_palette_entry_fails_loud() {
    // AC M4: a palette entry with blockstate properties is a hard error
    // (spec versioning-editions §10.4 — no silent substitution/dropping)
    // whose message carries the self-correction triple.
    let mut palette = Palette::new_with_air();
    let mut stairs = BlockState::bare("minecraft:spruce_stairs");
    stairs
        .properties
        .insert("facing".to_owned(), "north".to_owned());
    let idx = palette.intern(stairs);
    let ba = BlockArray {
        dims: Dims { x: 1, y: 1, z: 1 },
        palette,
        voxels: vec![idx],
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::stateful".to_owned(),
    };
    let err = build_mcstructure_tag(&ba, &target_1_21_60()).expect_err("stateful entry");
    match &err {
        BedrockStructureError::StatefulPaletteEntry { id, .. } => {
            assert_eq!(id, "minecraft:spruce_stairs");
        }
        other => panic!("expected StatefulPaletteEntry, got {other:?}"),
    }
    let msg = err.to_string();
    assert!(msg.contains("minecraft:spruce_stairs"), "got: {msg}");
    assert!(msg.contains("facing"), "got: {msg}");
    assert!(msg.contains("--edition java"), "suggested fix, got: {msg}");
}

#[test]
fn m5_abstract_palette_entry_fails_loud() {
    // AC M5: an unresolved abstract token is rejected the same way the
    // Java backend rejects it.
    let mut palette = Palette::new_with_air();
    let idx = palette.intern(BlockState::bare("@cobblestone"));
    let ba = BlockArray {
        dims: Dims { x: 1, y: 1, z: 1 },
        palette,
        voxels: vec![idx],
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::abstract".to_owned(),
    };
    let err = build_mcstructure_tag(&ba, &target_1_21_60()).expect_err("abstract entry");
    assert!(matches!(
        err,
        BedrockStructureError::AbstractPaletteEntry { ref id } if id == "@cobblestone"
    ));
}

#[test]
fn m6_write_mcstructure_is_uncompressed_little_endian() {
    // AC M6: the on-disk bytes are raw NBT — an unnamed root compound
    // (0x0a + u16 zero length), not a gzip stream (0x1f 0x8b) — and the
    // filename helper produces `.mcstructure` names alongside the `.nbt`
    // ones.
    let (ba, _) = asymmetric_array();
    let root = build_mcstructure_tag(&ba, &target_1_21_60()).expect("build");
    let mut buf = Vec::new();
    write_mcstructure(&mut buf, &root).expect("write");
    assert_eq!(&buf[..3], &[0x0a, 0x00, 0x00]);
    // format_version entry follows: Int tag id + name length 14 LE.
    assert_eq!(&buf[3..6], &[0x03, 0x0e, 0x00]);

    assert_eq!(
        output_filename("struct::asym", OutputExt::Mcstructure),
        "asym.mcstructure"
    );
    assert_eq!(output_filename("struct::asym", OutputExt::Nbt), "asym.nbt");
}
