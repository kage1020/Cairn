//! The `.litematic` writer: the files an IR splits into and the tag tree each holds.

use cairn_lang_core::block_array::{BlockArrayIr, Dims};
use cairn_lang_core::{Edition, WalkwayScopeKey};
use cairn_lang_formats::data_version::{JavaTarget, resolve_java_target};
use cairn_lang_formats::java_structure::JavaStructureError;
use cairn_lang_formats::litematic::{
    LitematicError, LitematicFile, LitematicRegion, litematic_files, prepare_litematic,
};
use cairn_lang_nbt::read_java_gzip;
use cairn_lang_nbt::tag::{Compound, List, Tag};

use crate::common::{array_of, comparator_array, ir_of, latest, lower_example};

/// A file's scope and stem, with each region's scope, name and position.
type FileShape<'a> = (&'a str, &'a str, Vec<(&'a str, &'a str, (i32, i32, i32))>);

fn target(version: &str) -> JavaTarget {
    resolve_java_target(version).expect("a targetable version")
}

/// The root `file` writes to for `target`, read back as a tree.
fn written_root(file: LitematicFile<'_>, target: &JavaTarget) -> Compound {
    let mut bytes = Vec::new();
    prepare_litematic(file, target)
        .expect("the file prepares")
        .write_gzip(&mut bytes)
        .expect("the file writes");
    let (name, root) = read_java_gzip(bytes.as_slice()).expect("the bytes read back");
    assert_eq!(name, "", "the root is unnamed");
    root
}

/// The only file `ir` writes to.
fn only_file(ir: &BlockArrayIr) -> LitematicFile<'_> {
    let mut files = litematic_files(ir);
    assert_eq!(files.len(), 1, "one file");
    files.remove(0)
}

fn xyz(x: i32, y: i32, z: i32) -> Tag {
    let mut compound = Compound::new();
    compound.insert("x", Tag::Int(x));
    compound.insert("y", Tag::Int(y));
    compound.insert("z", Tag::Int(z));
    Tag::Compound(compound)
}

fn string(value: &str) -> Tag {
    Tag::String(value.to_owned())
}

fn compound<const N: usize>(entries: [(&str, Tag); N]) -> Compound {
    let mut compound = Compound::new();
    for (name, tag) in entries {
        compound.insert(name, tag);
    }
    compound
}

fn entry<'c>(compound: &'c Compound, name: &str) -> &'c Compound {
    match compound.entries.get(name) {
        Some(Tag::Compound(inner)) => inner,
        other => panic!("`{name}` is not a compound: {other:?}"),
    }
}

#[test]
fn a_struct_is_one_file_of_one_region_at_the_origin() {
    let ir = ir_of(comparator_array("minecraft:comparator"));
    let file = only_file(&ir);
    assert_eq!(
        (file.scope.as_str(), file.stem.as_str()),
        ("struct::comparator", "comparator")
    );
    let root = written_root(file, &target("1.21.4"));

    let mut properties = Compound::new();
    for (key, value) in [("facing", "east"), ("mode", "compare"), ("powered", "true")] {
        properties.insert(key, string(value));
    }
    let palette = List::of_compounds(vec![
        compound([("Name", string("minecraft:air"))]),
        compound([("Name", string("minecraft:stone"))]),
        compound([
            ("Name", string("minecraft:comparator")),
            ("Properties", Tag::Compound(properties)),
        ]),
    ]);
    let tile_entity = compound([
        ("id", string("minecraft:comparator")),
        ("OutputSignal", Tag::Int(3)),
        ("x", Tag::Int(1)),
        ("y", Tag::Int(1)),
        ("z", Tag::Int(1)),
    ]);
    // Two bits an index, (y, z, x) order: six stone, then the comparator at voxel 9.
    let block_states = 0b10_00_00_00_01_01_01_01_01_01;
    let region = compound([
        ("BlockStatePalette", Tag::List(palette)),
        ("BlockStates", Tag::LongArray(vec![block_states])),
        (
            "TileEntities",
            Tag::List(List::of_compounds(vec![tile_entity])),
        ),
        ("PendingBlockTicks", Tag::List(List::empty())),
        ("PendingFluidTicks", Tag::List(List::empty())),
        ("Entities", Tag::List(List::empty())),
        ("Position", xyz(0, 0, 0)),
        ("Size", xyz(2, 2, 3)),
    ]);
    let metadata = compound([
        ("Name", string("comparator")),
        ("Author", string("Cairn")),
        ("Description", string("")),
        ("RegionCount", Tag::Int(1)),
        ("TotalVolume", Tag::Int(12)),
        ("TotalBlocks", Tag::Int(7)),
        ("EnclosingSize", xyz(2, 2, 3)),
    ]);
    let expected = compound([
        ("MinecraftDataVersion", Tag::Int(4189)),
        ("Version", Tag::Int(7)),
        ("SubVersion", Tag::Int(1)),
        ("Metadata", Tag::Compound(metadata)),
        (
            "Regions",
            Tag::Compound(compound([("comparator", Tag::Compound(region))])),
        ),
    ]);
    assert_eq!(root, expected);
}

