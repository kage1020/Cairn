//! `read_mcstructure` over what the `.mcstructure` writer wrote, and over files only the game or
//! another tool would write.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, Dims, PaletteIndex};
use cairn_lang_formats::bedrock_state::translate_states;
use cairn_lang_formats::bedrock_structure::{
    BedrockBlock, McStructureReadError, build_mcstructure_tag, read_mcstructure, write_mcstructure,
};
use cairn_lang_formats::data_version::{BedrockTarget, resolve_bedrock_target};
use cairn_lang_formats::volume::StoredBlockEntity;
use cairn_lang_nbt::NbtReadError;
use cairn_lang_nbt::tag::{Compound, List, Tag};

use crate::common::{comparator_array, examples, latest, lower_example, states_for};

fn target(version: &str) -> BedrockTarget {
    resolve_bedrock_target(version).expect("a targetable version")
}

fn bytes(root: &Compound) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_mcstructure(&mut bytes, root).expect("the root writes");
    bytes
}

fn structure_mut(root: &mut Compound) -> &mut Compound {
    match root.entries.get_mut("structure") {
        Some(Tag::Compound(structure)) => structure,
        other => panic!("`structure` is not a compound: {other:?}"),
    }
}

fn layer_mut(root: &mut Compound, layer: usize) -> &mut Vec<Tag> {
    match structure_mut(root).entries.get_mut("block_indices") {
        Some(Tag::List(layers)) => match &mut layers.items[layer] {
            Tag::List(items) => &mut items.items,
            other => panic!("a layer is not a list: {other:?}"),
        },
        other => panic!("`block_indices` is not a list: {other:?}"),
    }
}

#[test]
fn a_struct_reads_back_in_the_grids_order_with_its_water_and_block_entity() {
    let mut array = comparator_array("minecraft:powered_comparator");
    let mut slab = BlockState::bare("minecraft:oak_slab");
    slab.properties
        .insert("waterlogged".to_owned(), "true".to_owned());
    let wet = array.palette.intern(slab);
    let at = array.dims.index(0, 1, 2).expect("in dims");
    array.voxels[at] = wet;
    let target = target("1.21.60");
    let (root, _) =
        build_mcstructure_tag(&array, &target, &states_for(&target)).expect("the array writes");
    let read = read_mcstructure(bytes(&root).as_slice()).expect("the file reads back");
    assert_eq!(read.world_origin, (0, 0, 0));
    assert_eq!(read.entities, []);
    let volume = &read.volume;
    assert_eq!(volume.dims, Dims { x: 2, y: 2, z: 3 });
    let names: Vec<(&str, Option<&str>)> = volume
        .blocks
        .iter()
        .zip(&volume.liquids)
        .map(|(block, liquid)| {
            let name = |slot: &Option<PaletteIndex>| {
                slot.map(|slot| volume.palette[usize::from(slot.0)].name.as_str())
            };
            (name(block).expect("no structure void"), name(liquid))
        })
        .collect();
    let stone = ("minecraft:stone", None);
    let air = ("minecraft:air", None);
    assert_eq!(
        names,
        [
            stone,
            stone,
            stone,
            stone,
            stone,
            stone,
            air,
            air,
            air,
            ("minecraft:powered_comparator", None),
            ("minecraft:oak_slab", Some("minecraft:water")),
            air,
        ]
    );
    assert!(
        volume
            .palette
            .iter()
            .all(|block| block.version == Some(target.block_version))
    );
    let mut fields = Compound::new();
    fields.insert("OutputSignal", Tag::Int(3));
    assert_eq!(
        volume.block_entities,
        [StoredBlockEntity {
            pos: (1, 1, 1),
            id: "Comparator".to_owned(),
            data: fields,
        }]
    );
}

#[test]
fn every_shipped_example_reads_back_as_the_bedrock_blocks_its_arrays_translate_to() {
    let target = target(latest(Edition::Bedrock));
    let states = states_for(&target);
    for (name, _) in examples() {
        let ir = lower_example(&name, Edition::Bedrock, Some(&target.mc_version));
        for array in ir.structures.values() {
            let (root, _) =
                build_mcstructure_tag(array, &target, &states).expect("the array writes");
            let read = read_mcstructure(bytes(&root).as_slice()).expect("the file reads back");
            let read_blocks: Vec<(String, Compound)> = read
                .volume
                .blocks
                .iter()
                .map(|slot| {
                    let block = &read.volume.palette[usize::from(slot.expect("no void").0)];
                    (block.name.clone(), block.states.clone())
                })
                .collect();
            let translated: Vec<(String, Compound)> = array
                .voxels
                .iter()
                .map(|slot| {
                    let state = &array.palette.entries[usize::from(slot.0)];
                    let form = translate_states(&states, state, &[]).expect("the state maps");
                    (form.id, form.states)
                })
                .collect();
            assert_eq!(read_blocks, translated, "{name}");
        }
    }
}

