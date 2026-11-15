//! `read_litematic` over what the writer wrote, and over files only another tool would write.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{
    BlockArrayIr, BlockState, Dims, MAX_STRUCTURE_VOLUME, PALETTE_CAPACITY, PaletteIndex,
};
use cairn_lang_formats::data_version::{JavaTarget, resolve_java_target};
use cairn_lang_formats::litematic::{
    LitematicMetadata, LitematicReadError, LitematicReadRegion, LitematicSchematic,
    litematic_files, prepare_litematic, read_litematic,
};
use cairn_lang_formats::volume::{BlockVolume, StoredBlockEntity};
use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{NbtReadError, write_java_uncompressed};

use crate::common::{array_of, comparator_array, gzip, ir_of, latest, lower_example, volume_of};

fn target(version: &str) -> JavaTarget {
    resolve_java_target(version).expect("a targetable version")
}

/// Every file `ir` writes to for `target`, read back.
fn round_trip(ir: &BlockArrayIr, target: &JavaTarget) -> Vec<LitematicSchematic> {
    litematic_files(ir)
        .into_iter()
        .map(|file| {
            let mut bytes = Vec::new();
            prepare_litematic(file, target)
                .expect("the file prepares")
                .write_gzip(&mut bytes)
                .expect("the file writes");
            read_litematic(bytes.as_slice()).expect("the file reads back")
        })
        .collect()
}

#[test]
fn a_struct_reads_back_as_its_block_array_with_its_block_entity() {
    let array = comparator_array("minecraft:comparator");
    let read = round_trip(&ir_of(array.clone()), &target("1.20.4"));
    let expected = LitematicSchematic {
        data_version: 3700,
        version: 6,
        sub_version: Some(1),
        metadata: LitematicMetadata {
            name: Some("comparator".to_owned()),
            author: Some("Cairn".to_owned()),
            description: Some(String::new()),
            region_count: Some(1),
            total_volume: Some(12),
            total_blocks: Some(7),
            enclosing_size: Some([2, 2, 3]),
        },
        regions: vec![LitematicReadRegion {
            name: "comparator".to_owned(),
            min_corner: (0, 0, 0),
            volume: volume_of(&array),
            entities: vec![],
        }],
    };
    assert_eq!(read, vec![expected]);
}

#[test]
fn a_site_reads_back_region_by_region_at_each_origin() {
    let ir = lower_example("village.crn", Edition::Java, None);
    let read = round_trip(&ir, &target(latest(Edition::Java)));
    let regions: Vec<(String, (i32, i32, i32), BlockVolume)> = read
        .into_iter()
        .flat_map(|schematic| schematic.regions)
        .map(|region| (region.name, region.min_corner, region.volume))
        .collect();
    let expected: Vec<(String, (i32, i32, i32), BlockVolume)> = ir
        .structures
        .iter()
        .map(|(scope, array)| {
            let origin = ir
                .placements
                .get(scope)
                .map(|p| p.origin)
                .or_else(|| ir.walkways.get(scope.as_str()).map(|w| w.origin))
                .expect("every village structure is placed");
            (ir.artifact_stem(scope), origin, volume_of(array))
        })
        .collect();
    assert_eq!(regions, expected);
}

#[test]
fn every_shipped_example_reads_back_as_its_block_arrays() {
    let target = target(latest(Edition::Java));
    for (name, _) in crate::common::examples() {
        let ir = lower_example(&name, Edition::Java, None);
        let read: Vec<BlockVolume> = round_trip(&ir, &target)
            .into_iter()
            .flat_map(|schematic| schematic.regions)
            .map(|region| region.volume)
            .collect();
        let expected: Vec<BlockVolume> = ir.structures.values().map(volume_of).collect();
        assert_eq!(read, expected, "{name}");
    }
}

#[test]
fn a_palette_past_sixteen_entries_reads_back() {
    let ids: Vec<String> = (0..40).map(|i| format!("cairn:block_{i}")).collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let slots: Vec<u16> = (0..=40).rev().cycle().take(5 * 7 * 3).collect();
    let array = array_of("wide", Dims { x: 5, y: 7, z: 3 }, &ids, &slots);
    let read = round_trip(&ir_of(array.clone()), &target("1.21.4"));
    assert_eq!(read[0].regions[0].volume, volume_of(&array));
}

/// The uncompressed bytes of an unnamed root holding `entries` in order.
fn root_bytes(entries: Vec<(&str, Tag)>) -> Vec<u8> {
    let mut root = Compound::new();
    for (name, tag) in entries {
        root.insert(name, tag);
    }
    let mut bytes = Vec::new();
    write_java_uncompressed(&mut bytes, "", &root).expect("the root writes");
    bytes
}

