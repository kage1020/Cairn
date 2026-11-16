//! `read_schem` over what the writer wrote, and over files only another tool would write.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{
    BlockArrayIr, BlockState, Dims, MAX_STRUCTURE_VOLUME, PaletteIndex,
};
use cairn_lang_formats::data_version::{JavaTarget, resolve_java_target};
use cairn_lang_formats::schem::{
    SchemReadError, SpongeSchematic, prepare_schem, read_schem, schem_files,
};
use cairn_lang_formats::volume::{BlockVolume, StoredBlockEntity};
use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{
    CompoundStream, NbtIoError, NbtReadError, stream_java_uncompressed, write_java_gzip,
    write_java_uncompressed,
};

use crate::common::{comparator_array, gzip, ir_of, latest, lower_example, volume_of, wide_array};

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

/// The path and expectation each file is refused with.
fn malformed_files(files: impl IntoIterator<Item = Vec<u8>>) -> Vec<(String, &'static str)> {
    files
        .into_iter()
        .map(|bytes| match read_error(&bytes) {
            SchemReadError::Malformed { path, expected } => (path, expected),
            other => panic!("not malformed: {other:?}"),
        })
        .collect()
}

/// The path and expectation each version 3 schematic is refused with.
fn malformed(schematics: impl IntoIterator<Item = Compound>) -> Vec<(String, &'static str)> {
    malformed_files(schematics.into_iter().map(v3_bytes))
}

/// A version 2 root of `extents`, holding `palette` and `data` beside its other fields.
fn root_v2(extents: [i16; 3], palette: &[(&str, i32)], data: &[u8]) -> Compound {
    let mut schematic = schematic_v3(extents, palette, data);
    let Some(Tag::Compound(mut blocks)) = schematic.entries.shift_remove("Blocks") else {
        panic!("a version 3 schematic holds `Blocks`");
    };
    schematic.insert("Version", Tag::Int(2));
    for (from, to) in [("Palette", "Palette"), ("Data", "BlockData")] {
        let tag = blocks
            .entries
            .shift_remove(from)
            .expect("the field is built");
        schematic.insert(to, tag);
    }
    schematic
}

/// `root` gzip-written as version 2 keeps it, under a root named `Schematic`.
fn v2_bytes(root: &Compound) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_java_gzip(&mut bytes, "Schematic", root).expect("the root writes");
    bytes
}

/// A one-voxel version 2 root of air.
fn air_v2() -> Compound {
    root_v2([1, 1, 1], &[("minecraft:air", 0)], &[0])
}

/// The unnamed root `body` streams, gzip-written, which can hold a name twice.
fn streamed(
    body: impl FnOnce(&mut CompoundStream<'_, Vec<u8>>) -> Result<(), NbtIoError>,
) -> Vec<u8> {
    let mut uncompressed = Vec::new();
    stream_java_uncompressed(&mut uncompressed, "", body).expect("the root streams");
    gzip(&uncompressed)
}

/// `schematic`'s entries streamed into `compound`.
fn stream_entries(
    compound: &mut CompoundStream<'_, Vec<u8>>,
    schematic: &Compound,
) -> Result<(), NbtIoError> {
    schematic
        .entries
        .iter()
        .try_for_each(|(name, tag)| compound.tag(name, tag))
}

/// A one-voxel schematic of air, in version 3.
fn air_v3() -> Compound {
    schematic_v3([1, 1, 1], &[("minecraft:air", 0)], &[0])
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
        volume: BlockVolume::new(
            Dims { x: 3, y: 1, z: 1 },
            vec![
                BlockState::air(),
                BlockState::bare("minecraft:stone"),
                lever,
            ],
            vec![PaletteIndex(1), PaletteIndex(0), PaletteIndex(2)],
            vec![StoredBlockEntity {
                pos: (2, 0, 0),
                id: "minecraft:sign".to_owned(),
                data: compound([("Text1", string("hi"))]),
            }],
        )
        .expect("a volume"),
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
        BlockVolume::new(
            Dims { x: 3, y: 1, z: 1 },
            vec![BlockState::air(), BlockState::bare("minecraft:stone")],
            vec![PaletteIndex(1), PaletteIndex(0), PaletteIndex(1)],
            vec![],
        )
        .expect("a volume")
    );
    assert_eq!((read.metadata, read.offset), (Compound::new(), (0, 0, 0)));
}

