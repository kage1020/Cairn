//! AC F1–F11 for the Java vanilla structure backend.

use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_formats::data_version::{JavaTarget, resolve_java_target};
use cairn_lang_formats::java_structure::{
    JavaStructureError, build_structure_tag, output_filename, write_structure_gzip,
};
use cairn_lang_nbt::tag::Tag;

fn target_1_21_4() -> JavaTarget {
    resolve_java_target("1.21.4").expect("known target")
}

fn unit_air() -> BlockArray {
    BlockArray {
        dims: Dims { x: 1, y: 1, z: 1 },
        palette: Palette::new_with_air(),
        voxels: vec![PaletteIndex::AIR],
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::unit_air".to_owned(),
    }
}

fn synthetic_cottage() -> BlockArray {
    // A tiny cottage-shaped IR built by hand so the test does not depend on
    // the lowering pipeline. Two materials beyond air: a floor of
    // cobblestone and walls of oak_planks. dims = 2x2x2, voxels in (y,z,x)
    // order: floor row first, walls row second.
    let mut palette = Palette::new_with_air();
    let cobble = palette.intern(BlockState::bare("minecraft:cobblestone"));
    let planks = palette.intern(BlockState::bare("minecraft:oak_planks"));
    let dims = Dims { x: 2, y: 2, z: 2 };
    let mut voxels = vec![PaletteIndex::AIR; dims.volume()];
    // y=0 (floor): all cobblestone.
    for z in 0..2 {
        for x in 0..2 {
            let i = dims.index(x, y_const(0), z).unwrap();
            voxels[i] = cobble;
        }
    }
    // y=1: corner walls (z=0,x=0), rest air.
    let wall_at = dims.index(0, y_const(1), 0).unwrap();
    voxels[wall_at] = planks;
    BlockArray {
        dims,
        palette,
        voxels,
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::cottage".to_owned(),
    }
}

fn y_const(y: u32) -> u32 {
    y
}

#[test]
fn f1_unit_air_root_has_size_palette_blocks_entities_dataversion() {
    // AC F1: 1×1×1 all-air structure produces the canonical empty room.
    let target = target_1_21_4();
    let root = build_structure_tag(&unit_air(), target).expect("build");
    let keys: Vec<&String> = root.entries.keys().collect();
    assert_eq!(
        keys,
        vec!["size", "palette", "blocks", "entities", "DataVersion"]
    );

    // size = [1, 1, 1]
    match &root.entries["size"] {
        Tag::List(l) => assert_eq!(l.items, vec![Tag::Int(1), Tag::Int(1), Tag::Int(1)]),
        _ => panic!("size is not a List"),
    }
    // palette = [{Name: minecraft:air}]
    match &root.entries["palette"] {
        Tag::List(l) => {
            assert_eq!(l.items.len(), 1);
            match &l.items[0] {
                Tag::Compound(c) => {
                    assert_eq!(
                        c.entries.get("Name"),
                        Some(&Tag::String("minecraft:air".to_owned()))
                    );
                    assert!(!c.entries.contains_key("Properties"));
                }
                _ => panic!("palette entry not a compound"),
            }
        }
        _ => panic!("palette is not a List"),
    }
    // blocks = [{state: 0, pos: [0,0,0]}]
    match &root.entries["blocks"] {
        Tag::List(l) => {
            assert_eq!(l.items.len(), 1);
            match &l.items[0] {
                Tag::Compound(c) => {
                    assert_eq!(c.entries.get("state"), Some(&Tag::Int(0)));
                    match c.entries.get("pos") {
                        Some(Tag::List(ps)) => {
                            assert_eq!(ps.items, vec![Tag::Int(0), Tag::Int(0), Tag::Int(0)])
                        }
                        _ => panic!("pos not a list"),
                    }
                }
                _ => panic!("block entry not a compound"),
            }
        }
        _ => panic!("blocks is not a List"),
    }
    // entities = []
    match &root.entries["entities"] {
        Tag::List(l) => assert!(l.items.is_empty()),
        _ => panic!("entities is not a List"),
    }
    // DataVersion
    assert_eq!(
        root.entries.get("DataVersion"),
        Some(&Tag::Int(target.data_version))
    );
}

#[test]
fn f2_cottage_palette_contains_three_concrete_entries() {
    // AC F2: synthetic cottage IR exposes air / cobblestone / oak_planks.
    let root = build_structure_tag(&synthetic_cottage(), target_1_21_4()).expect("build");
    let Tag::List(palette) = &root.entries["palette"] else {
        unreachable!()
    };
    assert_eq!(palette.items.len(), 3);
    let names: Vec<String> = palette
        .items
        .iter()
        .map(|t| match t {
            Tag::Compound(c) => match c.entries.get("Name") {
                Some(Tag::String(s)) => s.clone(),
                _ => panic!("Name not a string"),
            },
            _ => panic!("palette entry not a compound"),
        })
        .collect();
    assert_eq!(
        names,
        vec![
            "minecraft:air".to_owned(),
            "minecraft:cobblestone".to_owned(),
            "minecraft:oak_planks".to_owned(),
        ]
    );
}