#[test]
fn version_is_6_through_1_20_4_and_7_from_1_20_5() {
    let ir = ir_of(comparator_array("minecraft:comparator"));
    let stamped: Vec<(String, Tag, Tag)> = ["1.20.4", "1.21", "1.21.4", latest(Edition::Java)]
        .into_iter()
        .map(|version| {
            let root = written_root(only_file(&ir), &target(version));
            (
                version.to_owned(),
                root.entries["MinecraftDataVersion"].clone(),
                root.entries["Version"].clone(),
            )
        })
        .collect();
    let expected: Vec<(String, Tag, Tag)> = [
        ("1.20.4", 3700, 6),
        ("1.21", 3953, 7),
        ("1.21.4", 4189, 7),
        (
            latest(Edition::Java),
            target(latest(Edition::Java)).data_version,
            7,
        ),
    ]
    .into_iter()
    .map(|(version, data_version, schematic)| {
        (
            version.to_owned(),
            Tag::Int(data_version),
            Tag::Int(schematic),
        )
    })
    .collect();
    assert_eq!(stamped, expected);
}

#[test]
fn the_palette_keys_stay_name_and_properties_where_structures_turn_lower_case() {
    let ir = ir_of(comparator_array("minecraft:comparator"));
    let latest = target(latest(Edition::Java));
    assert!(
        latest.data_version >= 5006,
        "latest is past the lower-case switch"
    );
    let root = written_root(only_file(&ir), &latest);
    let region = entry(entry(&root, "Regions"), "comparator");
    let Tag::List(palette) = &region.entries["BlockStatePalette"] else {
        panic!("the palette is a list");
    };
    let keys: Vec<Vec<&str>> = palette
        .items
        .iter()
        .map(|item| match item {
            Tag::Compound(c) => c.entries.keys().map(String::as_str).collect(),
            other => panic!("not a compound: {other:?}"),
        })
        .collect();
    assert_eq!(
        keys,
        vec![vec!["Name"], vec!["Name"], vec!["Name", "Properties"]]
    );
}

#[test]
fn an_index_runs_on_from_one_long_into_the_next() {
    // Eight entries take three bits an index, so voxel 21 holds bits 63 to 65.
    let ids = [
        "minecraft:stone",
        "minecraft:dirt",
        "minecraft:sand",
        "minecraft:gravel",
        "minecraft:glass",
        "minecraft:oak_log",
        "minecraft:oak_planks",
    ];
    let mut slots = [0; 22];
    slots[0] = 1;
    slots[21] = 7;
    let array = array_of("run_on", Dims { x: 22, y: 1, z: 1 }, &ids, &slots);
    let ir = ir_of(array);
    let root = written_root(only_file(&ir), &target("1.21.4"));
    let region = entry(entry(&root, "Regions"), "run_on");
    assert_eq!(
        region.entries["BlockStates"],
        Tag::LongArray(vec![i64::MIN + 1, 0b11])
    );
}

#[test]
fn a_palette_of_one_entry_still_takes_two_bits_an_index() {
    let array = array_of("void", Dims { x: 33, y: 1, z: 1 }, &[], &[0; 33]);
    let ir = ir_of(array);
    let root = written_root(only_file(&ir), &target("1.21.4"));
    let region = entry(entry(&root, "Regions"), "void");
    assert_eq!(region.entries["BlockStates"], Tag::LongArray(vec![0, 0]));
    assert_eq!(entry(&root, "Metadata").entries["TotalBlocks"], Tag::Int(0));
}

#[test]
fn a_site_is_one_file_with_a_region_per_placement_and_walkway_at_its_origin() {
    let ir = lower_example("village.crn", Edition::Java, None);
    let files = litematic_files(&ir);
    let shape: Vec<FileShape<'_>> = files
        .iter()
        .map(|file| {
            let regions = file
                .regions
                .iter()
                .map(|region| (region.scope, region.name.as_str(), region.position))
                .collect();
            (file.scope.as_str(), file.stem.as_str(), regions)
        })
        .collect();
    let origin = |scope: &str| ir.placements[scope].origin;
    let walkway = |scope: &str| ir.walkways[scope].origin;
    let walkway_scopes: Vec<&str> = ir.walkways.keys().map(WalkwayScopeKey::as_str).collect();
    assert_eq!(
        shape,
        vec![(
            "site::hamlet",
            "hamlet",
            vec![
                (
                    "site::hamlet::home1",
                    "home1",
                    origin("site::hamlet::home1")
                ),
                (
                    "site::hamlet::home2",
                    "home2",
                    origin("site::hamlet::home2")
                ),
                (
                    "site::hamlet::home3",
                    "home3",
                    origin("site::hamlet::home3")
                ),
                (
                    walkway_scopes[0],
                    "hamlet_walkway_home1_entry__home2_entry",
                    walkway(walkway_scopes[0]),
                ),
                (
                    walkway_scopes[1],
                    "hamlet_walkway_home1_entry__home3_entry",
                    walkway(walkway_scopes[1]),
                ),
            ],
        )],
    );
}

