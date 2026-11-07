//! The Bedrock `.mcstructure` tag tree and bytes, against wiki.bedrock.dev's "mcstructure" page.

use cairn_lang_core::block_array::{
    BlockArray, BlockState, Dims, PALETTE_CAPACITY, Palette, PaletteIndex,
};
use cairn_lang_formats::bedrock_state::BedrockStateError;
use cairn_lang_formats::bedrock_structure::{
    BedrockStructureError, McStructure, ParityNote, build_mcstructure_tag, prepare_mcstructure,
    write_mcstructure,
};
use cairn_lang_formats::data_version::{BedrockTarget, resolve_bedrock_target};
use cairn_lang_formats::java_structure::{OutputExt, output_filename};
use cairn_lang_nbt::tag::{Compound, Tag};

use crate::common::{
    block_palette, default_palette, dropped_shape, note_fields, states_for, structure_compound,
};

fn target_1_21_60() -> BedrockTarget {
    resolve_bedrock_target("1.21.60").expect("known target")
}

/// `ba`'s tag tree at `target`, through the built-in pack's state map.
fn build_at(
    ba: &BlockArray,
    target: &BedrockTarget,
) -> Result<(Compound, Vec<ParityNote>), BedrockStructureError> {
    build_mcstructure_tag(ba, target, &states_for(target))
}

/// [`build_at`] 1.21.60, whose stairs have no `shape` state.
fn build(ba: &BlockArray) -> Result<(Compound, Vec<ParityNote>), BedrockStructureError> {
    build_at(ba, &target_1_21_60())
}