#[test]
fn a_version_without_a_data_version_is_refused() {
    let mut schematic = air_v3();
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
    let mut no_width = air_v3();
    no_width.entries.shift_remove("Width");
    let mut long_data_version = air_v3();
    long_data_version.insert("DataVersion", Tag::Long(4189));
    let mut short_offset = air_v3();
    short_offset.insert("Offset", Tag::IntArray(vec![0, 0]));
    let mut blocks_of_ints = air_v3();
    blocks_of_ints.insert("Blocks", Tag::Int(0));
    let mut no_palette = air_v3();
    blocks_mut(&mut no_palette).entries.shift_remove("Palette");
    let mut not_int = air_v3();
    blocks_mut(&mut not_int).insert(
        "Palette",
        Tag::Compound(compound([("minecraft:air", string("0"))])),
    );
    let negative = schematic_v3(
        [1, 1, 1],
        &[("minecraft:air", 0), ("minecraft:stone", -1)],
        &[0],
    );
    let mut no_data = air_v3();
    blocks_mut(&mut no_data).entries.shift_remove("Data");
    let cut_short = schematic_v3([1, 1, 1], &[("minecraft:air", 0)], &[0x80]);
    assert_eq!(
        malformed([
            no_width,
            long_data_version,
            short_offset,
            blocks_of_ints,
            no_palette,
            not_int,
            negative,
            no_data,
            cut_short,
        ]),
        [
            ("Schematic.Width".to_owned(), "a short"),
            ("Schematic.DataVersion".to_owned(), "an int"),
            ("Schematic.Offset".to_owned(), "an int array of three"),
            ("Schematic.Blocks".to_owned(), "a compound"),
            (
                "Schematic.Blocks.Palette".to_owned(),
                "a compound of block states to indices"
            ),
            (
                "Schematic.Blocks.Palette.minecraft:air".to_owned(),
                "an int index"
            ),
            (
                "Schematic.Blocks.Palette.minecraft:stone".to_owned(),
                "an int index, 0 or more"
            ),
            ("Schematic.Blocks.Data".to_owned(), "a byte array"),
            (
                "Schematic.Blocks.Data".to_owned(),
                "an array of whole varints"
            ),
        ]
    );
}

#[test]
fn an_optional_field_of_another_type_is_refused_rather_than_read_as_absent() {
    let with = |name: &str, tag: Tag| {
        let mut schematic = air_v3();
        schematic.insert(name, tag);
        schematic
    };
    let offset_list = with(
        "Offset",
        Tag::List(List::of_tags(
            3,
            vec![Tag::Int(1), Tag::Int(2), Tag::Int(3)],
        )),
    );
    let offset_longs = with("Offset", Tag::LongArray(vec![1, 2, 3]));
    let metadata_int = with("Metadata", Tag::Int(0));
    let entities_compound = with("Entities", Tag::Compound(Compound::new()));
    let mut block_entities_compound = air_v3();
    blocks_mut(&mut block_entities_compound)
        .insert("BlockEntities", Tag::Compound(Compound::new()));
    assert_eq!(
        malformed([
            offset_list,
            offset_longs,
            metadata_int,
            entities_compound,
            block_entities_compound,
        ]),
        [
            ("Schematic.Offset".to_owned(), "an int array of three"),
            ("Schematic.Offset".to_owned(), "an int array of three"),
            ("Schematic.Metadata".to_owned(), "a compound"),
            ("Schematic.Entities".to_owned(), "a list of compounds"),
            (
                "Schematic.Blocks.BlockEntities".to_owned(),
                "a list of compounds"
            ),
        ]
    );
}