#[test]
fn a_sites_metadata_counts_every_region_and_encloses_them_all() {
    let ir = lower_example("village.crn", Edition::Java, None);
    let file = only_file(&ir);
    let (mut min, mut max) = ([i32::MAX; 3], [i32::MIN; 3]);
    let (mut volume, mut regions) = (0, 0);
    for region in &file.regions {
        let dims = region.block_array.dims;
        let start = [region.position.0, region.position.1, region.position.2];
        for (axis, len) in [dims.x, dims.y, dims.z].into_iter().enumerate() {
            let len = i32::try_from(len).expect("small");
            min[axis] = min[axis].min(start[axis]);
            max[axis] = max[axis].max(start[axis] + len - 1);
        }
        volume += dims.volume();
        regions += 1;
    }
    let root = written_root(file, &target("1.21.4"));
    let metadata = entry(&root, "Metadata");
    assert_eq!(
        (
            &metadata.entries["Name"],
            &metadata.entries["RegionCount"],
            &metadata.entries["TotalVolume"],
            &metadata.entries["EnclosingSize"],
        ),
        (
            &string("hamlet"),
            &Tag::Int(regions),
            &Tag::Int(i32::try_from(volume).expect("small")),
            &xyz(
                max[0] - min[0] + 1,
                max[1] - min[1] + 1,
                max[2] - min[2] + 1
            ),
        ),
    );
    let names: Vec<&str> = entry(&root, "Regions")
        .entries
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        names,
        [
            "home1",
            "home2",
            "home3",
            "hamlet_walkway_home1_entry__home2_entry",
            "hamlet_walkway_home1_entry__home3_entry",
        ]
    );
}

fn one_region(array: &cairn_lang_core::block_array::BlockArray) -> LitematicFile<'_> {
    LitematicFile {
        scope: "struct::probe".to_owned(),
        stem: "probe".to_owned(),
        regions: vec![LitematicRegion {
            scope: "struct::probe",
            name: "probe".to_owned(),
            position: (0, 0, 0),
            block_array: array,
        }],
    }
}

#[test]
fn a_region_the_nbt_writer_would_refuse_is_refused_with_its_name() {
    let array = array_of("probe", Dims { x: 1, y: 1, z: 1 }, &["oak_planks"], &[1]);
    let err = prepare_litematic(one_region(&array), &target("1.21.4")).expect_err("abstract");
    assert!(
        matches!(
            &err,
            LitematicError::Region {
                region,
                source: JavaStructureError::AbstractPaletteEntry { id },
            } if region == "probe" && id == "oak_planks"
        ),
        "got {err:?}"
    );
}

#[test]
fn a_name_the_nbt_encoder_cannot_write_is_refused() {
    let array = array_of("probe", Dims { x: 1, y: 1, z: 1 }, &[], &[0]);
    let mut file = one_region(&array);
    file.regions[0].name = "h\u{f6}me".to_owned();
    let err = prepare_litematic(file, &target("1.21.4")).expect_err("not ASCII");
    assert!(
        matches!(&err, LitematicError::UnencodableName { name, .. } if name == "h\u{f6}me"),
        "got {err:?}"
    );
}

#[test]
fn a_region_reaching_past_i32_is_refused() {
    let array = array_of("probe", Dims { x: 2, y: 1, z: 1 }, &[], &[0, 0]);
    let mut file = one_region(&array);
    file.regions[0].position = (i32::MAX, 0, 0);
    let err = prepare_litematic(file, &target("1.21.4")).expect_err("past i32");
    assert!(
        matches!(
            &err,
            LitematicError::CornerOverflow { region, axis: "x", value }
                if region == "probe" && *value == i64::from(i32::MAX) + 1
        ),
        "got {err:?}"
    );
}

#[test]
fn an_enclosing_size_past_i32_is_refused() {
    let array = array_of("probe", Dims { x: 1, y: 1, z: 1 }, &[], &[0]);
    let mut file = one_region(&array);
    let mut far = file.regions[0].clone();
    far.name = "far".to_owned();
    far.position = (0, 0, i32::MAX);
    file.regions[0].position = (0, 0, i32::MIN);
    file.regions.push(far);
    let err = prepare_litematic(file, &target("1.21.4")).expect_err("past i32");
    assert!(
        matches!(
            &err,
            LitematicError::MetadataOverflow { field: "EnclosingSize.z", value }
                if *value == i64::from(u32::MAX) + 1
        ),
        "got {err:?}"
    );
}