/// The bytes `prepare_mcstructure` streams for `ba` at `target`.
fn streamed_at(ba: &BlockArray, target: &BedrockTarget) -> Vec<u8> {
    let (prepared, _): (McStructure<'_>, _) =
        prepare_mcstructure(ba, target, &states_for(target)).expect("prepare");
    let mut streamed = Vec::new();
    prepared.write(&mut streamed).expect("stream");
    streamed
}

/// The `-1`-or-slot water layer of `block_indices`, as integers.
fn water_layer(root: &Compound) -> Vec<i32> {
    layer(root, 1)
}

/// The packed palette `version`: a byte each of major, minor, patch and revision.
fn block_version(major: i32, minor: i32, patch: i32, revision: i32) -> i32 {
    (major << 24) | (minor << 16) | (patch << 8) | revision
}

/// A 2×3×4 array with two marked voxels, so an axis-order mistake moves a flat index.
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

#[test]
fn root_carries_format_version_size_structure_and_origin() {
    let (ba, _) = asymmetric_array();
    let (root, _notes) = build(&ba).expect("build");
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
fn block_indices_are_z_fastest_with_minus_one_second_layer() {
    let (ba, planks) = asymmetric_array();
    let (root, _notes) = build(&ba).expect("build");
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
fn palette_entries_carry_name_empty_states_and_version() {
    let (ba, _) = asymmetric_array();
    let target = target_1_21_60();
    // 1.21.60's wiki-confirmed block-palette marker is 1.21.60.33.
    assert_eq!(target.block_version, block_version(1, 21, 60, 33));
    let (root, _notes) = build_at(&ba, &target).expect("build");
    let structure = structure_compound(&root);

    match structure.entries.get("entities") {
        Some(Tag::List(l)) => assert_eq!(l.items, [] as [Tag; 0]),
        other => panic!("entities is not a List: {other:?}"),
    }

    let entries = block_palette(&root);
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
    match default_palette(&root).entries.get("block_position_data") {
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
    match block_palette(root).last() {
        Some(Tag::Compound(c)) => match c.entries.get("states") {
            Some(Tag::Compound(s)) => s,
            other => panic!("states is not a Compound: {other:?}"),
        },
        other => panic!("no last palette entry: {other:?}"),
    }
}

#[test]
fn an_entry_the_versions_state_map_has_no_form_for_fails_loud() {
    let mut frame = BlockState::bare("minecraft:frame");
    frame
        .properties
        .insert("facing".to_owned(), "north".to_owned());
    let err = build(&single_stateful_array(frame)).expect_err("unmapped entry");
    let BedrockStructureError::State(err) = err else {
        panic!("expected a state refusal, got {err:?}");
    };
    assert_eq!(
        err,
        BedrockStateError::Unmapped {
            id: "minecraft:frame".to_owned(),
            properties: "facing=north".to_owned(),
            mc_version: "1.21.60".to_owned(),
            reason: "no Java block maps onto `minecraft:frame` there, so the canonical \
                     vocabulary names none of its states"
                .to_owned(),
        },
    );
}

#[test]
fn a_door_writes_the_states_of_the_java_door_mapped_onto_its_bedrock_id() {
    let mut door = BlockState::bare("minecraft:wooden_door");
    door.properties
        .insert("facing".to_owned(), "north".to_owned());
    let (root, notes) = build(&single_stateful_array(door)).expect("build");
    assert_eq!(note_fields(&notes), []);
    let states: Vec<(&str, &Tag)> = last_palette_states(&root)
        .entries
        .iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    assert_eq!(
        states,
        [
            ("door_hinge_bit", &Tag::Byte(0)),
            // GeyserMC's mapping: a Bedrock door's direction turns a quarter from Java's facing.
            (
                "minecraft:cardinal_direction",
                &Tag::String("east".to_owned())
            ),
            ("open_bit", &Tag::Byte(0)),
            ("upper_block_bit", &Tag::Byte(0)),
        ],
    );
}

#[test]
fn a_waterlogged_entry_holds_water_in_the_second_layer() {
    let mut wet = stair(OAK_STAIRS, "north", "straight");
    wet.properties
        .insert("waterlogged".to_owned(), "true".to_owned());
    let ba = row_of(&[wet, stair(OAK_STAIRS, "north", "straight")]);
    let target = target_1_21_60();
    let (root, _) = build_at(&ba, &target).expect("build");

    let entries = block_palette(&root);
    let names: Vec<&str> = entries.iter().map(name_of).collect();
    assert_eq!(names, ["minecraft:air", OAK_STAIRS, "minecraft:water"]);
    assert_eq!(
        states_of(&entries[2]).entries.get("liquid_depth"),
        Some(&Tag::Int(0)),
    );
    assert_eq!(block_layer(&root), [0, 1, 1], "both stairs are one block");
    assert_eq!(water_layer(&root), [-1, 2, -1]);

    let mut tree = Vec::new();
    write_mcstructure(&mut tree, &root).expect("write tree");
    assert_eq!(streamed_at(&ba, &target), tree);
}

/// `state` with `waterlogged=true`.
fn wet(mut state: BlockState) -> BlockState {
    state
        .properties
        .insert("waterlogged".to_owned(), "true".to_owned());
    state
}

/// The names `block_palette` lists, in slot order.
fn palette_names(root: &Compound) -> Vec<&str> {
    block_palette(root).iter().map(name_of).collect()
}

#[test]
fn entries_that_merge_into_one_block_keep_their_own_water() {
    let ba = row_of(&[
        wet(stair(OAK_STAIRS, "north", "outer_left")),
        stair(OAK_STAIRS, "north", "straight"),
    ]);
    let (root, _) = build(&ba).expect("build");
    assert_eq!(
        palette_names(&root),
        ["minecraft:air", OAK_STAIRS, "minecraft:water"]
    );
    assert_eq!(block_layer(&root), [0, 1, 1]);
    assert_eq!(water_layer(&root), [-1, 2, -1]);
}

#[test]
fn two_waterlogged_blocks_share_one_water_slot() {
    let ba = row_of(&[
        wet(stair(OAK_STAIRS, "north", "straight")),
        wet(stair(OAK_STAIRS, "south", "straight")),
    ]);
    let (root, _) = build(&ba).expect("build");
    assert_eq!(
        palette_names(&root),
        ["minecraft:air", OAK_STAIRS, OAK_STAIRS, "minecraft:water"]
    );
    assert_eq!(block_layer(&root), [0, 1, 2]);
    assert_eq!(water_layer(&root), [-1, 3, 3]);
}

#[test]
fn a_water_block_shares_the_slot_a_waterlogged_block_holds() {
    let ba = row_of(&[
        BlockState::bare("minecraft:water"),
        wet(stair(OAK_STAIRS, "north", "straight")),
    ]);
    let (root, _) = build(&ba).expect("build");
    assert_eq!(
        palette_names(&root),
        ["minecraft:air", "minecraft:water", OAK_STAIRS]
    );
    assert_eq!(block_layer(&root), [0, 1, 2]);
    assert_eq!(water_layer(&root), [-1, -1, 1]);
}

#[test]
fn the_water_layer_holds_still_water_at_every_version() {
    let ba = row_of(&[wet(stair(OAK_STAIRS, "north", "straight"))]);
    for version in ["1.21.0", "1.21.40", "1.21.60", "1.26.60"] {
        let target = resolve_bedrock_target(version).expect("known target");
        let (root, _) = build_at(&ba, &target).expect("build");
        let entries = block_palette(&root);
        assert_eq!(name_of(&entries[2]), "minecraft:water", "at {version}");
        assert_eq!(
            states_of(&entries[2]).entries.iter().collect::<Vec<_>>(),
            [(&"liquid_depth".to_owned(), &Tag::Int(0))],
            "at {version}",
        );
    }
}

#[test]
fn a_block_bedrock_keeps_only_under_water_holds_water_without_waterlogged() {
    let ba = row_of(&[
        BlockState::bare("minecraft:kelp"),
        BlockState::bare("minecraft:seagrass"),
        BlockState::bare("minecraft:oak_planks"),
    ]);
    let (root, _) = build(&ba).expect("build");
    assert_eq!(
        palette_names(&root),
        [
            "minecraft:air",
            "minecraft:kelp",
            "minecraft:seagrass",
            "minecraft:oak_planks",
            "minecraft:water",
        ]
    );
    assert_eq!(water_layer(&root), [-1, 4, 4, -1]);
}

#[test]
fn a_slab_and_its_double_spelling_share_one_block() {
    let mut double = BlockState::bare("minecraft:oak_slab");
    double
        .properties
        .insert("type".to_owned(), "double".to_owned());
    let ba = row_of(&[double, BlockState::bare("minecraft:oak_double_slab")]);
    let (root, notes) = build(&ba).expect("build");
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(
        palette_names(&root),
        ["minecraft:air", "minecraft:oak_double_slab"]
    );
    assert_eq!(block_layer(&root), [0, 1, 1]);
}

#[test]
fn stair_states_map_to_bedrock_vocabulary() {
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
    let (root, notes) = build(&ba).expect("build");
    assert!(notes.is_empty(), "straight shape is lossless: {notes:?}");
    let states = last_palette_states(&root);
    assert_eq!(states.entries.get("weirdo_direction"), Some(&Tag::Int(2)));
    assert_eq!(states.entries.get("upside_down_bit"), Some(&Tag::Byte(1)));
    assert_eq!(states.entries.len(), 2);
}

#[test]
fn non_straight_stair_shape_degrades() {
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
    let (root, notes) = build(&ba).expect("build");
    assert_eq!(
        note_fields(&notes),
        [(
            "minecraft:oak_stairs",
            "facing=east,half=bottom,shape=outer_left",
            &dropped_shape("outer_left", &["1.21.60"]),
        )],
    );
    let states = last_palette_states(&root);
    assert_eq!(states.entries.get("weirdo_direction"), Some(&Tag::Int(0)));
    assert_eq!(states.entries.get("upside_down_bit"), Some(&Tag::Byte(0)));
}

#[test]
fn abstract_palette_entry_fails_loud() {
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
    let err = build(&ba).expect_err("abstract entry");
    assert!(matches!(
        err,
        BedrockStructureError::AbstractPaletteEntry { ref id } if id == "@cobblestone"
    ));
}

#[test]
fn write_mcstructure_is_uncompressed_little_endian() {
    let (ba, _) = asymmetric_array();
    let (root, _notes) = build(&ba).expect("build");
    let mut buf = Vec::new();
    write_mcstructure(&mut buf, &root).expect("write");
    assert_eq!(&buf[..3], &[0x0a, 0x00, 0x00]);
    // Then `format_version`: an Int tag id and a name length of 14.
    assert_eq!(&buf[3..6], &[0x03, 0x0e, 0x00]);

    assert_eq!(
        output_filename("asym", OutputExt::Mcstructure),
        "asym.mcstructure"
    );
    assert_eq!(output_filename("asym", OutputExt::Nbt), "asym.nbt");
}

#[test]
fn a_voxel_naming_a_slot_the_palette_does_not_have_is_refused() {
    let (mut ba, _) = asymmetric_array();
    assert_eq!(ba.palette.entries.len(), 2, "fixture palette");
    ba.voxels[0] = PaletteIndex(7);
    let err = build(&ba).expect_err("out-of-range index");
    assert!(
        matches!(
            err,
            BedrockStructureError::PaletteIndexOutOfRange { index: 7, len: 2 }
        ),
        "unexpected error: {err}"
    );
}

/// A bottom-half stair of `id`, its properties in the order lowering interns them.
fn stair(id: &str, facing: &str, shape: &str) -> BlockState {
    let mut stairs = BlockState::bare(id);
    for (key, value) in [("facing", facing), ("half", "bottom"), ("shape", shape)] {
        stairs.properties.insert(key.to_owned(), value.to_owned());
    }
    stairs
}

/// A row along z: air, then one voxel per state, each its own palette entry.
fn row_of(states: &[BlockState]) -> BlockArray {
    let mut palette = Palette::new_with_air();
    let mut voxels = vec![PaletteIndex::AIR];
    voxels.extend(states.iter().map(|state| palette.intern(state.clone())));
    BlockArray {
        dims: Dims {
            x: 1,
            y: 1,
            z: u32::try_from(voxels.len()).expect("a short row"),
        },
        palette,
        voxels,
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::row".to_owned(),
    }
}

/// Layer 0 of `block_indices`, as integers.
fn block_layer(root: &Compound) -> Vec<i32> {
    layer(root, 0)
}

/// Layer `n` of `block_indices`, as integers.
fn layer(root: &Compound, n: usize) -> Vec<i32> {
    let layers = match structure_compound(root).entries.get("block_indices") {
        Some(Tag::List(layers)) => layers,
        other => panic!("block_indices is not a List: {other:?}"),
    };
    let layer = match layers.items.get(n) {
        Some(Tag::List(layer)) => layer,
        other => panic!("layer {n} is not a List: {other:?}"),
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

/// The `block_palette` entry a layer-0 value names, refusing a slot past the list.
fn written_entry(entries: &[Tag], slot: i32) -> &Tag {
    match usize::try_from(slot)
        .ok()
        .and_then(|slot| entries.get(slot))
    {
        Some(entry) => entry,
        None => panic!(
            "a voxel is written as slot {slot}, past the {}-entry block_palette",
            entries.len()
        ),
    }
}

/// A `block_palette` entry's `name`.
fn name_of(entry: &Tag) -> &str {
    match entry {
        Tag::Compound(entry) => match entry.entries.get("name") {
            Some(Tag::String(name)) => name,
            other => panic!("name is not a String: {other:?}"),
        },
        other => panic!("block_palette entry is not a Compound: {other:?}"),
    }
}

/// A `block_palette` entry's `states`.
fn states_of(entry: &Tag) -> &Compound {
    match entry {
        Tag::Compound(entry) => match entry.entries.get("states") {
            Some(Tag::Compound(states)) => states,
            other => panic!("states is not a Compound: {other:?}"),
        },
        other => panic!("block_palette entry is not a Compound: {other:?}"),
    }
}

#[test]
fn stairs_that_differ_only_in_a_dropped_shape_share_one_palette_entry() {
    let oak = "minecraft:oak_stairs";
    let ba = row_of(&[
        stair(oak, "north", "outer_left"),
        stair(oak, "north", "straight"),
        stair(oak, "south", "outer_right"),
        stair(oak, "north", "outer_right"),
        stair(oak, "south", "outer_left"),
    ]);
    let (root, notes) = build(&ba).expect("build");

    let entries = block_palette(&root);
    for (i, a) in entries.iter().enumerate() {
        for b in &entries[i + 1..] {
            assert_ne!(a, b, "block_palette holds one block twice: {entries:?}");
        }
    }
    // Air, one north stair, one south stair.
    assert_eq!(entries.len(), 3, "{entries:?}");
    assert_eq!(name_of(&entries[0]), "minecraft:air");

    let layer = block_layer(&root);
    assert_eq!(layer[0], 0, "air stays at slot 0");
    // `weirdo_direction` 3 is north and 2 is south.
    let expected = [3, 3, 2, 3, 2];
    for (z, want) in expected.into_iter().enumerate() {
        let entry = written_entry(entries, layer[z + 1]);
        assert_eq!(
            states_of(entry).entries.get("weirdo_direction"),
            Some(&Tag::Int(want)),
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

    // Four corner shapes degrade, one note per Java state; `straight` drops losslessly.
    assert_eq!(
        note_fields(&notes),
        [
            (
                oak,
                "facing=north,half=bottom,shape=outer_left",
                &dropped_shape("outer_left", &["1.21.60"]),
            ),
            (
                oak,
                "facing=south,half=bottom,shape=outer_right",
                &dropped_shape("outer_right", &["1.21.60"]),
            ),
            (
                oak,
                "facing=north,half=bottom,shape=outer_right",
                &dropped_shape("outer_right", &["1.21.60"]),
            ),
            (
                oak,
                "facing=south,half=bottom,shape=outer_left",
                &dropped_shape("outer_left", &["1.21.60"]),
            ),
        ],
    );

    let mut tree = Vec::new();
    write_mcstructure(&mut tree, &root).expect("write tree");
    assert_eq!(streamed_at(&ba, &target_1_21_60()), tree);
}

/// Two ids whose states translate equal stay two blocks, kept apart by `name`.
#[test]
fn stairs_of_two_ids_with_equal_states_keep_an_entry_each() {
    let ids = ["minecraft:oak_stairs", "minecraft:spruce_stairs"];
    let ba = row_of(&ids.map(|id| stair(id, "north", "straight")));
    let (root, _) = build(&ba).expect("build");

    let entries = block_palette(&root);
    let names: Vec<&str> = entries.iter().map(name_of).collect();
    assert_eq!(names, ["minecraft:air", ids[0], ids[1]], "one entry per id");
    assert_eq!(
        states_of(&entries[1]),
        states_of(&entries[2]),
        "premise: both translate to the same states: {entries:?}"
    );
    let layer = block_layer(&root);
    for (voxel, id) in ids.into_iter().enumerate() {
        assert_eq!(
            name_of(written_entry(entries, layer[voxel + 1])),
            id,
            "voxel {}",
            voxel + 1
        );
    }
}

const OAK_STAIRS: &str = "minecraft:oak_stairs";

/// Air, a north oak stair, the same stair `outer_left`, which shares its slot, and bare blocks of
/// distinct ids until `block_palette` holds `PALETTE_CAPACITY` blocks, then `rest` from entry
/// `PALETTE_CAPACITY + 1`, over voxels naming entries `0`, `2` and `u16::MAX`.
fn capacity_blocks_then<const N: usize>(rest: [BlockState; N]) -> BlockArray {
    let mut entries = vec![
        BlockState::air(),
        stair(OAK_STAIRS, "north", "straight"),
        stair(OAK_STAIRS, "north", "outer_left"),
    ];
    entries.extend((3..=PALETTE_CAPACITY).map(|i| BlockState::bare(format!("test:block_{i}"))));
    entries.extend(rest);
    BlockArray {
        dims: Dims { x: 1, y: 1, z: 3 },
        palette: Palette { entries },
        voxels: vec![PaletteIndex::AIR, PaletteIndex(2), PaletteIndex(u16::MAX)],
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::capacity_blocks".to_owned(),
    }
}

/// The names of the `PALETTE_CAPACITY` blocks `capacity_blocks_then` writes before its `rest`.
fn capacity_names(ba: &BlockArray) -> Vec<&str> {
    let bare = ba.palette.entries[3..=PALETTE_CAPACITY].iter();
    ["minecraft:air", OAK_STAIRS]
        .into_iter()
        .chain(bare.map(|entry| entry.id.as_str()))
        .collect()
}

/// Asserts `actual` equals `expected` by length, then by the first position they differ at.
fn assert_same_list<T: PartialEq + std::fmt::Debug>(actual: &[T], expected: &[T], what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: length");
    let first_difference = actual
        .iter()
        .zip(expected)
        .enumerate()
        .find(|(_, (actual, expected))| actual != expected);
    assert_eq!(
        first_difference, None,
        "{what}: first position that differs"
    );
}

#[test]
fn a_new_block_past_the_palette_capacity_takes_slot_65536_and_a_later_entry_shares_it() {
    let south = stair(OAK_STAIRS, "south", "straight");
    let ba = capacity_blocks_then([south.clone(), stair(OAK_STAIRS, "south", "outer_left")]);
    let target = target_1_21_60();
    let (root, notes) = build_at(&ba, &target).expect("build");

    let entries = block_palette(&root);
    let names: Vec<&str> = entries.iter().map(name_of).collect();
    let mut expected = capacity_names(&ba);
    expected.push(OAK_STAIRS);
    assert_same_list(&names, &expected, "block_palette names");
    let (short, _) = build_at(&row_of(&[south]), &target).expect("build short");
    assert_eq!(entries[65_536], block_palette(&short)[1]);
    assert_eq!(block_layer(&root), [0, 1, 65_534]);
    assert_eq!(
        note_fields(&notes),
        [
            (
                OAK_STAIRS,
                "facing=north,half=bottom,shape=outer_left",
                &dropped_shape("outer_left", &["1.21.60"]),
            ),
            (
                OAK_STAIRS,
                "facing=south,half=bottom,shape=outer_left",
                &dropped_shape("outer_left", &["1.21.60"]),
            ),
        ],
    );

    let mut tree = Vec::new();
    write_mcstructure(&mut tree, &root).expect("write tree");
    assert_same_list(&streamed_at(&ba, &target), &tree, "streamed bytes");
}

#[test]
fn an_entry_past_the_palette_capacity_is_accepted_when_it_merges_into_an_earlier_block() {
    let ba = capacity_blocks_then([stair(OAK_STAIRS, "north", "outer_right")]);
    let (root, notes) = build(&ba).expect("build");

    let names: Vec<&str> = block_palette(&root).iter().map(name_of).collect();
    assert_same_list(&names, &capacity_names(&ba), "block_palette names");
    assert_eq!(block_layer(&root), [0, 1, 65_534]);
    assert_eq!(
        note_fields(&notes),
        [
            (
                OAK_STAIRS,
                "facing=north,half=bottom,shape=outer_left",
                &dropped_shape("outer_left", &["1.21.60"]),
            ),
            (
                OAK_STAIRS,
                "facing=north,half=bottom,shape=outer_right",
                &dropped_shape("outer_right", &["1.21.60"]),
            ),
        ],
    );
}

#[test]
#[should_panic(expected = "the state map must be the target version's")]
fn a_state_map_of_another_version_is_a_caller_bug() {
    let (ba, _) = asymmetric_array();
    let latest = resolve_bedrock_target("1.26.60").expect("known target");
    let _ = build_mcstructure_tag(&ba, &target_1_21_60(), &states_for(&latest));
}
