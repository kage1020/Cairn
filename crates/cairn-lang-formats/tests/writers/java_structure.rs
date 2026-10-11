//! The Java structure tag tree, and the file name a scope writes to.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_formats::data_version::{JavaTarget, resolve_java_target};
use cairn_lang_formats::java_structure::{
    JavaStructureError, OutputExt, build_structure_tag, output_filename, write_structure_gzip,
};
use cairn_lang_nbt::tag::Tag;

use crate::common::{latest, lower_example};

fn target_1_21_4() -> JavaTarget {
    resolve_java_target("1.21.4").expect("known target")
}

fn output_filename_nbt(source_scope: &str) -> String {
    output_filename(source_scope, OutputExt::Nbt)
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
    // A cobblestone floor and one plank wall cell, built without lowering.
    let mut palette = Palette::new_with_air();
    let cobble = palette.intern(BlockState::bare("minecraft:cobblestone"));
    let planks = palette.intern(BlockState::bare("minecraft:oak_planks"));
    let dims = Dims { x: 2, y: 2, z: 2 };
    let mut voxels = vec![PaletteIndex::AIR; dims.volume()];
    for z in 0..2 {
        for x in 0..2 {
            voxels[dims.index(x, 0, z).unwrap()] = cobble;
        }
    }
    voxels[dims.index(0, 1, 0).unwrap()] = planks;
    BlockArray {
        dims,
        palette,
        voxels,
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::cottage".to_owned(),
    }
}

#[test]
fn unit_air_root_has_size_palette_blocks_entities_dataversion() {
    let target = target_1_21_4();
    let root = build_structure_tag(&unit_air(), &target).expect("build");
    let keys: Vec<&String> = root.entries.keys().collect();
    assert_eq!(
        keys,
        vec!["size", "palette", "blocks", "entities", "DataVersion"]
    );

    match &root.entries["size"] {
        Tag::List(l) => assert_eq!(l.items, vec![Tag::Int(1), Tag::Int(1), Tag::Int(1)]),
        _ => panic!("size is not a List"),
    }
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
    match &root.entries["blocks"] {
        Tag::List(l) => {
            assert_eq!(l.items.len(), 1);
            match &l.items[0] {
                Tag::Compound(c) => {
                    assert_eq!(c.entries.get("state"), Some(&Tag::Int(0)));
                    match c.entries.get("pos") {
                        Some(Tag::List(ps)) => {
                            assert_eq!(ps.items, vec![Tag::Int(0), Tag::Int(0), Tag::Int(0)]);
                        }
                        _ => panic!("pos not a list"),
                    }
                }
                _ => panic!("block entry not a compound"),
            }
        }
        _ => panic!("blocks is not a List"),
    }
    match &root.entries["entities"] {
        Tag::List(l) => assert!(l.items.is_empty()),
        _ => panic!("entities is not a List"),
    }
    assert_eq!(
        root.entries.get("DataVersion"),
        Some(&Tag::Int(target.data_version))
    );
}