#[test]
fn a_palette_key_that_spells_no_block_state_is_refused() {
    let keys = [
        "minecraft:lever[face]",
        "minecraft:lever[face=wall",
        "[face=wall]",
        "minecraft:lever[=wall]",
        "minecraft:lever[face=a=b]",
        "minecraft:lever[face=wall,face=floor]",
    ];
    let refused =
        malformed(keys.map(|key| schematic_v3([1, 1, 1], &[("minecraft:air", 0), (key, 1)], &[0])));
    let expected = keys.map(|key| {
        (
            format!("Schematic.Blocks.Palette.{key}"),
            "keyed by a block state, `namespace:id[name=value,...]`",
        )
    });
    assert_eq!(refused, expected);
}

#[test]
fn a_missing_or_mistyped_version_is_refused_where_the_file_would_keep_it() {
    let mut v2_without = air_v2();
    v2_without.entries.shift_remove("Version");
    let mut v3_without = air_v3();
    v3_without.entries.shift_remove("Version");
    let mut short = air_v3();
    short.insert("Version", Tag::Short(3));
    assert_eq!(
        malformed_files([v2_bytes(&v2_without), v3_bytes(v3_without), v3_bytes(short)]),
        [
            ("Version".to_owned(), "an int"),
            ("Schematic.Version".to_owned(), "an int"),
            ("Schematic.Version".to_owned(), "an int"),
        ]
    );
}

#[test]
fn a_version_2_file_is_refused_by_its_version_2_paths() {
    let without = |name: &str| {
        let mut root = air_v2();
        root.entries.shift_remove(name);
        v2_bytes(&root)
    };
    let mut short_offset = air_v2();
    short_offset.insert("Offset", Tag::IntArray(vec![0]));
    let mut no_pos = air_v2();
    no_pos.insert(
        "BlockEntities",
        Tag::List(List::of_compounds(vec![compound([(
            "Id",
            string("minecraft:chest"),
        )])])),
    );
    let mut entities_of_ints = air_v2();
    entities_of_ints.insert("Entities", Tag::List(List::of_tags(3, vec![Tag::Int(0)])));
    assert_eq!(
        malformed_files([
            without("DataVersion"),
            without("Length"),
            v2_bytes(&short_offset),
            without("Palette"),
            without("BlockData"),
            v2_bytes(&no_pos),
            v2_bytes(&entities_of_ints),
        ]),
        [
            ("DataVersion".to_owned(), "an int"),
            ("Length".to_owned(), "a short"),
            ("Offset".to_owned(), "an int array of three"),
            (
                "Palette".to_owned(),
                "a compound of block states to indices"
            ),
            ("BlockData".to_owned(), "a byte array"),
            ("BlockEntities[0].Pos".to_owned(), "an int array of three"),
            ("Entities".to_owned(), "a list of compounds"),
        ]
    );
    let err = read_error(&v2_bytes(&root_v2(
        [2, 1, 1],
        &[("minecraft:air", 0)],
        &[0],
    )));
    assert!(
        matches!(
            err,
            SchemReadError::DataLength {
                path: "BlockData",
                found: 1,
                needed: 2
            }
        ),
        "got {err:?}"
    );
}

/// The path, version and place each file is refused with as a field away from its place.
fn misplaced(files: impl IntoIterator<Item = Vec<u8>>) -> Vec<(String, i32, String)> {
    files
        .into_iter()
        .map(|bytes| match read_error(&bytes) {
            SchemReadError::Misplaced {
                path,
                version,
                belongs,
            } => (path, version, belongs),
            other => panic!("not misplaced: {other:?}"),
        })
        .collect()
}

