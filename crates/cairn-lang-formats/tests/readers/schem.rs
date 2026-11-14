//! `read_schem` over what the writer wrote, and over files only another tool would write.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArrayIr, BlockState, Dims, PaletteIndex};
use cairn_lang_formats::data_version::{JavaTarget, resolve_java_target};
use cairn_lang_formats::schem::{
    SchemReadError, SpongeSchematic, prepare_schem, read_schem, schem_files,
};
use cairn_lang_formats::volume::{BlockVolume, StoredBlockEntity};
use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{NbtReadError, write_java_gzip};

use crate::common::{comparator_array, ir_of, latest, lower_example, volume_of, wide_array};

fn target(version: &str) -> JavaTarget {
    resolve_java_target(version).expect("a targetable version")
}

/// Every file `ir` writes to for `target`, read back.
fn round_trip(ir: &BlockArrayIr, target: &JavaTarget) -> Vec<SpongeSchematic> {
    schem_files(ir)
        .into_iter()
        .map(|file| {
            let mut bytes = Vec::new();
            prepare_schem(file, target)
                .expect("the file prepares")
                .write_gzip(&mut bytes)
                .expect("the file writes");
            read_schem(bytes.as_slice()).expect("the file reads back")
        })
        .collect()
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

#[test]
fn a_struct_reads_back_as_its_block_array_with_its_block_entity() {
    let array = comparator_array("minecraft:comparator");
    let read = round_trip(&ir_of(array.clone()), &target("1.20.4"));
    let expected = SpongeSchematic {
        version: 3,
        data_version: 3700,
        metadata: compound([("Name", string("comparator")), ("Author", string("Cairn"))]),
        offset: (0, 0, 0),
        volume: volume_of(&array),
        entities: vec![],
    };
    assert_eq!(read, vec![expected]);
}

#[test]
fn each_placement_and_walkway_reads_back_at_its_offset() {
    let ir = lower_example("village.crn", Edition::Java, None);
    let read: Vec<((i32, i32, i32), BlockVolume)> = round_trip(&ir, &target(latest(Edition::Java)))
        .into_iter()
        .map(|schematic| (schematic.offset, schematic.volume))
        .collect();
    let expected: Vec<((i32, i32, i32), BlockVolume)> = ir
        .structures
        .iter()
        .map(|(scope, array)| {
            let origin = ir
                .placements
                .get(scope)
                .map(|p| p.origin)
                .or_else(|| ir.walkways.get(scope.as_str()).map(|w| w.origin))
                .expect("every village structure is placed");
            (origin, volume_of(array))
        })
        .collect();
    assert_eq!(read, expected);
}

#[test]
fn every_shipped_example_reads_back_as_its_block_arrays() {
    let target = target(latest(Edition::Java));
    for (name, _) in crate::common::examples() {
        let ir = lower_example(&name, Edition::Java, None);
        let read: Vec<BlockVolume> = round_trip(&ir, &target)
            .into_iter()
            .map(|schematic| schematic.volume)
            .collect();
        let expected: Vec<BlockVolume> = ir.structures.values().map(volume_of).collect();
        assert_eq!(read, expected, "{name}");
    }
}

#[test]
fn an_index_of_three_varint_bytes_reads_back() {
    let array = wide_array(16385, &[16384, 0, 127, 128, 16383, 16384]);
    let read = round_trip(&ir_of(array.clone()), &target("1.21.4"));
    assert_eq!(read[0].volume, volume_of(&array));
}

/// A version 3 `Schematic` of `extents`, its `Blocks` holding `palette` and `data`.
fn schematic_v3(extents: [i16; 3], palette: &[(&str, i32)], data: &[u8]) -> Compound {
    let mut stored = Compound::new();
    for &(spelled, index) in palette {
        stored.insert(spelled, Tag::Int(index));
    }
    let blocks = compound([
        ("Palette", Tag::Compound(stored)),
        (
            "Data",
            Tag::ByteArray(data.iter().map(|byte| byte.cast_signed()).collect()),
        ),
    ]);
    compound([
        ("Version", Tag::Int(3)),
        ("DataVersion", Tag::Int(4189)),
        ("Width", Tag::Short(extents[0])),
        ("Height", Tag::Short(extents[1])),
        ("Length", Tag::Short(extents[2])),
        ("Blocks", Tag::Compound(blocks)),
    ])
}

/// `schematic` gzip-written as version 3 nests it, under an unnamed root.
fn v3_bytes(schematic: Compound) -> Vec<u8> {
    let root = compound([("Schematic", Tag::Compound(schematic))]);
    let mut bytes = Vec::new();
    write_java_gzip(&mut bytes, "", &root).expect("the root writes");
    bytes
}

fn read_error(bytes: &[u8]) -> SchemReadError {
    read_schem(bytes).expect_err("the file is refused")
}

fn blocks_mut(schematic: &mut Compound) -> &mut Compound {
    match schematic.entries.get_mut("Blocks") {
        Some(Tag::Compound(blocks)) => blocks,
        other => panic!("`Blocks` is not a compound: {other:?}"),
    }
}

#[test]
fn a_version_2_file_reads_with_its_fields_beside_each_block_entitys_id() {
    let mut palette = Compound::new();
    for (spelled, index) in [
        ("minecraft:air", 0),
        ("stone", 1),
        ("minecraft:lever[face=wall,powered=false]", 2),
    ] {
        palette.insert(spelled, Tag::Int(index));
    }
    let sign = compound([
        ("Pos", Tag::IntArray(vec![2, 0, 0])),
        ("Id", string("minecraft:sign")),
        ("Text1", string("hi")),
    ]);
    let creeper = compound([
        (
            "Pos",
            Tag::List(List::of_tags(
                6,
                vec![Tag::Double(0.5), Tag::Double(0.0), Tag::Double(0.5)],
            )),
        ),
        ("Id", string("minecraft:creeper")),
    ]);
    let root = compound([
        ("Version", Tag::Int(2)),
        ("DataVersion", Tag::Int(3700)),
        (
            "Metadata",
            Tag::Compound(compound([("Name", string("v2"))])),
        ),
        ("Width", Tag::Short(3)),
        ("Height", Tag::Short(1)),
        ("Length", Tag::Short(1)),
        ("Offset", Tag::IntArray(vec![-1, 64, 2])),
        ("PaletteMax", Tag::Int(3)),
        ("Palette", Tag::Compound(palette)),
        ("BlockData", Tag::ByteArray(vec![1, 0, 2])),
        ("BlockEntities", Tag::List(List::of_compounds(vec![sign]))),
        (
            "Entities",
            Tag::List(List::of_compounds(vec![creeper.clone()])),
        ),
    ]);
    let mut bytes = Vec::new();
    write_java_gzip(&mut bytes, "Schematic", &root).expect("the root writes");

    let mut lever = BlockState::bare("minecraft:lever");
    for (name, value) in [("face", "wall"), ("powered", "false")] {
        lever.properties.insert(name.to_owned(), value.to_owned());
    }
    let expected = SpongeSchematic {
        version: 2,
        data_version: 3700,
        metadata: compound([("Name", string("v2"))]),
        offset: (-1, 64, 2),
        volume: BlockVolume {
            dims: Dims { x: 3, y: 1, z: 1 },
            palette: vec![
                BlockState::air(),
                BlockState::bare("minecraft:stone"),
                lever,
            ],
            voxels: vec![PaletteIndex(1), PaletteIndex(0), PaletteIndex(2)],
            block_entities: vec![StoredBlockEntity {
                pos: (2, 0, 0),
                id: "minecraft:sign".to_owned(),
                data: compound([("Text1", string("hi"))]),
            }],
        },
        entities: vec![creeper],
    };
    assert_eq!(
        read_schem(bytes.as_slice()).expect("the file reads"),
        expected
    );
}

#[test]
fn a_sparse_palette_reads_back_in_index_order() {
    let schematic = schematic_v3(
        [3, 1, 1],
        &[("minecraft:stone", 9), ("minecraft:air", 4)],
        &[9, 4, 9],
    );
    let read = read_schem(v3_bytes(schematic).as_slice()).expect("the file reads");
    assert_eq!(
        read.volume,
        BlockVolume {
            dims: Dims { x: 3, y: 1, z: 1 },
            palette: vec![BlockState::air(), BlockState::bare("minecraft:stone")],
            voxels: vec![PaletteIndex(1), PaletteIndex(0), PaletteIndex(1)],
            block_entities: vec![],
        }
    );
    assert_eq!((read.metadata, read.offset), (Compound::new(), (0, 0, 0)));
}

#[test]
fn a_version_without_a_data_version_is_refused() {
    let mut schematic = schematic_v3([1, 1, 1], &[("minecraft:air", 0)], &[0]);
    schematic.insert("Version", Tag::Int(1));
    let err = read_error(&v3_bytes(schematic));
    assert!(
        matches!(err, SchemReadError::UnsupportedVersion { version: 1 }),
        "got {err:?}"
    );
}

#[test]
fn data_holding_other_than_one_varint_a_voxel_is_refused() {
    let schematic = schematic_v3([2, 1, 1], &[("minecraft:air", 0)], &[0, 0x80, 0x01, 0]);
    let err = read_error(&v3_bytes(schematic));
    assert!(
        matches!(
            err,
            SchemReadError::DataLength {
                path: "Schematic.Blocks.Data",
                found: 3,
                needed: 2
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn an_index_no_palette_entry_maps_is_refused_at_its_voxel() {
    let schematic = schematic_v3([2, 1, 1], &[("minecraft:air", 0)], &[0, 0x85, 0x01]);
    let err = read_error(&v3_bytes(schematic));
    assert!(
        matches!(
            err,
            SchemReadError::UnknownPaletteIndex {
                voxel: 1,
                index: 133
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn extents_past_the_block_array_budget_are_refused_before_the_data_is_decoded() {
    let schematic = schematic_v3([257, 256, 256], &[("minecraft:air", 0)], &[]);
    let err = read_error(&v3_bytes(schematic));
    assert!(
        matches!(
            err,
            SchemReadError::TooLarge { volume, limit }
                if volume == 257 * 256 * 256 && limit == 256 * 256 * 256
        ),
        "got {err:?}"
    );
}

#[test]
fn a_palette_past_a_block_arrays_is_refused() {
    let spelled: Vec<String> = (0..=65536).map(|i| format!("minecraft:b{i}")).collect();
    let palette: Vec<(&str, i32)> = spelled
        .iter()
        .zip(0..)
        .map(|(spelled, index)| (spelled.as_str(), index))
        .collect();
    let err = read_error(&v3_bytes(schematic_v3([1, 1, 1], &palette, &[0])));
    assert!(
        matches!(
            err,
            SchemReadError::PaletteTooLarge {
                len: 65537,
                limit: 65536
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn a_field_of_the_wrong_shape_is_refused_by_its_path() {
    let base = || schematic_v3([1, 1, 1], &[("minecraft:air", 0)], &[0]);
    let mut no_width = base();
    no_width.entries.shift_remove("Width");
    let mut short_offset = base();
    short_offset.insert("Offset", Tag::IntArray(vec![0, 0]));
    let mut no_palette = base();
    blocks_mut(&mut no_palette).entries.shift_remove("Palette");
    let unparsed = schematic_v3([1, 1, 1], &[("minecraft:lever[face]", 0)], &[0]);
    let twice = schematic_v3(
        [1, 1, 1],
        &[("minecraft:air", 0), ("minecraft:stone", 0)],
        &[0],
    );
    let mut not_int = base();
    blocks_mut(&mut not_int).insert(
        "Palette",
        Tag::Compound(compound([("minecraft:air", string("0"))])),
    );
    let mut no_data = base();
    blocks_mut(&mut no_data).entries.shift_remove("Data");
    let cut_short = schematic_v3([1, 1, 1], &[("minecraft:air", 0)], &[0x80]);
    let mut outside = base();
    let chest = compound([
        ("Pos", Tag::IntArray(vec![1, 0, 0])),
        ("Id", string("minecraft:chest")),
    ]);
    blocks_mut(&mut outside).insert("BlockEntities", Tag::List(List::of_compounds(vec![chest])));
    let refused: Vec<(String, &str)> = [
        no_width,
        short_offset,
        no_palette,
        unparsed,
        twice,
        not_int,
        no_data,
        cut_short,
        outside,
    ]
    .into_iter()
    .map(|schematic| match read_error(&v3_bytes(schematic)) {
        SchemReadError::Malformed { path, expected } => (path, expected),
        other => panic!("not malformed: {other:?}"),
    })
    .collect();
    assert_eq!(
        refused,
        [
            ("Schematic.Width".to_owned(), "a short"),
            ("Schematic.Offset".to_owned(), "an int array of three"),
            (
                "Schematic.Blocks.Palette".to_owned(),
                "a compound of block states to indices"
            ),
            (
                "Schematic.Blocks.Palette.minecraft:lever[face]".to_owned(),
                "keyed by a block state, `namespace:id[name=value,...]`"
            ),
            (
                "Schematic.Blocks.Palette".to_owned(),
                "a compound that maps each index once"
            ),
            (
                "Schematic.Blocks.Palette.minecraft:air".to_owned(),
                "an int index"
            ),
            ("Schematic.Blocks.Data".to_owned(), "a byte array"),
            (
                "Schematic.Blocks.Data".to_owned(),
                "an array of whole varints"
            ),
            (
                "Schematic.Blocks.BlockEntities".to_owned(),
                "a list of block entities, each an `Id` and a `Pos` inside the schematic"
            ),
        ]
    );
}

#[test]
fn bytes_that_are_not_gzip_are_an_nbt_error() {
    let err = read_error(b"not a schematic");
    assert!(
        matches!(err, SchemReadError::Nbt(NbtReadError::Io(_))),
        "got {err:?}"
    );
}