#[test]
fn f3_palette_entry_without_properties_omits_properties_compound() {
    // AC F3: property-less states leave Properties out entirely.
    let root = build_structure_tag(&unit_air(), target_1_21_4()).expect("build");
    let Tag::List(palette) = &root.entries["palette"] else {
        unreachable!()
    };
    let Tag::Compound(air) = &palette.items[0] else {
        unreachable!()
    };
    assert!(!air.entries.contains_key("Properties"));
}

#[test]
fn f4_palette_entry_with_properties_keeps_them() {
    // AC F4: non-empty properties round-trip into a Properties compound.
    let mut palette = Palette::new_with_air();
    let mut stairs = BlockState::bare("minecraft:oak_stairs");
    stairs
        .properties
        .insert("facing".to_owned(), "north".to_owned());
    stairs
        .properties
        .insert("half".to_owned(), "bottom".to_owned());
    let idx = palette.intern(stairs);
    let dims = Dims { x: 1, y: 1, z: 1 };
    let mut voxels = vec![PaletteIndex::AIR; 1];
    voxels[0] = idx;
    let ba = BlockArray {
        dims,
        palette,
        voxels,
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::stairs".to_owned(),
    };
    let root = build_structure_tag(&ba, target_1_21_4()).expect("build");
    let Tag::List(palette) = &root.entries["palette"] else {
        unreachable!()
    };
    let Tag::Compound(stairs_entry) = &palette.items[1] else {
        unreachable!()
    };
    let Some(Tag::Compound(props)) = stairs_entry.entries.get("Properties") else {
        panic!("Properties missing")
    };
    assert_eq!(
        props.entries.get("facing"),
        Some(&Tag::String("north".to_owned()))
    );
    assert_eq!(
        props.entries.get("half"),
        Some(&Tag::String("bottom".to_owned()))
    );
}

#[test]
fn f5_blocks_entries_count_equals_dims_volume() {
    // AC F5: every voxel — including air — has an entry.
    let root = build_structure_tag(&synthetic_cottage(), target_1_21_4()).expect("build");
    let Tag::List(blocks) = &root.entries["blocks"] else {
        unreachable!()
    };
    assert_eq!(blocks.items.len(), 2 * 2 * 2);
}

#[test]
fn f6_blocks_entries_walk_yzx_order() {
    // AC F6: block list traversal matches voxel storage order.
    let root = build_structure_tag(&synthetic_cottage(), target_1_21_4()).expect("build");
    let Tag::List(blocks) = &root.entries["blocks"] else {
        unreachable!()
    };
    let positions: Vec<(i32, i32, i32)> = blocks
        .items
        .iter()
        .map(|t| match t {
            Tag::Compound(c) => match c.entries.get("pos") {
                Some(Tag::List(l)) => (
                    int_at(&l.items, 0),
                    int_at(&l.items, 1),
                    int_at(&l.items, 2),
                ),
                _ => panic!("pos missing"),
            },
            _ => panic!("block not compound"),
        })
        .collect();
    let expected = vec![
        (0, 0, 0),
        (1, 0, 0),
        (0, 0, 1),
        (1, 0, 1),
        (0, 1, 0),
        (1, 1, 0),
        (0, 1, 1),
        (1, 1, 1),
    ];
    assert_eq!(positions, expected);
}

fn int_at(items: &[Tag], i: usize) -> i32 {
    match items[i] {
        Tag::Int(v) => v,
        _ => panic!("not an int"),
    }
}

#[test]
fn f7_dataversion_matches_target() {
    // AC F7: the integer in the root reflects the target table.
    let t = resolve_java_target("1.20.4").expect("known");
    let root = build_structure_tag(&unit_air(), t).expect("build");
    assert_eq!(root.entries.get("DataVersion"), Some(&Tag::Int(3700)));
}

#[test]
fn f8_abstract_palette_id_rejected() {
    // AC F8: an unresolved abstract token is a hard error, not silent air.
    let mut palette = Palette::new_with_air();
    palette.intern(BlockState::bare("@cobblestone"));
    let dims = Dims { x: 1, y: 1, z: 1 };
    let ba = BlockArray {
        dims,
        palette,
        voxels: vec![PaletteIndex::AIR],
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::abs".to_owned(),
    };
    let err = build_structure_tag(&ba, target_1_21_4()).expect_err("abstract");
    match err {
        JavaStructureError::AbstractPaletteEntry { id } => assert_eq!(id, "@cobblestone"),
        other => panic!("expected AbstractPaletteEntry, got {other:?}"),
    }
}

#[test]
fn f9_output_filename_strips_struct_prefix() {
    // AC F9: scope key → filename.
    assert_eq!(output_filename("struct::cottage"), "cottage.nbt");
    assert_eq!(output_filename("cottage"), "cottage.nbt");
}

#[test]
fn f10_gzip_output_begins_with_magic_bytes() {
    // AC F10: write_structure_gzip produces a gzip stream.
    let mut buf = Vec::new();
    write_structure_gzip(&mut buf, &unit_air(), target_1_21_4()).expect("write");
    assert!(buf.len() >= 2);
    assert_eq!(&buf[..2], &[0x1f, 0x8b], "gzip magic prefix");
}

#[test]
fn f11_unit_air_yaml_snapshot_is_stable() {
    // AC F11: stable snapshot of the tag tree so a future refactor that
    // accidentally changes Tag construction order is caught.
    let root = build_structure_tag(&unit_air(), target_1_21_4()).expect("build");
    insta::assert_debug_snapshot!(root);
}