#[test]
fn a_field_away_from_where_its_version_keeps_it_is_refused() {
    let mut v2_labelled_3 = air_v2();
    v2_labelled_3.insert("Version", Tag::Int(3));
    let v2_labelled_3 = v3_bytes(v2_labelled_3);

    let mut v2_with_blocks = air_v2();
    let palette = v2_with_blocks
        .entries
        .shift_remove("Palette")
        .expect("built");
    let data = v2_with_blocks
        .entries
        .shift_remove("BlockData")
        .expect("built");
    v2_with_blocks.insert(
        "Blocks",
        Tag::Compound(compound([("Palette", palette), ("Data", data)])),
    );
    let v2_with_blocks = v2_bytes(&v2_with_blocks);

    let mut v2_beside_v3 = compound([("Version", Tag::Int(2))]);
    v2_beside_v3.insert("Schematic", Tag::Compound(air_v3()));
    let mut v2_beside_v3_bytes = Vec::new();
    write_java_gzip(&mut v2_beside_v3_bytes, "", &v2_beside_v3).expect("the root writes");

    let mut version_in_root = air_v3();
    version_in_root.entries.shift_remove("Version");
    let mut root = compound([("Version", Tag::Int(3))]);
    root.insert("Schematic", Tag::Compound(version_in_root));
    let mut version_in_root = Vec::new();
    write_java_gzip(&mut version_in_root, "", &root).expect("the root writes");

    let mut block_data_too = air_v3();
    block_data_too.insert("BlockData", Tag::ByteArray(vec![0]));

    let mut blocks_in_root = air_v3();
    let blocks = blocks_in_root
        .entries
        .shift_remove("Blocks")
        .expect("built");
    let mut root = compound([("Blocks", blocks)]);
    root.insert("Schematic", Tag::Compound(blocks_in_root));
    let mut blocks_in_root = Vec::new();
    write_java_gzip(&mut blocks_in_root, "", &root).expect("the root writes");

    assert_eq!(
        misplaced([
            v2_labelled_3,
            v2_with_blocks,
            v2_beside_v3_bytes,
            version_in_root,
            v3_bytes(block_data_too),
            blocks_in_root,
            v3_bytes(air_v2()),
        ]),
        [
            (
                "Schematic.Palette".to_owned(),
                3,
                "Schematic.Blocks.Palette".to_owned()
            ),
            ("Blocks.Palette".to_owned(), 2, "Palette".to_owned()),
            ("Schematic.Version".to_owned(), 2, "Version".to_owned()),
            ("Version".to_owned(), 3, "Schematic.Version".to_owned()),
            (
                "Schematic.BlockData".to_owned(),
                3,
                "Schematic.Blocks.Data".to_owned()
            ),
            ("Blocks".to_owned(), 3, "Schematic.Blocks".to_owned()),
            ("Schematic.Version".to_owned(), 2, "Version".to_owned()),
        ]
    );
}

#[test]
fn a_field_or_palette_key_stored_twice_is_refused_by_its_path() {
    let schematic = air_v3();
    let width_twice = streamed(|root| {
        root.compound("Schematic", |inner| {
            stream_entries(inner, &schematic)?;
            inner.tag("Width", &Tag::Short(1))
        })
    });
    let blocks_twice = streamed(|root| {
        root.compound("Schematic", |inner| {
            stream_entries(inner, &schematic)?;
            inner.compound("Blocks", |_| Ok(()))
        })
    });
    let key_twice = streamed(|root| {
        root.compound("Schematic", |inner| {
            for (name, tag) in &schematic.entries {
                if name != "Blocks" {
                    inner.tag(name, tag)?;
                }
            }
            inner.compound("Blocks", |blocks| {
                blocks.compound("Palette", |palette| {
                    palette.tag("minecraft:air", &Tag::Int(0))?;
                    palette.tag("minecraft:stone", &Tag::Int(1))?;
                    palette.tag("minecraft:stone", &Tag::Int(2))
                })?;
                blocks.tag("Data", &Tag::ByteArray(vec![0]))
            })
        })
    });
    let root = air_v2();
    let block_data_twice = {
        let mut uncompressed = Vec::new();
        stream_java_uncompressed(&mut uncompressed, "Schematic", |inner| {
            stream_entries(inner, &root)?;
            inner.tag("BlockData", &Tag::ByteArray(vec![0]))
        })
        .expect("the root streams");
        gzip(&uncompressed)
    };
    let repeated: Vec<String> = [width_twice, blocks_twice, key_twice, block_data_twice]
        .iter()
        .map(|bytes| match read_error(bytes) {
            SchemReadError::Repeated { path } => path,
            other => panic!("not repeated: {other:?}"),
        })
        .collect();
    assert_eq!(
        repeated,
        [
            "Schematic.Width",
            "Schematic.Blocks",
            "Schematic.Blocks.Palette.minecraft:stone",
            "BlockData",
        ]
    );
}