/// `{r: region}`, as a root's `Regions`.
fn regions_of(region: Compound) -> Tag {
    let mut regions = Compound::new();
    regions.insert("r", Tag::Compound(region));
    Tag::Compound(regions)
}

/// The uncompressed root `{Version, MinecraftDataVersion, Metadata, Regions: {r: region}}`.
fn litematic_root(version: i32, region: Compound) -> Vec<u8> {
    root_bytes(vec![
        ("Version", Tag::Int(version)),
        ("MinecraftDataVersion", Tag::Int(4189)),
        ("Metadata", Tag::Compound(Compound::new())),
        ("Regions", regions_of(region)),
    ])
}

/// [`litematic_root`], gzip-wrapped.
fn litematic_with(version: i32, region: Compound) -> Vec<u8> {
    gzip(&litematic_root(version, region))
}

fn xyz([x, y, z]: [i32; 3]) -> Tag {
    let mut compound = Compound::new();
    compound.insert("x", Tag::Int(x));
    compound.insert("y", Tag::Int(y));
    compound.insert("z", Tag::Int(z));
    Tag::Compound(compound)
}

fn named(id: &str) -> Compound {
    let mut entry = Compound::new();
    entry.insert("Name", Tag::String(id.to_owned()));
    entry
}

/// A region at `position` of `size`, its palette air then stone, its fields in the order
/// Litematica's hash map writes them, `BlockStates` first.
fn region(position: [i32; 3], size: [i32; 3], block_states: Vec<i64>) -> Compound {
    let mut region = Compound::new();
    region.insert("BlockStates", Tag::LongArray(block_states));
    region.insert("Position", xyz(position));
    region.insert(
        "BlockStatePalette",
        Tag::List(List::of_compounds(vec![
            named("minecraft:air"),
            named("minecraft:stone"),
        ])),
    );
    region.insert("Size", xyz(size));
    region
}

fn read_error(bytes: &[u8]) -> LitematicReadError {
    read_litematic(bytes).expect_err("the file is refused")
}

/// The path and expectation of the `Malformed` refusal `bytes` raise.
fn malformed(bytes: &[u8]) -> (String, &'static str) {
    match read_error(bytes) {
        LitematicReadError::Malformed { path, expected } => (path, expected),
        other => panic!("not malformed: {other:?}"),
    }
}

#[test]
fn a_negative_size_reads_from_the_minimum_corner_it_reaches_back_to() {
    // Voxel 0 is air and voxel 1 stone, two bits each.
    let bytes = litematic_with(7, region([5, 0, -1], [-2, 1, -1], vec![0b01_00]));
    let read = read_litematic(bytes.as_slice()).expect("the file reads");
    let region = &read.regions[0];
    assert_eq!(region.min_corner, (4, 0, -1));
    assert_eq!(
        region.volume,
        BlockVolume::new(
            Dims { x: 2, y: 1, z: 1 },
            vec![BlockState::air(), BlockState::bare("minecraft:stone")],
            vec![PaletteIndex(0), PaletteIndex(1)],
            vec![],
        )
        .expect("a volume"),
    );
}

#[test]
fn a_tile_entity_keeps_every_field_but_its_id_and_position() {
    let mut chest = Compound::new();
    chest.insert("id", Tag::String("minecraft:chest".to_owned()));
    chest.insert("CustomName", Tag::String("\"loot\"".to_owned()));
    chest.insert("x", Tag::Int(1));
    chest.insert("y", Tag::Int(0));
    chest.insert("z", Tag::Int(0));
    let mut stored = region([0, 0, 0], [2, 1, 1], vec![0]);
    stored.insert("TileEntities", Tag::List(List::of_compounds(vec![chest])));
    let read = read_litematic(litematic_with(7, stored).as_slice()).expect("the file reads");
    let mut data = Compound::new();
    data.insert("CustomName", Tag::String("\"loot\"".to_owned()));
    assert_eq!(
        read.regions[0].volume.block_entities(),
        [StoredBlockEntity {
            pos: (1, 0, 0),
            id: "minecraft:chest".to_owned(),
            data,
        }],
    );
}

#[test]
fn fields_a_region_does_not_return_are_skipped() {
    let mut padded = region([0, 0, 0], [1, 1, 1], vec![0]);
    padded.insert(
        "PendingBlockTicks",
        Tag::List(List::of_compounds(vec![Compound::new(); 3])),
    );
    padded.insert("Unknown", Tag::Compound(named("minecraft:stone")));
    let read = read_litematic(litematic_with(7, padded).as_slice()).expect("the file reads");
    let plain = read_litematic(litematic_with(7, region([0; 3], [1; 3], vec![0])).as_slice())
        .expect("the file reads");
    assert_eq!(read, plain);
}