#[test]
fn cottage_palette_contains_three_concrete_entries() {
    let root = build_structure_tag(&synthetic_cottage(), &target_1_21_4()).expect("build");
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
fn palette_entry_without_properties_omits_properties_compound() {
    let root = build_structure_tag(&unit_air(), &target_1_21_4()).expect("build");
    let Tag::List(palette) = &root.entries["palette"] else {
        unreachable!()
    };
    let Tag::Compound(air) = &palette.items[0] else {
        unreachable!()
    };
    assert!(!air.entries.contains_key("Properties"));
}

#[test]
fn palette_entry_with_properties_keeps_them() {
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
    let root = build_structure_tag(&ba, &target_1_21_4()).expect("build");
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
fn blocks_entries_count_equals_dims_volume() {
    let root = build_structure_tag(&synthetic_cottage(), &target_1_21_4()).expect("build");
    let Tag::List(blocks) = &root.entries["blocks"] else {
        unreachable!()
    };
    assert_eq!(blocks.items.len(), 2 * 2 * 2);
}

#[test]
fn blocks_entries_walk_yzx_order() {
    let root = build_structure_tag(&synthetic_cottage(), &target_1_21_4()).expect("build");
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
fn dataversion_matches_target() {
    let t = resolve_java_target("1.20.4").expect("known");
    let root = build_structure_tag(&unit_air(), &t).expect("build");
    assert_eq!(root.entries.get("DataVersion"), Some(&Tag::Int(3700)));
}

#[test]
fn abstract_palette_id_rejected() {
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
    let err = build_structure_tag(&ba, &target_1_21_4()).expect_err("abstract");
    match err {
        JavaStructureError::AbstractPaletteEntry { id } => assert_eq!(id, "@cobblestone"),
        other => panic!("expected AbstractPaletteEntry, got {other:?}"),
    }
}

#[test]
fn output_filename_strips_struct_prefix() {
    assert_eq!(output_filename_nbt("struct::cottage"), "cottage.nbt");
    assert_eq!(output_filename_nbt("cottage"), "cottage.nbt");
}

#[test]
fn output_filename_strips_site_and_name_prefixes_to_place_id() {
    assert_eq!(output_filename_nbt("site::hamlet::home1"), "home1.nbt");
    assert_eq!(
        output_filename_nbt("site::village::cottage_2"),
        "cottage_2.nbt"
    );
}

#[test]
fn output_filename_preserves_inner_colons_and_unicode() {
    assert_eq!(output_filename_nbt("struct::deep::room"), "deep::room.nbt");
    assert_eq!(output_filename_nbt("塔"), "塔.nbt");
}

#[test]
fn output_filename_flattens_walkway_scope_to_site_prefixed_name() {
    assert_eq!(
        output_filename_nbt("walkway::hamlet::home1.entry__home2.entry"),
        "hamlet_walkway_home1_entry__home2_entry.nbt",
    );
    assert_eq!(
        output_filename_nbt("walkway::village::a.door__b.door"),
        "village_walkway_a_door__b_door.nbt",
    );
    assert_eq!(
        output_filename_nbt("walkway::no_site"),
        "walkway::no_site.nbt"
    );
}

#[test]
fn output_filename_handles_empty_input() {
    // Left malformed, since a silent rename would hide a bad scope key.
    assert_eq!(output_filename_nbt(""), ".nbt");
}

#[test]
fn gzip_output_begins_with_magic_bytes() {
    let mut buf = Vec::new();
    write_structure_gzip(&mut buf, &unit_air(), &target_1_21_4()).expect("write");
    assert!(buf.len() >= 2);
    assert_eq!(&buf[..2], &[0x1f, 0x8b], "gzip magic prefix");
}

#[test]
fn unit_air_debug_snapshot_is_stable() {
    let root = build_structure_tag(&unit_air(), &target_1_21_4()).expect("build");
    insta::assert_debug_snapshot!(root);
}

/// The cottage example's structure tag, lowered as `cairn compile --edition java` lowers it.
fn cottage_structure_tag() -> cairn_lang_formats::java_structure::Compound {
    let block_array = lower_example("cottage.crn", Edition::Java, Some(latest(Edition::Java)));
    let ba = block_array
        .structures
        .get("struct::cottage")
        .expect("cottage struct lowered");
    build_structure_tag(ba, &target_1_21_4()).expect("build")
}

#[test]
fn cottage_end_to_end_palette_carries_spruce_stairs_properties() {
    let root = cottage_structure_tag();
    let Tag::List(palette) = &root.entries["palette"] else {
        panic!("palette must be a list")
    };
    let mut stair_entries = 0;
    for item in &palette.items {
        let Tag::Compound(c) = item else {
            panic!("palette entry must be a compound")
        };
        let Some(Tag::String(name)) = c.entries.get("Name") else {
            panic!("palette Name must be a string")
        };
        if name != "minecraft:spruce_stairs" {
            continue;
        }
        stair_entries += 1;
        let Some(Tag::Compound(props)) = c.entries.get("Properties") else {
            panic!("spruce_stairs palette entry must carry Properties")
        };
        assert!(
            matches!(props.entries.get("facing"), Some(Tag::String(_))),
            "spruce_stairs entry must carry `facing`",
        );
        assert!(
            matches!(props.entries.get("half"), Some(Tag::String(_))),
            "spruce_stairs entry must carry `half`",
        );
    }
    assert!(
        stair_entries >= 3,
        "expected at least 3 spruce_stairs blockstates, got {stair_entries}",
    );
}

#[test]
fn cottage_end_to_end_size_matches_overhang_inflated_dims() {
    let root = cottage_structure_tag();
    match &root.entries["size"] {
        Tag::List(l) => assert_eq!(
            l.items,
            vec![Tag::Int(11), Tag::Int(10), Tag::Int(9)],
            "size should be [11, 10, 9] after overhang inflation",
        ),
        _ => panic!("size is not a List"),
    }
}

#[test]
fn a_voxel_naming_a_slot_the_palette_does_not_have_is_refused() {
    let mut ba = unit_air();
    ba.voxels[0] = PaletteIndex(7);
    let err = build_structure_tag(&ba, &target_1_21_4()).expect_err("out-of-range index");
    assert!(
        matches!(
            err,
            JavaStructureError::PaletteIndexOutOfRange { index: 7, len: 1 }
        ),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains('7') && err.to_string().contains('1'),
        "the message should name both the index and the palette length: {err}"
    );
}

#[test]
fn the_check_lands_exactly_on_the_first_index_that_is_not_a_slot() {
    // Both sides of the boundary, since only `len` tells `>=` from `>`.
    let ba = synthetic_cottage();
    let len = ba.palette.entries.len();
    let highest = ba.voxels.iter().map(|i| i.0).max().expect("non-empty");
    assert_eq!(
        usize::from(highest) + 1,
        len,
        "fixture should use the last palette slot, or this proves nothing"
    );
    build_structure_tag(&ba, &target_1_21_4()).expect("in-range indices");

    let mut past_the_end = ba;
    past_the_end.voxels[0] = PaletteIndex(u16::try_from(len).expect("small palette"));
    let err =
        build_structure_tag(&past_the_end, &target_1_21_4()).expect_err("one past the last slot");
    assert!(
        matches!(err, JavaStructureError::PaletteIndexOutOfRange { index, .. }
            if usize::from(index) == len),
        "unexpected error: {err}"
    );
}