#[test]
fn a_palette_that_spells_one_state_twice_or_shares_an_index_is_refused() {
    let unprefixed = schematic_v3([1, 1, 1], &[("stone", 0), ("minecraft:stone", 1)], &[0]);
    let reordered = schematic_v3(
        [1, 1, 1],
        &[
            ("minecraft:lever[face=wall,powered=false]", 0),
            ("minecraft:lever[powered=false,face=wall]", 1),
        ],
        &[0],
    );
    let duplicates: Vec<(&str, String, String)> = [unprefixed, reordered]
        .into_iter()
        .map(|schematic| match read_error(&v3_bytes(schematic)) {
            SchemReadError::DuplicateState {
                path,
                first,
                second,
            } => (path, first, second),
            other => panic!("not a duplicate state: {other:?}"),
        })
        .collect();
    assert_eq!(
        duplicates,
        [
            (
                "Schematic.Blocks.Palette",
                "stone".to_owned(),
                "minecraft:stone".to_owned()
            ),
            (
                "Schematic.Blocks.Palette",
                "minecraft:lever[face=wall,powered=false]".to_owned(),
                "minecraft:lever[powered=false,face=wall]".to_owned()
            ),
        ]
    );

    let shared = schematic_v3(
        [1, 1, 1],
        &[("minecraft:air", 0), ("minecraft:stone", 0)],
        &[0],
    );
    let err = read_error(&v3_bytes(shared));
    assert!(
        matches!(
            &err,
            SchemReadError::SharedIndex { path: "Schematic.Blocks.Palette", index: 0, first, second }
                if first == "minecraft:air" && second == "minecraft:stone"
        ),
        "got {err:?}"
    );
}

#[test]
fn varints_of_four_and_five_bytes_read() {
    let schematic = schematic_v3(
        [3, 1, 1],
        &[
            ("minecraft:air", 0),
            ("minecraft:stone", 1 << 21),
            ("minecraft:dirt", 1 << 28),
        ],
        &[0x80, 0x80, 0x80, 0x01, 0x80, 0x80, 0x80, 0x80, 0x01, 0x00],
    );
    let read = read_schem(v3_bytes(schematic).as_slice()).expect("the file reads");
    assert_eq!(
        read.volume,
        BlockVolume::new(
            Dims { x: 3, y: 1, z: 1 },
            vec![
                BlockState::air(),
                BlockState::bare("minecraft:stone"),
                BlockState::bare("minecraft:dirt"),
            ],
            vec![PaletteIndex(1), PaletteIndex(2), PaletteIndex(0)],
            vec![],
        )
        .expect("a volume")
    );
}

#[test]
fn a_varint_past_five_bytes_or_32_bits_is_refused() {
    let six_bytes = schematic_v3(
        [1, 1, 1],
        &[("minecraft:air", 0)],
        &[0x80, 0x80, 0x80, 0x80, 0x80, 0x00],
    );
    let past_32_bits = schematic_v3(
        [1, 1, 1],
        &[("minecraft:air", 0)],
        &[0xff, 0xff, 0xff, 0xff, 0x1f],
    );
    let expected = "an array of varints, each an int of at most five bytes";
    assert_eq!(
        malformed([six_bytes, past_32_bits]),
        [
            ("Schematic.Blocks.Data".to_owned(), expected),
            ("Schematic.Blocks.Data".to_owned(), expected),
        ]
    );
}

#[test]
fn a_varint_past_i32_max_is_an_index_below_0_no_key_maps() {
    let indices: Vec<(usize, i32)> = [
        [0xff, 0xff, 0xff, 0xff, 0x0f],
        [0x80, 0x80, 0x80, 0x80, 0x08],
    ]
    .iter()
    .map(|data| {
        let schematic = schematic_v3([1, 1, 1], &[("minecraft:air", 0)], data);
        match read_error(&v3_bytes(schematic)) {
            SchemReadError::UnknownPaletteIndex { voxel, index } => (voxel, index),
            other => panic!("not an unknown index: {other:?}"),
        }
    })
    .collect();
    assert_eq!(indices, [(0, -1), (0, i32::MIN)]);
}

