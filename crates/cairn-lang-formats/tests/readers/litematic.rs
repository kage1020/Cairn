//! `read_litematic` over what the writer wrote, and over files only another tool would write.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArrayIr, BlockState, Dims, PaletteIndex};
use cairn_lang_formats::data_version::{JavaTarget, resolve_java_target};
use cairn_lang_formats::litematic::{
    LitematicMetadata, LitematicReadError, LitematicSchematic, ReadRegion, litematic_files,
    prepare_litematic, read_litematic,
};
use cairn_lang_formats::volume::{BlockVolume, StoredBlockEntity};
use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{NbtReadError, write_java_gzip};

use crate::common::{array_of, comparator_array, ir_of, latest, lower_example, volume_of};

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
        regions: vec![ReadRegion {
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

/// A gzip-wrapped root `{Version, MinecraftDataVersion, Metadata, Regions: {r: region}}`.
fn litematic_with(version: i32, region: Compound) -> Vec<u8> {
    let mut regions = Compound::new();
    regions.insert("r", Tag::Compound(region));
    let mut root = Compound::new();
    root.insert("Version", Tag::Int(version));
    root.insert("MinecraftDataVersion", Tag::Int(4189));
    root.insert("Metadata", Tag::Compound(Compound::new()));
    root.insert("Regions", Tag::Compound(regions));
    let mut bytes = Vec::new();
    write_java_gzip(&mut bytes, "", &root).expect("the root writes");
    bytes
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

/// A region at `position` of `size`, its palette air then stone, its `BlockStates` as given.
fn region(position: [i32; 3], size: [i32; 3], block_states: Vec<i64>) -> Compound {
    let mut region = Compound::new();
    region.insert(
        "BlockStatePalette",
        Tag::List(List::of_compounds(vec![
            named("minecraft:air"),
            named("minecraft:stone"),
        ])),
    );
    region.insert("BlockStates", Tag::LongArray(block_states));
    region.insert("Position", xyz(position));
    region.insert("Size", xyz(size));
    region
}

fn read_error(bytes: &[u8]) -> LitematicReadError {
    read_litematic(bytes).expect_err("the file is refused")
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
        BlockVolume {
            dims: Dims { x: 2, y: 1, z: 1 },
            palette: vec![BlockState::air(), BlockState::bare("minecraft:stone")],
            voxels: vec![PaletteIndex(0), PaletteIndex(1)],
            block_entities: vec![],
        },
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
        read.regions[0].volume.block_entities,
        vec![StoredBlockEntity {
            pos: (1, 0, 0),
            id: "minecraft:chest".to_owned(),
            data,
        }],
    );
}

#[test]
fn a_version_from_before_the_flattening_is_refused() {
    let err = read_error(&litematic_with(4, region([0; 3], [1; 3], vec![0])));
    assert!(
        matches!(err, LitematicReadError::UnsupportedVersion { version: 4 }),
        "got {err:?}"
    );
}

#[test]
fn block_states_shorter_than_the_region_are_refused() {
    // 33 voxels of two bits need two longs.
    let err = read_error(&litematic_with(7, region([0; 3], [33, 1, 1], vec![0])));
    assert!(
        matches!(
            &err,
            LitematicReadError::BlockStatesTooShort { region, len: 1, needed: 2 } if region == "r"
        ),
        "got {err:?}"
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
fn a_region_past_the_block_array_budget_is_refused_before_its_states_are_read() {
    let err = read_error(&litematic_with(7, region([0; 3], [257, 256, 256], vec![])));
    assert!(
        matches!(
            &err,
            LitematicReadError::RegionTooLarge { region, volume, limit }
                if region == "r" && *volume == 257 * 256 * 256 && *limit == 256 * 256 * 256
        ),
        "got {err:?}"
    );
}

#[test]
fn a_field_of_the_wrong_shape_is_refused_by_its_path() {
    let mut no_position = region([0; 3], [1; 3], vec![0]);
    no_position.entries.shift_remove("Position");
    let zero = region([0; 3], [1, 0, 1], vec![0]);
    let mut nameless = region([0; 3], [1; 3], vec![0]);
    nameless.insert(
        "BlockStatePalette",
        Tag::List(List::of_compounds(vec![Compound::new()])),
    );
    let mut outside = region([0; 3], [1; 3], vec![0]);
    let mut entity = named("minecraft:chest");
    entity.insert("x", Tag::Int(1));
    entity.insert("y", Tag::Int(0));
    entity.insert("z", Tag::Int(0));
    outside.insert("TileEntities", Tag::List(List::of_compounds(vec![entity])));
    let mut no_states = region([0; 3], [1; 3], vec![]);
    no_states.entries.shift_remove("BlockStates");
    let refused: Vec<String> = [no_position, zero, nameless, outside, no_states]
        .into_iter()
        .map(|region| match read_error(&litematic_with(7, region)) {
            LitematicReadError::Malformed { path, .. } => path,
            other => panic!("not malformed: {other:?}"),
        })
        .collect();
    assert_eq!(
        refused,
        [
            "Regions.r.Position",
            "Regions.r.Size",
            "Regions.r.BlockStatePalette",
            "Regions.r.TileEntities",
            "Regions.r.BlockStates",
        ]
    );
}

#[test]
fn bytes_that_are_not_gzip_are_an_nbt_error() {
    let err = read_error(b"not a litematic");
    assert!(
        matches!(err, LitematicReadError::Nbt(NbtReadError::Io(_))),
        "got {err:?}"
    );
}