/// A one-voxel structure of stone, at 1.21.60.
fn one_stone() -> Compound {
    let target = target("1.21.60");
    let array =
        crate::common::array_of("one", Dims { x: 1, y: 1, z: 1 }, &["minecraft:stone"], &[1]);
    build_mcstructure_tag(&array, &target, &states_for(&target))
        .expect("the array writes")
        .0
}

#[test]
fn a_structure_void_reads_as_no_block() {
    let mut root = one_stone();
    layer_mut(&mut root, 0)[0] = Tag::Int(-1);
    let read = read_mcstructure(bytes(&root).as_slice()).expect("the file reads");
    assert_eq!(
        (read.volume.blocks, read.volume.liquids),
        (vec![None], vec![None])
    );
}

#[test]
fn a_layer_or_index_the_structure_cannot_hold_is_refused() {
    let mut short = one_stone();
    layer_mut(&mut short, 1).push(Tag::Int(-1));
    let mut past = one_stone();
    layer_mut(&mut past, 0)[0] = Tag::Int(5);
    let mut version = one_stone();
    version.insert("format_version", Tag::Int(2));
    let mut huge = one_stone();
    huge.insert("size", Tag::List(List::of_ints([300, 300, 300])));
    let refused: Vec<String> = [short, past, version, huge]
        .iter()
        .map(|root| {
            read_mcstructure(bytes(root).as_slice())
                .expect_err("refused")
                .to_string()
        })
        .collect();
    assert_eq!(
        refused,
        [
            "`block_indices[1]` holds 2 indices; the structure's voxels need 1",
            "`block_indices[0]` has palette index 5 at voxel 0, past the palette's 2 entries",
            "`.mcstructure` `format_version` 2 is not one this reader reads (1)",
            "the structure holds 27000000 voxels, past the 16777216 a block array may hold",
        ]
    );
}

#[test]
fn a_field_of_the_wrong_shape_is_refused_by_its_path() {
    let mut no_version = one_stone();
    no_version.entries.shift_remove("format_version");
    let mut short_size = one_stone();
    short_size.insert("size", Tag::List(List::of_ints([1, 1])));
    let mut no_palette = one_stone();
    structure_mut(&mut no_palette)
        .entries
        .shift_remove("palette");
    let mut one_layer = one_stone();
    if let Some(Tag::List(layers)) = structure_mut(&mut one_layer)
        .entries
        .get_mut("block_indices")
    {
        layers.items.pop();
    }
    let mut nameless = one_stone();
    if let Some(Tag::Compound(palette)) = structure_mut(&mut nameless).entries.get_mut("palette")
        && let Some(Tag::Compound(default)) = palette.entries.get_mut("default")
    {
        let mut entry = Compound::new();
        entry.insert("states", Tag::Compound(Compound::new()));
        default.insert("block_palette", Tag::List(List::of_compounds(vec![entry])));
    }
    let mut stray = one_stone();
    if let Some(Tag::Compound(palette)) = structure_mut(&mut stray).entries.get_mut("palette")
        && let Some(Tag::Compound(default)) = palette.entries.get_mut("default")
    {
        let mut data = Compound::new();
        data.insert("7", Tag::Compound(Compound::new()));
        default.insert("block_position_data", Tag::Compound(data));
    }
    let refused: Vec<(String, &str)> = [
        no_version, short_size, no_palette, one_layer, nameless, stray,
    ]
    .iter()
    .map(|root| match read_mcstructure(bytes(root).as_slice()) {
        Err(McStructureReadError::Malformed { path, expected }) => (path, expected),
        other => panic!("not malformed: {other:?}"),
    })
    .collect();
    assert_eq!(
        refused,
        [
            ("format_version".to_owned(), "an int"),
            ("size".to_owned(), "a list of three ints, none negative"),
            ("structure.palette.default".to_owned(), "a compound"),
            (
                "structure.block_indices".to_owned(),
                "a list of two lists of ints"
            ),
            (
                "structure.palette.default.block_palette".to_owned(),
                "a list of blocks, each a `name` and its `states`"
            ),
            (
                "structure.palette.default.block_position_data.7".to_owned(),
                "keyed by a voxel's index"
            ),
        ]
    );
}

#[test]
fn a_palette_entry_keeps_its_typed_states_and_version() {
    let read = read_mcstructure(bytes(&one_stone()).as_slice()).expect("the file reads");
    assert_eq!(
        read.volume.palette,
        ["minecraft:air", "minecraft:stone"].map(|name| BedrockBlock {
            name: name.to_owned(),
            states: Compound::new(),
            version: Some(target("1.21.60").block_version),
        })
    );
}

#[test]
fn bytes_that_are_not_nbt_are_an_nbt_error() {
    let err = read_mcstructure(&b"\x0a"[..]).expect_err("refused");
    assert!(
        matches!(err, McStructureReadError::Nbt(NbtReadError::Io(_))),
        "got {err:?}"
    );
}