#[test]
fn an_extent_past_a_signed_short_reads_as_its_unsigned_value() {
    let schematic = schematic_v3([-25536, 1, 1], &[("minecraft:air", 0)], &vec![0; 40000]);
    let read = read_schem(v3_bytes(schematic).as_slice()).expect("the file reads");
    assert_eq!(
        read.volume,
        BlockVolume::new(
            Dims {
                x: 40000,
                y: 1,
                z: 1
            },
            vec![BlockState::air()],
            vec![PaletteIndex(0); 40000],
            vec![],
        )
        .expect("a volume")
    );
}

#[test]
fn fields_stored_in_any_order_read_alike() {
    let mut in_order = schematic_v3(
        [2, 1, 1],
        &[("minecraft:air", 0), ("minecraft:chest", 1)],
        &[1, 0],
    );
    in_order.insert(
        "Metadata",
        Tag::Compound(compound([("Name", string("order"))])),
    );
    in_order.insert("Offset", Tag::IntArray(vec![3, -2, 1]));
    let chest = compound([
        ("Pos", Tag::IntArray(vec![0, 0, 0])),
        ("Id", string("minecraft:chest")),
        ("Data", Tag::Compound(compound([("Lock", string("key"))]))),
    ]);
    blocks_mut(&mut in_order).insert("BlockEntities", Tag::List(List::of_compounds(vec![chest])));
    let mut reversed = Compound::new();
    for (name, tag) in in_order.entries.iter().rev() {
        let tag = match tag {
            Tag::Compound(inner) if name == "Blocks" => {
                let mut flipped = Compound::new();
                for (name, tag) in inner.entries.iter().rev() {
                    flipped.insert(name.clone(), tag.clone());
                }
                Tag::Compound(flipped)
            }
            tag => tag.clone(),
        };
        reversed.insert(name.clone(), tag);
    }
    let keys: Vec<&str> = reversed.entries.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "Offset",
            "Metadata",
            "Blocks",
            "Length",
            "Height",
            "Width",
            "DataVersion",
            "Version"
        ]
    );
    let expected = SpongeSchematic {
        version: 3,
        data_version: 4189,
        metadata: compound([("Name", string("order"))]),
        offset: (3, -2, 1),
        volume: BlockVolume::new(
            Dims { x: 2, y: 1, z: 1 },
            vec![BlockState::air(), BlockState::bare("minecraft:chest")],
            vec![PaletteIndex(1), PaletteIndex(0)],
            vec![StoredBlockEntity {
                pos: (0, 0, 0),
                id: "minecraft:chest".to_owned(),
                data: compound([("Lock", string("key"))]),
            }],
        )
        .expect("a volume"),
        entities: vec![],
    };
    let read = [in_order, reversed]
        .map(|schematic| read_schem(v3_bytes(schematic).as_slice()).expect("the file reads"));
    assert_eq!(read, [expected.clone(), expected]);
}

