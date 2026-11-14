//! The `.schem` writer: the files an IR splits into and the tag tree each holds.

use cairn_lang_core::block_array::{BlockArray, BlockEntity, BlockState, Dims};
use cairn_lang_core::{Edition, WalkwayScopeKey};
use cairn_lang_formats::data_version::{JavaTarget, resolve_java_target};
use cairn_lang_formats::java_structure::JavaStructureError;
use cairn_lang_formats::schem::{SchemError, SchemFile, prepare_schem, schem_files};
use cairn_lang_nbt::read_java_gzip;
use cairn_lang_nbt::tag::{Compound, List, Tag};

use crate::common::{array_of, comparator_array, ir_of, lower_example, wide_array};

fn target(version: &str) -> JavaTarget {
    resolve_java_target(version).expect("a targetable version")
}

/// The file `block_array` alone writes to, under its own scope at the origin.
fn struct_file(block_array: &BlockArray) -> SchemFile<'_> {
    SchemFile {
        scope: &block_array.source_scope,
        stem: "probe".to_owned(),
        offset: (0, 0, 0),
        block_array,
    }
}

/// The `Schematic` compound `file` writes for `target`, read back as a tree.
fn written_schematic(file: SchemFile<'_>, target: &JavaTarget) -> Compound {
    let mut bytes = Vec::new();
    prepare_schem(file, target)
        .expect("the file prepares")
        .write_gzip(&mut bytes)
        .expect("the file writes");
    let (name, mut root) = read_java_gzip(bytes.as_slice()).expect("the bytes read back");
    assert_eq!(name, "", "the root is unnamed");
    let keys: Vec<&str> = root.entries.keys().map(String::as_str).collect();
    assert_eq!(keys, ["Schematic"], "the root holds only the schematic");
    match root.entries.shift_remove("Schematic") {
        Some(Tag::Compound(schematic)) => schematic,
        other => panic!("`Schematic` is not a compound: {other:?}"),
    }
}

fn entry<'c>(compound: &'c Compound, name: &str) -> &'c Compound {
    match compound.entries.get(name) {
        Some(Tag::Compound(inner)) => inner,
        other => panic!("`{name}` is not a compound: {other:?}"),
    }
}

fn compound<const N: usize>(entries: [(&str, Tag); N]) -> Compound {
    let mut compound = Compound::new();
    for (name, tag) in entries {
        compound.insert(name, tag);
    }
    compound
}

fn string(value: &str) -> Tag {
    Tag::String(value.to_owned())
}

/// The `Palette` keys `file` writes, in index order.
fn palette_keys(file: SchemFile<'_>) -> Vec<(String, Tag)> {
    let schematic = written_schematic(file, &target("1.21.4"));
    let palette = entry(entry(&schematic, "Blocks"), "Palette");
    palette
        .entries
        .iter()
        .map(|(key, index)| (key.clone(), index.clone()))
        .collect()
}

#[test]
fn a_struct_is_a_version_3_schematic_at_the_origin() {
    let ir = ir_of(comparator_array("minecraft:comparator"));
    let mut files = schem_files(&ir);
    assert_eq!(files.len(), 1, "one file");
    let file = files.remove(0);
    assert_eq!(
        (file.scope, file.stem.as_str(), file.offset),
        ("struct::comparator", "comparator", (0, 0, 0))
    );
    let schematic = written_schematic(file, &target("1.21.4"));

    let palette = compound([
        ("minecraft:air", Tag::Int(0)),
        ("minecraft:stone", Tag::Int(1)),
        (
            "minecraft:comparator[facing=east,mode=compare,powered=true]",
            Tag::Int(2),
        ),
    ]);
    // (y, z, x) order: six stone, then the comparator at voxel 9.
    let data = vec![1, 1, 1, 1, 1, 1, 0, 0, 0, 2, 0, 0];
    let block_entity = compound([
        ("Pos", Tag::IntArray(vec![1, 1, 1])),
        ("Id", string("minecraft:comparator")),
        (
            "Data",
            Tag::Compound(compound([("OutputSignal", Tag::Int(3))])),
        ),
    ]);
    let blocks = compound([
        ("Palette", Tag::Compound(palette)),
        ("Data", Tag::ByteArray(data)),
        (
            "BlockEntities",
            Tag::List(List::of_compounds(vec![block_entity])),
        ),
    ]);
    let expected = compound([
        ("Version", Tag::Int(3)),
        ("DataVersion", Tag::Int(4189)),
        (
            "Metadata",
            Tag::Compound(compound([
                ("Name", string("comparator")),
                ("Author", string("Cairn")),
            ])),
        ),
        ("Width", Tag::Short(2)),
        ("Height", Tag::Short(2)),
        ("Length", Tag::Short(3)),
        ("Offset", Tag::IntArray(vec![0, 0, 0])),
        ("Blocks", Tag::Compound(blocks)),
    ]);
    assert_eq!(schematic, expected);
    let keys: Vec<&str> = schematic.entries.keys().map(String::as_str).collect();
    let expected_keys: Vec<&str> = expected.entries.keys().map(String::as_str).collect();
    assert_eq!(keys, expected_keys, "the specification's order");
}