#[test]
fn versions_5_to_7_read_and_the_versions_around_them_are_refused() {
    let read: Vec<i32> = [5, 6, 7]
        .into_iter()
        .map(|version| {
            read_litematic(litematic_with(version, region([0; 3], [1; 3], vec![0])).as_slice())
                .unwrap_or_else(|err| panic!("version {version}: {err}"))
                .version
        })
        .collect();
    assert_eq!(read, [5, 6, 7]);
    for version in [4, 8] {
        let err = read_error(&litematic_with(version, region([0; 3], [1; 3], vec![0])));
        assert!(
            matches!(err, LitematicReadError::UnsupportedVersion { version: v } if v == version),
            "version {version}: got {err:?}"
        );
    }
}

#[test]
fn an_unsupported_version_is_reported_over_a_region_it_would_explain() {
    // Two voxels need one long; the region holds none, and its `Version` comes after it.
    let short = region([0; 3], [2, 1, 1], vec![]);
    let bytes = gzip(&root_bytes(vec![
        ("MinecraftDataVersion", Tag::Int(4189)),
        ("Metadata", Tag::Compound(Compound::new())),
        ("Regions", regions_of(short.clone())),
        ("Version", Tag::Int(99)),
    ]));
    let err = read_error(&bytes);
    assert!(
        matches!(err, LitematicReadError::UnsupportedVersion { version: 99 }),
        "got {err:?}"
    );

    let bytes = gzip(&root_bytes(vec![
        ("MinecraftDataVersion", Tag::Int(4189)),
        ("Metadata", Tag::Compound(Compound::new())),
        ("Regions", regions_of(short)),
        ("Version", Tag::Int(7)),
    ]));
    let err = read_error(&bytes);
    assert!(
        matches!(
            &err,
            LitematicReadError::BlockStatesLength { region, len: 0, needed: 1, bits: 2 }
                if region == "r"
        ),
        "a supported version reports the region, got {err:?}"
    );
}

#[test]
fn block_states_of_any_length_but_the_one_the_voxels_fill_are_refused() {
    // 33 voxels of two bits fill two longs.
    for (longs, len) in [(vec![0], 1), (vec![0, 0, 0], 3)] {
        let err = read_error(&litematic_with(7, region([0; 3], [33, 1, 1], longs)));
        assert!(
            matches!(
                &err,
                LitematicReadError::BlockStatesLength { region, len: l, needed: 2, bits: 2 }
                    if region == "r" && *l == len
            ),
            "{len} longs: got {err:?}"
        );
    }
}

#[test]
fn bits_set_past_the_last_voxel_are_refused() {
    // Two voxels of two bits use bits 0 to 3; bit 4 would be a third voxel's.
    let err = read_error(&litematic_with(
        7,
        region([0; 3], [2, 1, 1], vec![0b1_00_00]),
    ));
    assert!(
        matches!(
            &err,
            LitematicReadError::BlockStatesPadding { region, bits: 2, len: 2 } if region == "r"
        ),
        "got {err:?}"
    );
    let full = read_litematic(litematic_with(7, region([0; 3], [32, 1, 1], vec![-1])).as_slice());
    assert!(
        matches!(
            &full,
            Err(LitematicReadError::PaletteIndexOutOfRange {
                voxel: 0,
                index: 3,
                ..
            })
        ),
        "a long the voxels fill has no padding to check, got {full:?}"
    );
}

#[test]
fn an_index_past_the_palette_is_refused_at_its_voxel() {
    let err = read_error(&litematic_with(7, region([0; 3], [2, 1, 1], vec![0b11_00])));
    assert!(
        matches!(
            &err,
            LitematicReadError::PaletteIndexOutOfRange { region, voxel: 1, index: 3, len: 2 }
                if region == "r"
        ),
        "got {err:?}"
    );
}

#[test]
fn a_region_past_the_block_array_budget_is_refused_before_its_states_are_decoded() {
    let err = read_error(&litematic_with(7, region([0; 3], [257, 256, 256], vec![])));
    assert!(
        matches!(
            &err,
            LitematicReadError::RegionTooLarge { region, volume, limit }
                if region == "r" && *volume == 257 * 256 * 256 && *limit == MAX_STRUCTURE_VOLUME
        ),
        "got {err:?}"
    );
}

