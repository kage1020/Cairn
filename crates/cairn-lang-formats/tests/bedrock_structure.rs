//! Acceptance criteria 1–6 for the Bedrock `.mcstructure` backend.
//!
//! Format reference: wiki.bedrock.dev's "mcstructure" page — little-endian
//! uncompressed NBT, `block_indices` in two layers ordered with Z as the
//! fastest axis, palette entries of `{ name, states, version }`.

use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_core::resolve::DroppedIntent;
use cairn_lang_formats::bedrock_state::{BedrockStateError, degradation_message};
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
    // AC 1: root key set and scalar values.
    let (ba, _) = asymmetric_array();
    let (root, _notes) = build_mcstructure_tag(&ba, &target_1_21_60()).expect("build");
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
    // AC 2: layer 0 is the palette indices in (x, y, z) nesting with z
    // fastest — flat index (x * size_y + y) * size_z + z — and layer 1 is
    // volume × -1 (no waterlog layer authored).
    let (ba, planks) = asymmetric_array();
    let (root, _notes) = build_mcstructure_tag(&ba, &target_1_21_60()).expect("build");
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
    // AC 3: `structure.palette.default.block_palette[i]` mirrors the IR
    // palette order, `states` is an empty compound for a bare (property-free)
    // block, and `version` is the target's block version.
    // `block_position_data` is present and empty, and `entities` is an
    // empty list.
    let (ba, _) = asymmetric_array();
    let target = target_1_21_60();
    // 1.21.60's wiki-confirmed block-palette marker is 1.21.60.33.
    assert_eq!(target.block_version, block_version(1, 21, 60, 33));
    let (root, _notes) = build_mcstructure_tag(&ba, &target).expect("build");
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

/// Build a single-voxel array whose one non-air palette entry is `state`.
fn single_stateful_array(state: BlockState) -> BlockArray {
    let mut palette = Palette::new_with_air();
    let idx = palette.intern(state);
    BlockArray {
        dims: Dims { x: 1, y: 1, z: 1 },
        palette,
        voxels: vec![idx],
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::stateful".to_owned(),
    }
}

/// The typed `states` compound of the last (non-air) block-palette entry.
fn last_palette_states(root: &Compound) -> &Compound {
    let structure = structure_compound(root);
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
    match entries.last() {
        Some(Tag::Compound(c)) => match c.entries.get("states") {
            Some(Tag::Compound(s)) => s,
            other => panic!("states is not a Compound: {other:?}"),
        },
        other => panic!("no last palette entry: {other:?}"),
    }
}

#[test]
fn m4_unmappable_stateful_entry_fails_loud() {
    // AC8: a stateful entry outside a mapped family is a hard error
    // (`spec/versioning-editions` "Fail-loud and minimum-version inference"
    // — no silent substitution/dropping) whose message carries the
    // self-correction triple. Stairs are now mapped, so fail-loud is pinned
    // on a non-stair stateful block.
    let mut door = BlockState::bare("minecraft:oak_door");
    door.properties
        .insert("facing".to_owned(), "north".to_owned());
    let ba = single_stateful_array(door);
    let err = build_mcstructure_tag(&ba, &target_1_21_60()).expect_err("stateful entry");
    match &err {
        BedrockStructureError::State(BedrockStateError::UnmappableBlock { id, .. }) => {
            assert_eq!(id, "minecraft:oak_door");
        }
        other => panic!("expected State(UnmappableBlock), got {other:?}"),
    }
    let msg = err.to_string();
    assert!(msg.contains("minecraft:oak_door"), "got: {msg}");
    assert!(msg.contains("facing=north"), "got: {msg}");
    assert!(msg.contains("--edition java"), "suggested fix, got: {msg}");
}

#[test]
fn m4b_stair_states_map_to_bedrock_vocabulary() {
    // AC9: a stair palette entry's Java facing/half map to Bedrock's
    // weirdo_direction/upside_down_bit as typed states; a straight shape is
    // lossless (no degradation note).
    let mut stairs = BlockState::bare("minecraft:dark_oak_stairs");
    stairs
        .properties
        .insert("facing".to_owned(), "south".to_owned());
    stairs
        .properties
        .insert("half".to_owned(), "top".to_owned());
    stairs
        .properties
        .insert("shape".to_owned(), "straight".to_owned());
    let ba = single_stateful_array(stairs);
    let (root, notes) = build_mcstructure_tag(&ba, &target_1_21_60()).expect("build");
    assert!(notes.is_empty(), "straight shape is lossless: {notes:?}");
    let states = last_palette_states(&root);
    assert_eq!(states.entries.get("weirdo_direction"), Some(&Tag::Int(2)));
    assert_eq!(states.entries.get("upside_down_bit"), Some(&Tag::Byte(1)));
    assert_eq!(states.entries.len(), 2);
}

