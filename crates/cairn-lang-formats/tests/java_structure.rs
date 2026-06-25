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
            voxels[dims.index(x, 0, z).unwrap()] = cobble;
        }
    }
    // y=1: corner walls (z=0,x=0), rest air.
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
fn f1_unit_air_root_has_size_palette_blocks_entities_dataversion() {
    // AC F1: 1×1×1 all-air structure produces the canonical empty room.
    let target = target_1_21_4();
    let root = build_structure_tag(&unit_air(), &target).expect("build");
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
fn f3_palette_entry_without_properties_omits_properties_compound() {
    // AC F3: property-less states leave Properties out entirely.
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
fn f5_blocks_entries_count_equals_dims_volume() {
    // AC F5: every voxel — including air — has an entry.
    let root = build_structure_tag(&synthetic_cottage(), &target_1_21_4()).expect("build");
    let Tag::List(blocks) = &root.entries["blocks"] else {
        unreachable!()
    };
    assert_eq!(blocks.items.len(), 2 * 2 * 2);
}

#[test]
fn f6_blocks_entries_walk_yzx_order() {
    // AC F6: block list traversal matches voxel storage order.
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
fn f7_dataversion_matches_target() {
    // AC F7: the integer in the root reflects the target table.
    let t = resolve_java_target("1.20.4").expect("known");
    let root = build_structure_tag(&unit_air(), &t).expect("build");
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
    let err = build_structure_tag(&ba, &target_1_21_4()).expect_err("abstract");
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
fn output_filename_preserves_inner_colons_and_unicode() {
    // Scope keys that the IR could legitimately produce (a nested-namespace
    // notation in M3) should not be silently rewritten. We strip only the
    // exact `struct::` prefix and leave the rest verbatim, including any
    // subsequent `::`; downstream filename sanitisation is the OS layer's
    // responsibility on Windows.
    assert_eq!(output_filename("struct::deep::room"), "deep::room.nbt");
    // No prefix, no transformation: the scope key carries through verbatim.
    assert_eq!(output_filename("塔"), "塔.nbt");
}

#[test]
fn output_filename_handles_empty_input() {
    // Degenerate: an empty scope key produces a file named `.nbt`, which
    // is malformed on every OS we target. We don't try to fix it — IR
    // callers control the scope key, and a silent rename would mask the
    // bug.
    assert_eq!(output_filename(""), ".nbt");
}

#[test]
fn f10_gzip_output_begins_with_magic_bytes() {
    // AC F10: write_structure_gzip produces a gzip stream.
    let mut buf = Vec::new();
    write_structure_gzip(&mut buf, &unit_air(), &target_1_21_4()).expect("write");
    assert!(buf.len() >= 2);
    assert_eq!(&buf[..2], &[0x1f, 0x8b], "gzip magic prefix");
}

#[test]
fn f11_unit_air_debug_snapshot_is_stable() {
    // AC F11: stable debug snapshot of the tag tree so a future refactor
    // that accidentally changes Tag construction order is caught.
    let root = build_structure_tag(&unit_air(), &target_1_21_4()).expect("build");
    insta::assert_debug_snapshot!(root);
}

/// Run the full pipeline (parse → lower → resolve → block-array →
/// structure tag) so the cottage example reaches the Java backend through
/// the same code path the CLI uses. Built locally rather than imported
/// because the formats crate intentionally only depends on the IR + NBT
/// codec; the test layer is the right place to reach across into the parser.
fn cottage_structure_tag() -> cairn_lang_formats::java_structure::Compound {
    use cairn_lang_core::block_array::lower_to_block_array;
    use cairn_lang_core::{lower, parse, resolve};

    let source = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("examples")
            .join("cottage.crn"),
    )
    .expect("read cottage.crn");
    let module = parse(&source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir);
    let block_array = lower_to_block_array(&ir, &resolution, None);
    let ba = block_array
        .structures
        .get("struct::cottage")
        .expect("cottage struct lowered");
    build_structure_tag(ba, &target_1_21_4()).expect("build")
}

#[test]
fn f12_cottage_end_to_end_palette_carries_spruce_stairs_properties() {
    // M2-PR6: lowering the cottage example must produce a Java palette
    // where spruce_stairs appears with `facing` and `half` properties on
    // every entry. The Properties compound is the contract the Minecraft
    // structure block reads — losing it would silently strip the roof
    // orientation and render the gable as flat slabs.
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
fn f13_cottage_end_to_end_size_matches_overhang_inflated_dims() {
    // M2-PR6: cottage.crn declares size=9x7 walls height=4 roof overhang=1.
    // The Java structure root must mirror the lowering pass:
    //   x = 9 + 2 = 11
    //   z = 7 + 2 = 9
    //   y = 1 (floor) + 4 (walls) + ceil(9/2) (gable) = 10
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