/// A region whose palette is air and then `len - 1` other blocks, every voxel air.
fn region_with_palette(len: usize) -> Compound {
    let mut palette = vec![named("minecraft:air")];
    palette.extend((1..len).map(|i| named(&format!("cairn:block_{i}"))));
    let mut region = region([0; 3], [1; 3], vec![0]);
    region.insert("BlockStatePalette", Tag::List(List::of_compounds(palette)));
    region
}

#[test]
fn a_palette_past_a_block_arrays_is_refused_by_its_length() {
    let read = read_litematic(litematic_with(7, region_with_palette(PALETTE_CAPACITY)).as_slice())
        .expect("a full palette reads");
    assert_eq!(read.regions[0].volume.palette().len(), PALETTE_CAPACITY);

    let err = read_error(&litematic_with(
        7,
        region_with_palette(PALETTE_CAPACITY + 1),
    ));
    assert!(
        matches!(
            &err,
            LitematicReadError::PaletteTooLarge { region, len, limit }
                if region == "r" && *len == PALETTE_CAPACITY + 1 && *limit == PALETTE_CAPACITY
        ),
        "got {err:?}"
    );
}

/// The gzip of a root whose region `r` opens with `header`, a list or array declaring its length,
/// and ends there.
fn region_opening_with(header: &[u8]) -> Vec<u8> {
    let mut bytes = vec![10, 0, 0, 10, 0, 7];
    bytes.extend(b"Regions");
    bytes.extend([10, 0, 1, b'r']);
    bytes.extend(header);
    gzip(&bytes)
}

#[test]
fn a_list_or_array_longer_than_a_region_can_need_is_refused_before_it_is_read() {
    // At the 16 bits a full palette takes, four indices fill a long.
    let longs = MAX_STRUCTURE_VOLUME / 4;
    let mut header = vec![12, 0, 11];
    header.extend(b"BlockStates");
    header.extend(i32::try_from(longs + 1).expect("an i32").to_be_bytes());
    let err = read_error(&region_opening_with(&header));
    assert!(
        matches!(
            err,
            LitematicReadError::Nbt(NbtReadError::TooLong {
                context: "long_array",
                len,
                limit,
                offset: 31,
            }) if len == longs + 1 && limit == longs
        ),
        "got {err:?}"
    );

    let mut header = vec![9, 0, 12];
    header.extend(b"TileEntities");
    header.push(10);
    header.extend(
        i32::try_from(MAX_STRUCTURE_VOLUME + 1)
            .expect("an i32")
            .to_be_bytes(),
    );
    let err = read_error(&region_opening_with(&header));
    assert!(
        matches!(
            err,
            LitematicReadError::Nbt(NbtReadError::TooLong {
                context: "list",
                len,
                limit,
                offset: 33,
            }) if len == MAX_STRUCTURE_VOLUME + 1 && limit == MAX_STRUCTURE_VOLUME
        ),
        "got {err:?}"
    );
}

/// A `TileEntities` list of one chest at `pos`, or without an `id` when `id` is `None`.
fn with_chest(mut region: Compound, id: Option<&str>, [x, y, z]: [i32; 3]) -> Compound {
    let mut chest = Compound::new();
    if let Some(id) = id {
        chest.insert("id", Tag::String(id.to_owned()));
    }
    chest.insert("x", Tag::Int(x));
    chest.insert("y", Tag::Int(y));
    chest.insert("z", Tag::Int(z));
    region.insert("TileEntities", Tag::List(List::of_compounds(vec![chest])));
    region
}