#[test]
fn each_placement_and_walkway_is_a_file_offset_by_its_origin_in_the_site() {
    let ir = lower_example("village.crn", Edition::Java, None);
    let shape: Vec<(&str, String, (i32, i32, i32))> = schem_files(&ir)
        .into_iter()
        .map(|file| (file.scope, file.stem, file.offset))
        .collect();
    let origin = |scope: &str| ir.placements[scope].origin;
    let walkway_scopes: Vec<&str> = ir.walkways.keys().map(WalkwayScopeKey::as_str).collect();
    let walkway = |index: usize| ir.walkways[walkway_scopes[index]].origin;
    let expected: Vec<(&str, String, (i32, i32, i32))> = vec![
        (
            "site::hamlet::home1",
            "home1".to_owned(),
            origin("site::hamlet::home1"),
        ),
        (
            "site::hamlet::home2",
            "home2".to_owned(),
            origin("site::hamlet::home2"),
        ),
        (
            "site::hamlet::home3",
            "home3".to_owned(),
            origin("site::hamlet::home3"),
        ),
        (
            walkway_scopes[0],
            "hamlet_walkway_home1_entry__home2_entry".to_owned(),
            walkway(0),
        ),
        (
            walkway_scopes[1],
            "hamlet_walkway_home1_entry__home3_entry".to_owned(),
            walkway(1),
        ),
    ];
    assert_eq!(shape, expected);
    let scopes: Vec<&str> = ir.structures.keys().map(String::as_str).collect();
    let file_scopes: Vec<&str> = expected.iter().map(|(scope, _, _)| *scope).collect();
    assert_eq!(
        scopes, file_scopes,
        "one file per structure, in the IR's order"
    );
}

#[test]
fn the_offset_is_written_as_the_files_offset() {
    let array = array_of("probe", Dims { x: 1, y: 1, z: 1 }, &[], &[0]);
    let mut file = struct_file(&array);
    file.offset = (-4, 7, i32::MAX);
    let schematic = written_schematic(file, &target("1.21.4"));
    assert_eq!(
        schematic.entries["Offset"],
        Tag::IntArray(vec![-4, 7, i32::MAX])
    );
}

#[test]
fn the_data_version_is_the_targets() {
    let array = array_of("probe", Dims { x: 1, y: 1, z: 1 }, &[], &[0]);
    let versions: Vec<(&str, Tag)> = ["1.20.4", "1.21", "1.21.4", "26.3"]
        .into_iter()
        .map(|version| {
            let schematic = written_schematic(struct_file(&array), &target(version));
            (version, schematic.entries["DataVersion"].clone())
        })
        .collect();
    assert_eq!(
        versions,
        [
            ("1.20.4", Tag::Int(3700)),
            ("1.21", Tag::Int(3953)),
            ("1.21.4", Tag::Int(4189)),
            ("26.3", Tag::Int(5023)),
        ]
    );
}

#[test]
fn an_index_takes_one_varint_byte_for_each_seven_bits() {
    let array = wide_array(16385, &[0, 127, 128, 16383, 16384]);
    let schematic = written_schematic(struct_file(&array), &target("1.21.4"));
    let data = &entry(&schematic, "Blocks").entries["Data"];
    let bytes: [u8; 9] = [0x00, 0x7f, 0x80, 0x01, 0xff, 0x7f, 0x80, 0x80, 0x01];
    assert_eq!(
        *data,
        Tag::ByteArray(bytes.iter().map(|byte| byte.cast_signed()).collect())
    );
}

#[test]
fn a_states_properties_are_spelled_sorted_by_name() {
    let mut array = array_of("probe", Dims { x: 1, y: 1, z: 1 }, &[], &[0]);
    let mut state = BlockState::bare("minecraft:lever");
    for (name, value) in [("powered", "true"), ("face", "wall"), ("facing", "north")] {
        state.properties.insert(name.to_owned(), value.to_owned());
    }
    array.palette.intern(state);
    assert_eq!(
        palette_keys(struct_file(&array)),
        [
            ("minecraft:air".to_owned(), Tag::Int(0)),
            (
                "minecraft:lever[face=wall,facing=north,powered=true]".to_owned(),
                Tag::Int(1),
            ),
        ]
    );
}