#[test]
fn a_block_entity_or_entity_of_the_wrong_shape_is_refused_by_its_index_and_field() {
    let with_entities = |entities: Vec<Compound>| {
        let mut schematic = air_v3();
        blocks_mut(&mut schematic).insert("BlockEntities", Tag::List(List::of_compounds(entities)));
        schematic
    };
    let chest = |pos: Vec<i32>| {
        compound([
            ("Pos", Tag::IntArray(pos)),
            ("Id", string("minecraft:chest")),
        ])
    };
    let no_id = with_entities(vec![compound([("Pos", Tag::IntArray(vec![0, 0, 0]))])]);
    let two_ints = with_entities(vec![chest(vec![0, 0])]);
    let negative = with_entities(vec![chest(vec![0, -1, 0])]);
    let outside = with_entities(vec![chest(vec![0, 0, 0]), chest(vec![1, 0, 0])]);
    let mut data_not_compound = chest(vec![0, 0, 0]);
    data_not_compound.insert("Data", Tag::Int(0));
    let data_not_compound = with_entities(vec![data_not_compound]);
    let mut ints = air_v3();
    blocks_mut(&mut ints).insert(
        "BlockEntities",
        Tag::List(List::of_tags(3, vec![Tag::Int(0)])),
    );
    let mut entities_of_ints = air_v3();
    entities_of_ints.insert("Entities", Tag::List(List::of_tags(3, vec![Tag::Int(0)])));
    assert_eq!(
        malformed([
            no_id,
            two_ints,
            negative,
            outside,
            data_not_compound,
            ints,
            entities_of_ints,
        ]),
        [
            (
                "Schematic.Blocks.BlockEntities[0].Id".to_owned(),
                "a block entity id string"
            ),
            (
                "Schematic.Blocks.BlockEntities[0].Pos".to_owned(),
                "an int array of three"
            ),
            (
                "Schematic.Blocks.BlockEntities[0].Pos".to_owned(),
                "three ints, each 0 or more"
            ),
            (
                "Schematic.Blocks.BlockEntities[1].Pos".to_owned(),
                "a voxel inside the schematic's `Width`, `Height` and `Length`"
            ),
            (
                "Schematic.Blocks.BlockEntities[0].Data".to_owned(),
                "a compound"
            ),
            (
                "Schematic.Blocks.BlockEntities".to_owned(),
                "a list of compounds"
            ),
            ("Schematic.Entities".to_owned(), "a list of compounds"),
        ]
    );
}

/// The voxel-sized version 3 schematic of air holding `block_entities`.
fn with_block_entities(block_entities: Vec<Compound>) -> Compound {
    let mut schematic = schematic_v3([2, 1, 1], &[("minecraft:air", 0)], &[0, 0]);
    blocks_mut(&mut schematic).insert(
        "BlockEntities",
        Tag::List(List::of_compounds(block_entities)),
    );
    schematic
}

#[test]
fn a_block_entity_at_an_earlier_ones_position_is_refused() {
    let chest = |x: i32| {
        compound([
            ("Pos", Tag::IntArray(vec![x, 0, 0])),
            ("Id", string("minecraft:chest")),
        ])
    };
    let err = read_error(&v3_bytes(with_block_entities(vec![
        chest(1),
        chest(0),
        chest(1),
    ])));
    assert!(
        matches!(
            &err,
            SchemReadError::DuplicatePosition { path, first: 0 }
                if path == "Schematic.Blocks.BlockEntities[2].Pos"
        ),
        "got {err:?}"
    );
}