#[test]
fn m4c_non_straight_stair_shape_degrades() {
    // AC10: a non-straight stair shape has no Bedrock state; the build
    // succeeds but raises exactly one W_INTENT_DEGRADED note, keyed by the
    // id and naming the Java state the entry came from.
    let mut stairs = BlockState::bare("minecraft:oak_stairs");
    stairs
        .properties
        .insert("facing".to_owned(), "east".to_owned());
    stairs
        .properties
        .insert("half".to_owned(), "bottom".to_owned());
    stairs
        .properties
        .insert("shape".to_owned(), "outer_left".to_owned());
    let ba = single_stateful_array(stairs);
    let (root, notes) = build_mcstructure_tag(&ba, &target_1_21_60()).expect("build");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].id, "minecraft:oak_stairs");
    // Against the writer rather than a literal: `cairn info`'s degraded
    // note renders the same clause from the same function, and a literal
    // here would let the build's wording drift away from the report's
    // without a test noticing.
    assert_eq!(
        notes[0].message,
        degradation_message(
            "minecraft:oak_stairs[facing=east,half=bottom,shape=outer_left]",
            &DroppedIntent::Shape {
                value: "outer_left".to_owned(),
            },
        ),
    );
    // The mappable intent still lands in `states`.
    let states = last_palette_states(&root);
    assert_eq!(states.entries.get("weirdo_direction"), Some(&Tag::Int(0)));
    assert_eq!(states.entries.get("upside_down_bit"), Some(&Tag::Byte(0)));
}