#[test]
fn a_field_of_the_wrong_shape_is_refused_by_its_path() {
    let one = || region([0; 3], [1; 3], vec![0]);
    let mut no_position = one();
    no_position.entries.shift_remove("Position");
    let zero = region([0; 3], [1, 0, 1], vec![0]);
    let mut nameless = one();
    nameless.insert(
        "BlockStatePalette",
        Tag::List(List::of_compounds(vec![
            named("minecraft:air"),
            Compound::new(),
        ])),
    );
    let mut flat_properties = one();
    let mut stairs = named("minecraft:oak_stairs");
    stairs.insert("Properties", Tag::String("facing=east".to_owned()));
    flat_properties.insert(
        "BlockStatePalette",
        Tag::List(List::of_compounds(vec![stairs])),
    );
    let mut int_property = one();
    let mut stairs = named("minecraft:oak_stairs");
    let mut properties = Compound::new();
    properties.insert("facing", Tag::Int(2));
    stairs.insert("Properties", Tag::Compound(properties));
    int_property.insert(
        "BlockStatePalette",
        Tag::List(List::of_compounds(vec![stairs])),
    );
    let mut ints = one();
    ints.insert("BlockStatePalette", Tag::List(List::of_ints([0])));
    let mut no_states = one();
    no_states.entries.shift_remove("BlockStates");
    let refused: Vec<(String, &str)> = [
        no_position,
        zero,
        nameless,
        flat_properties,
        int_property,
        ints,
        with_chest(one(), None, [0, 0, 0]),
        with_chest(one(), Some("minecraft:chest"), [0, -1, 0]),
        with_chest(one(), Some("minecraft:chest"), [0, 0, 1]),
        no_states,
    ]
    .into_iter()
    .map(|region| malformed(&litematic_with(7, region)))
    .collect();
    let expected = [
        ("Regions.r.Position", "a compound of x, y and z ints"),
        ("Regions.r.Size", "a compound of non-zero x, y and z ints"),
        ("Regions.r.BlockStatePalette[1].Name", "a block id string"),
        (
            "Regions.r.BlockStatePalette[0].Properties",
            "a compound of strings",
        ),
        (
            "Regions.r.BlockStatePalette[0].Properties.facing",
            "a string",
        ),
        ("Regions.r.BlockStatePalette", "a list of compounds"),
        ("Regions.r.TileEntities[0].id", "a block entity id string"),
        ("Regions.r.TileEntities[0].y", "an int, 0 or more"),
        (
            "Regions.r.TileEntities[0].z",
            "an int below the region's `Size` on that axis",
        ),
        ("Regions.r.BlockStates", "a long array"),
    ]
    .map(|(path, expected)| (path.to_owned(), expected));
    assert_eq!(refused, expected);
}

#[test]
fn a_root_without_its_compounds_is_refused_by_their_names() {
    let without = |name: &str| {
        let mut entries = vec![
            ("Version", Tag::Int(7)),
            ("MinecraftDataVersion", Tag::Int(4189)),
            ("Metadata", Tag::Compound(Compound::new())),
            ("Regions", Tag::Compound(Compound::new())),
        ];
        entries.retain(|(entry, _)| *entry != name);
        gzip(&root_bytes(entries))
    };
    let regions_as_a_list = gzip(&root_bytes(vec![
        ("Version", Tag::Int(7)),
        ("MinecraftDataVersion", Tag::Int(4189)),
        ("Metadata", Tag::Compound(Compound::new())),
        ("Regions", Tag::List(List::of_compounds(vec![]))),
    ]));
    let refused: Vec<(String, &str)> = [
        without("Version"),
        without("MinecraftDataVersion"),
        without("Metadata"),
        without("Regions"),
        regions_as_a_list,
    ]
    .iter()
    .map(|bytes| malformed(bytes))
    .collect();
    let expected = [
        ("Version", "an int"),
        ("MinecraftDataVersion", "an int"),
        ("Metadata", "a compound"),
        ("Regions", "a compound"),
        ("Regions", "a compound"),
    ]
    .map(|(path, expected)| (path.to_owned(), expected));
    assert_eq!(refused, expected);
    let empty = read_litematic(without("").as_slice()).expect("no regions is a schematic");
    assert_eq!(empty.regions, []);
}

#[test]
fn bytes_that_are_not_gzip_are_an_nbt_error() {
    let err = read_error(b"not a litematic");
    assert!(
        matches!(err, LitematicReadError::Nbt(NbtReadError::Io(_))),
        "got {err:?}"
    );
}

/// The kind of the gzip failure `bytes` are refused with.
fn gzip_error(bytes: &[u8]) -> std::io::ErrorKind {
    match read_error(bytes) {
        LitematicReadError::Nbt(NbtReadError::Io(io)) => io.kind(),
        other => panic!("not a gzip failure: {other:?}"),
    }
}

#[test]
fn a_gzip_trailer_that_does_not_match_is_refused() {
    let bytes = litematic_with(7, region([0; 3], [1; 3], vec![0]));
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
    let root = litematic_root(7, region([0; 3], [1; 3], vec![0]));
    let end = root.len() as u64;
    let mut in_stream = root.clone();
    in_stream.push(0);
    let err = read_error(&gzip(&in_stream));
    assert!(
        matches!(err, LitematicReadError::Nbt(NbtReadError::TrailingBytes { offset }) if offset == end),
        "got {err:?}"
    );

    let mut second_member = gzip(&root);
    second_member.extend(gzip(&root));
    let err = read_error(&second_member);
    assert!(
        matches!(err, LitematicReadError::Nbt(NbtReadError::TrailingBytes { offset }) if offset == end),
        "got {err:?}"
    );

    let mut garbage = gzip(&root);
    garbage.extend(b"not a gzip member");
    assert_eq!(gzip_error(&garbage), std::io::ErrorKind::InvalidInput);
}