#[test]
fn only_the_last_block_entity_at_each_voxel_of_the_grid_is_written() {
    let mut array = comparator_array("minecraft:comparator");
    let base = array.block_entities[0].clone();
    let at = |pos, id: &str| BlockEntity {
        pos,
        id: id.to_owned(),
        ..base.clone()
    };
    array.block_entities = vec![
        at((1, 1, 1), "minecraft:first"),
        at((0, 0, 3), "minecraft:off_the_grid"),
        at((u32::MAX, 0, 0), "minecraft:far_off_the_grid"),
        at((1, 1, 1), "minecraft:last"),
        at((0, 0, 0), "minecraft:origin"),
    ];
    let schematic = written_schematic(struct_file(&array), &target("1.21.4"));
    let Some(Tag::List(block_entities)) = entry(&schematic, "Blocks").entries.get("BlockEntities")
    else {
        panic!("BlockEntities is a list");
    };
    let written: Vec<(&Tag, &Tag)> = block_entities
        .items
        .iter()
        .map(|item| {
            let Tag::Compound(nbt) = item else {
                panic!("a block entity is a compound");
            };
            (&nbt.entries["Id"], &nbt.entries["Pos"])
        })
        .collect();
    let (origin, last) = (string("minecraft:origin"), string("minecraft:last"));
    let (at_origin, at_one) = (Tag::IntArray(vec![0, 0, 0]), Tag::IntArray(vec![1, 1, 1]));
    assert_eq!(written, [(&origin, &at_origin), (&last, &at_one)]);
}

#[test]
fn an_extent_past_an_unsigned_short_is_refused() {
    let array = array_of(
        "probe",
        Dims {
            x: 1,
            y: 65536,
            z: 1,
        },
        &[],
        &vec![0; 65536],
    );
    let err = prepare_schem(struct_file(&array), &target("1.21.4")).expect_err("too tall");
    assert!(
        matches!(
            err,
            SchemError::DimensionOverflow {
                axis: "y",
                field: "Height",
                value: 65536
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn a_state_its_spelling_would_misread_is_refused() {
    let mut array = array_of("probe", Dims { x: 1, y: 1, z: 1 }, &[], &[0]);
    let mut state = BlockState::bare("minecraft:sign");
    state.properties.insert("text".to_owned(), "a,b".to_owned());
    array.palette.intern(state);
    let err = prepare_schem(struct_file(&array), &target("1.21.4")).expect_err("a comma");
    assert!(
        matches!(
            &err,
            SchemError::UnspellableState { state } if state == "minecraft:sign[text=a,b]"
        ),
        "got {err:?}"
    );
}

#[test]
fn two_palette_entries_spelled_alike_are_refused() {
    let mut array = array_of(
        "probe",
        Dims { x: 1, y: 1, z: 1 },
        &["minecraft:stone"],
        &[0],
    );
    array
        .palette
        .entries
        .push(BlockState::bare("minecraft:stone"));
    let err = prepare_schem(struct_file(&array), &target("1.21.4")).expect_err("twice");
    assert!(
        matches!(
            &err,
            SchemError::DuplicateState { state, first: 1, second: 2 } if state == "minecraft:stone"
        ),
        "got {err:?}"
    );
}

#[test]
fn an_array_the_nbt_writer_would_refuse_is_refused() {
    let array = array_of("probe", Dims { x: 1, y: 1, z: 1 }, &["oak_planks"], &[1]);
    let err = prepare_schem(struct_file(&array), &target("1.21.4")).expect_err("abstract");
    assert!(
        matches!(
            &err,
            SchemError::Structure(JavaStructureError::AbstractPaletteEntry { id })
                if id == "oak_planks"
        ),
        "got {err:?}"
    );
}

#[test]
fn a_name_the_nbt_encoder_cannot_write_is_refused() {
    let array = array_of("probe", Dims { x: 1, y: 1, z: 1 }, &[], &[0]);
    let mut file = struct_file(&array);
    file.stem = "h\u{f6}me".to_owned();
    let err = prepare_schem(file, &target("1.21.4")).expect_err("not ASCII");
    assert!(
        matches!(&err, SchemError::UnencodableName { name, .. } if name == "h\u{f6}me"),
        "got {err:?}"
    );
}