#[test]
fn m5_abstract_palette_entry_fails_loud() {
    // AC 5: an unresolved abstract token is rejected the same way the
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
    // AC 6: the on-disk bytes are raw NBT — an unnamed root compound
    // (0x0a + u16 zero length), not a gzip stream (0x1f 0x8b) — and the
    // filename helper produces `.mcstructure` names alongside the `.nbt`
    // ones.
    let (ba, _) = asymmetric_array();
    let (root, _notes) = build_mcstructure_tag(&ba, &target_1_21_60()).expect("build");
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

#[test]
fn a_voxel_naming_a_slot_the_palette_does_not_have_is_refused() {
    // Same hole as the Java backend's, reached the same way: public struct
    // fields plus a public builder. The index was written into `block_indices`
    // as an `i32` with nothing to point at.
    let (mut ba, _) = asymmetric_array();
    assert_eq!(ba.palette.entries.len(), 2, "fixture palette");
    ba.voxels[0] = PaletteIndex(7);
    let err = build_mcstructure_tag(&ba, &target_1_21_60()).expect_err("out-of-range index");
    assert!(
        matches!(
            err,
            BedrockStructureError::PaletteIndexOutOfRange { index: 7, len: 2 }
        ),
        "unexpected error: {err}"
    );
}

fn stair(facing: &str, shape: &str) -> BlockState {
    let mut stairs = BlockState::bare("minecraft:oak_stairs");
    for (key, value) in [("facing", facing), ("half", "bottom"), ("shape", shape)] {
        stairs.properties.insert(key.to_owned(), value.to_owned());
    }
    stairs
}

/// The `block_palette` entries of a built root.
fn block_palette(root: &Compound) -> &Vec<Tag> {
    let structure = structure_compound(root);
    let Some(Tag::Compound(palette)) = structure.entries.get("palette") else {
        panic!("palette is not a Compound");
    };
    let Some(Tag::Compound(default)) = palette.entries.get("default") else {
        panic!("palette.default is not a Compound");
    };
    let Some(Tag::List(entries)) = default.entries.get("block_palette") else {
        panic!("block_palette is not a List");
    };
    &entries.items
}

/// Layer 0 of `block_indices`, as integers.
fn block_layer(root: &Compound) -> Vec<i32> {
    let Some(Tag::List(layers)) = structure_compound(root).entries.get("block_indices") else {
        panic!("block_indices is not a List");
    };
    let Tag::List(layer) = &layers.items[0] else {
        panic!("layer 0 is not a List");
    };
    layer
        .items
        .iter()
        .map(|tag| match tag {
            Tag::Int(slot) => *slot,
            other => panic!("block index is not an Int: {other:?}"),
        })
        .collect()
}

/// Stairs that differ only in `shape` are distinct Java states and one
/// Bedrock block once `shape` is dropped. The written palette is a set:
/// they share one `block_palette` entry, and every voxel is written as
/// the slot of the block it holds. Each degradation note names its own
/// Java state, so no two read the same.
#[test]
fn stairs_that_differ_only_in_a_dropped_shape_share_one_palette_entry() {
    let mut palette = Palette::new_with_air();
    let java = [
        stair("north", "outer_left"),
        stair("north", "straight"),
        stair("south", "outer_right"),
        stair("north", "outer_right"),
        stair("south", "outer_left"),
    ];
    let mut voxels = vec![PaletteIndex::AIR];
    voxels.extend(java.iter().map(|state| palette.intern(state.clone())));
    let ba = BlockArray {
        dims: Dims { x: 1, y: 1, z: 6 },
        palette,
        voxels,
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::corners".to_owned(),
    };
    let (root, notes) = build_mcstructure_tag(&ba, &target_1_21_60()).expect("build");

    let entries = block_palette(&root);
    for (i, a) in entries.iter().enumerate() {
        for b in &entries[i + 1..] {
            assert_ne!(a, b, "block_palette holds one block twice: {entries:?}");
        }
    }
    // Air, one north stair, one south stair.
    assert_eq!(entries.len(), 3, "{entries:?}");
    let Tag::Compound(air) = &entries[0] else {
        panic!("slot 0 is not a Compound");
    };
    assert_eq!(
        air.entries.get("name"),
        Some(&Tag::String("minecraft:air".to_owned()))
    );

    // Every voxel is written as a slot the palette has, and that slot holds
    // the block the voxel's own entry translates to.
    let layer = block_layer(&root);
    let direction = |slot: i32| {
        let Tag::Compound(entry) = &entries[usize::try_from(slot).expect("slot")] else {
            panic!("entry is not a Compound");
        };
        let Some(Tag::Compound(states)) = entry.entries.get("states") else {
            panic!("states is not a Compound");
        };
        states.entries.get("weirdo_direction").cloned()
    };
    assert_eq!(layer[0], 0, "air stays at slot 0");
    // `weirdo_direction` 3 is north and 2 is south.
    let expected = [3, 3, 2, 3, 2];
    for (z, want) in expected.into_iter().enumerate() {
        assert_eq!(
            direction(layer[z + 1]),
            Some(Tag::Int(want)),
            "voxel {}",
            z + 1
        );
    }
    assert_eq!(
        layer[1], layer[2],
        "north outer_left and north straight are one block"
    );
    assert_eq!(layer[1], layer[4]);
    assert_eq!(layer[3], layer[5]);

    // Four corner shapes degrade; `straight` drops losslessly.
    let messages: Vec<&str> = notes.iter().map(|note| note.message.as_str()).collect();
    assert_eq!(messages.len(), 4, "{messages:?}");
    for (i, a) in messages.iter().enumerate() {
        for b in &messages[i + 1..] {
            assert_ne!(a, b, "two notes read the same: {messages:?}");
        }
    }
    assert_eq!(
        messages[0],
        degradation_message(
            "minecraft:oak_stairs[facing=north,half=bottom,shape=outer_left]",
            &DroppedIntent::Shape {
                value: "outer_left".to_owned(),
            },
        ),
    );
    assert!(notes.iter().all(|note| note.id == "minecraft:oak_stairs"));

    // The streamed writer writes the same merged structure as the tree.
    let mut tree = Vec::new();
    write_mcstructure(&mut tree, &root).expect("write tree");
    let (prepared, _) =
        cairn_lang_formats::bedrock_structure::prepare_mcstructure(&ba, &target_1_21_60())
            .expect("prepare");
    let mut streamed = Vec::new();
    prepared.write(&mut streamed).expect("stream");
    assert_eq!(streamed, tree);
}