#[test]
fn a_version_3_block_entity_field_beside_its_data_is_refused() {
    let sign = compound([
        ("Pos", Tag::IntArray(vec![0, 0, 0])),
        ("Id", string("minecraft:sign")),
        ("Data", Tag::Compound(Compound::new())),
        ("Text1", string("hi")),
    ]);
    assert_eq!(
        misplaced([v3_bytes(with_block_entities(vec![sign]))]),
        [(
            "Schematic.Blocks.BlockEntities[0].Text1".to_owned(),
            3,
            "Schematic.Blocks.BlockEntities[0].Data.Text1".to_owned()
        )]
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

#[test]
fn a_version_it_does_not_read_is_reported_over_a_part_it_would_explain() {
    let mut schematic = air_v3();
    blocks_mut(&mut schematic).insert(
        "BlockEntities",
        Tag::List(List::of_compounds(vec![Compound::new()])),
    );
    schematic.insert("Version", Tag::Int(4));
    let err = read_error(&v3_bytes(schematic.clone()));
    assert!(
        matches!(err, SchemReadError::UnsupportedVersion { version: 4 }),
        "got {err:?}"
    );
    schematic.insert("Version", Tag::Int(3));
    let err = read_error(&v3_bytes(schematic));
    assert!(
        matches!(
            &err,
            SchemReadError::Malformed { path, expected: "a block entity id string" }
                if path == "Schematic.Blocks.BlockEntities[0].Id"
        ),
        "got {err:?}"
    );

    let mut misplaced = air_v3();
    misplaced.insert("Version", Tag::Int(4));
    misplaced.insert("Palette", Tag::Compound(Compound::new()));
    let err = read_error(&v3_bytes(misplaced));
    assert!(
        matches!(err, SchemReadError::UnsupportedVersion { version: 4 }),
        "got {err:?}"
    );
}

/// A gzip-wrapped version 2 root that opens and then holds `header`, which cuts it short.
fn root_opening_with(header: &[u8]) -> Vec<u8> {
    let mut bytes = vec![10, 0, 0];
    bytes.extend(header);
    gzip(&bytes)
}

#[test]
fn a_list_or_array_longer_than_a_schematic_can_need_is_refused_before_it_is_read() {
    // A full-size schematic's varints, each of an index below `PALETTE_CAPACITY`.
    let limit = MAX_STRUCTURE_VOLUME * 3;
    let mut header = vec![7, 0, 9];
    header.extend(b"BlockData");
    header.extend(i32::try_from(limit + 1).expect("an i32").to_be_bytes());
    let err = read_error(&root_opening_with(&header));
    assert!(
        matches!(
            err,
            SchemReadError::Nbt(NbtReadError::TooLong {
                context: "byte_array",
                len,
                limit: l,
                offset: 15,
            }) if len == limit + 1 && l == limit
        ),
        "got {err:?}"
    );

    let mut header = vec![9, 0, 13];
    header.extend(b"BlockEntities");
    header.push(10);
    header.extend(
        i32::try_from(MAX_STRUCTURE_VOLUME + 1)
            .expect("an i32")
            .to_be_bytes(),
    );
    let err = read_error(&root_opening_with(&header));
    assert!(
        matches!(
            err,
            SchemReadError::Nbt(NbtReadError::TooLong {
                context: "list",
                len,
                limit,
                offset: 20,
            }) if len == MAX_STRUCTURE_VOLUME + 1 && limit == MAX_STRUCTURE_VOLUME
        ),
        "got {err:?}"
    );
}

/// The kind of the gzip failure `bytes` are refused with.
fn gzip_error(bytes: &[u8]) -> std::io::ErrorKind {
    match read_error(bytes) {
        SchemReadError::Nbt(NbtReadError::Io(io)) => io.kind(),
        other => panic!("not a gzip failure: {other:?}"),
    }
}

#[test]
fn a_gzip_trailer_that_does_not_match_is_refused() {
    let bytes = v3_bytes(air_v3());
    let mut crc = bytes.clone();
    let at = crc.len() - 8;
    crc[at] ^= 1;
    let mut size = bytes.clone();
    let at = size.len() - 1;
    size[at] ^= 1;
    let kinds = [crc.as_slice(), size.as_slice(), &bytes[..bytes.len() - 8]].map(gzip_error);
    assert_eq!(
        kinds,
        [
            std::io::ErrorKind::InvalidInput,
            std::io::ErrorKind::InvalidInput,
            std::io::ErrorKind::UnexpectedEof,
        ]
    );
}

#[test]
fn input_past_the_root_is_refused_in_the_stream_and_after_the_gzip_member() {
    let root = compound([("Schematic", Tag::Compound(air_v3()))]);
    let mut uncompressed = Vec::new();
    write_java_uncompressed(&mut uncompressed, "", &root).expect("the root writes");
    let end = uncompressed.len() as u64;
    let mut in_stream = uncompressed.clone();
    in_stream.push(0);
    let err = read_error(&gzip(&in_stream));
    assert!(
        matches!(err, SchemReadError::Nbt(NbtReadError::TrailingBytes { offset }) if offset == end),
        "got {err:?}"
    );

    let mut second_member = gzip(&uncompressed);
    second_member.extend(gzip(&uncompressed));
    let err = read_error(&second_member);
    assert!(
        matches!(err, SchemReadError::Nbt(NbtReadError::TrailingBytes { offset }) if offset == end),
        "got {err:?}"
    );

    let mut garbage = gzip(&uncompressed);
    garbage.extend(b"not a gzip member");
    assert_eq!(gzip_error(&garbage), std::io::ErrorKind::InvalidInput);
}
