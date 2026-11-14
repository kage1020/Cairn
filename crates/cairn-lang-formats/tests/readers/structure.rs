//! `read_structure` over what the `.nbt` writer wrote, and over files only the game or another
//! tool would write.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, Dims, PaletteIndex};
use cairn_lang_formats::data_version::{JavaTarget, resolve_java_target};
use cairn_lang_formats::java_structure::{
    StoredStructure, StructureReadError, prepare_structure, read_structure,
};
use cairn_lang_formats::volume::{BlockVolume, StoredBlockEntity};
use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{NbtReadError, write_java_gzip};

use crate::common::{comparator_array, examples, latest, lower_example, volume_of};

fn target(version: &str) -> JavaTarget {
    resolve_java_target(version).expect("a targetable version")
}

fn compound<const N: usize>(entries: [(&str, Tag); N]) -> Compound {
    let mut compound = Compound::new();
    for (name, tag) in entries {
        compound.insert(name, tag);
    }
    compound
}

fn named(id: &str) -> Compound {
    compound([("Name", Tag::String(id.to_owned()))])
}

fn block(state: i32, pos: [i32; 3]) -> Compound {
    compound([
        ("state", Tag::Int(state)),
        ("pos", Tag::List(List::of_ints(pos))),
    ])
}

/// A structure root of `size` with `palette` and `blocks`, saved at 1.21.4.
fn structure(size: [i32; 3], palette: Vec<Compound>, blocks: Vec<Compound>) -> Compound {
    compound([
        ("size", Tag::List(List::of_ints(size))),
        ("palette", Tag::List(List::of_compounds(palette))),
        ("blocks", Tag::List(List::of_compounds(blocks))),
        ("entities", Tag::List(List::empty())),
        ("DataVersion", Tag::Int(4189)),
    ])
}

fn bytes(root: &Compound) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_java_gzip(&mut bytes, "", root).expect("the root writes");
    bytes
}

fn read(root: &Compound) -> StoredStructure {
    read_structure(bytes(root).as_slice()).expect("the file reads")
}

fn read_error(root: &Compound) -> StructureReadError {
    read_structure(bytes(root).as_slice()).expect_err("the file is refused")
}

#[test]
fn a_struct_reads_back_as_its_block_array_with_its_block_entity() {
    let array = comparator_array("minecraft:comparator");
    let mut bytes = Vec::new();
    prepare_structure(&array, &target("1.20.4"))
        .expect("the array prepares")
        .write_gzip(&mut bytes)
        .expect("the file writes");
    let read = read_structure(bytes.as_slice()).expect("the file reads back");
    assert_eq!(
        read,
        StoredStructure {
            data_version: 3700,
            volume: volume_of(&array),
            void_cells: 0,
            palettes: 1,
            entities: vec![],
        }
    );
}

#[test]
fn every_shipped_example_reads_back_as_its_block_arrays_under_either_palette_key_pair() {
    for version in ["1.20.4", latest(Edition::Java)] {
        for (name, _) in examples() {
            let ir = lower_example(&name, Edition::Java, None);
            for array in ir.structures.values() {
                let mut bytes = Vec::new();
                prepare_structure(array, &target(version))
                    .expect("the array prepares")
                    .write_gzip(&mut bytes)
                    .expect("the file writes");
                let read = read_structure(bytes.as_slice()).expect("the file reads back");
                assert_eq!(read.volume, volume_of(array), "{name} at {version}");
            }
        }
    }
}

#[test]
fn a_cell_blocks_lists_nothing_at_reads_as_air_added_to_a_palette_without_it() {
    let stone = || named("minecraft:stone");
    let mut chest = block(0, [2, 0, 0]);
    chest.insert(
        "nbt",
        Tag::Compound(compound([
            ("id", Tag::String("minecraft:chest".to_owned())),
            ("Lock", Tag::String(String::new())),
        ])),
    );
    let root = structure([3, 1, 1], vec![stone()], vec![chest, block(0, [0, 0, 0])]);
    assert_eq!(
        read(&root),
        StoredStructure {
            data_version: 4189,
            volume: BlockVolume {
                dims: Dims { x: 3, y: 1, z: 1 },
                palette: vec![BlockState::bare("minecraft:stone"), BlockState::air()],
                voxels: vec![PaletteIndex(0), PaletteIndex(1), PaletteIndex(0)],
                block_entities: vec![StoredBlockEntity {
                    pos: (2, 0, 0),
                    id: "minecraft:chest".to_owned(),
                    data: compound([("Lock", Tag::String(String::new()))]),
                }],
            },
            void_cells: 1,
            palettes: 1,
            entities: vec![],
        }
    );
}

#[test]
fn of_several_palettes_the_first_is_read_and_the_rest_counted() {
    let mut root = structure([1, 1, 1], vec![], vec![block(0, [0, 0, 0])]);
    root.entries.shift_remove("palette");
    let palette = |id: &str| Tag::List(List::of_compounds(vec![named(id)]));
    root.insert(
        "palettes",
        Tag::List(List::of_tags(
            9,
            vec![
                palette("minecraft:white_wool"),
                palette("minecraft:red_wool"),
                palette("minecraft:blue_wool"),
            ],
        )),
    );
    let read = read(&root);
    assert_eq!(
        (read.volume.palette, read.palettes),
        (vec![BlockState::bare("minecraft:white_wool")], 3)
    );
}

#[test]
fn a_block_entry_the_palette_or_size_cannot_place_is_refused() {
    let stone = || vec![named("minecraft:stone")];
    let refused: Vec<String> = [
        structure([2, 1, 1], stone(), vec![block(1, [0, 0, 0])]),
        structure([2, 1, 1], stone(), vec![block(0, [2, 0, 0])]),
        structure([2, 1, 1], stone(), vec![block(0, [-1, 0, 0])]),
        structure(
            [2, 1, 1],
            stone(),
            vec![block(0, [1, 0, 0]), block(0, [1, 0, 0])],
        ),
        structure([300, 300, 300], stone(), vec![]),
    ]
    .iter()
    .map(|root| read_error(root).to_string())
    .collect();
    assert_eq!(
        refused,
        [
            "`blocks[0]` has state 1, past the palette's 1 entries",
            "`blocks[0]` is at 2,0,0, outside the structure's 2x1x1",
            "`blocks[0]` is at -1,0,0, outside the structure's 2x1x1",
            "`blocks[1]` is at 1,0,0, where an earlier entry already is",
            "the structure holds 27000000 voxels, past the 16777216 a block array may hold",
        ]
    );
}

#[test]
fn a_field_of_the_wrong_shape_is_refused_by_its_path() {
    let base = || {
        structure(
            [1, 1, 1],
            vec![named("minecraft:stone")],
            vec![block(0, [0, 0, 0])],
        )
    };
    let mut no_data_version = base();
    no_data_version.entries.shift_remove("DataVersion");
    let mut short_size = base();
    short_size.insert("size", Tag::List(List::of_ints([1, 1])));
    let mut no_palette = base();
    no_palette.entries.shift_remove("palette");
    let unnamed = structure([1, 1, 1], vec![Compound::new()], vec![block(0, [0, 0, 0])]);
    let mut no_blocks = base();
    no_blocks.entries.shift_remove("blocks");
    let mut stateless = block(0, [0, 0, 0]);
    stateless.entries.shift_remove("state");
    let stateless = structure([1, 1, 1], vec![named("minecraft:stone")], vec![stateless]);
    let mut short_pos = block(0, [0, 0, 0]);
    short_pos.insert("pos", Tag::List(List::of_ints([0, 0])));
    let short_pos = structure([1, 1, 1], vec![named("minecraft:stone")], vec![short_pos]);
    let mut anonymous = block(0, [0, 0, 0]);
    anonymous.insert("nbt", Tag::Compound(Compound::new()));
    let anonymous = structure([1, 1, 1], vec![named("minecraft:stone")], vec![anonymous]);
    let refused: Vec<(String, &str)> = [
        no_data_version,
        short_size,
        no_palette,
        unnamed,
        no_blocks,
        stateless,
        short_pos,
        anonymous,
    ]
    .iter()
    .map(|root| match read_error(root) {
        StructureReadError::Malformed { path, expected } => (path, expected),
        other => panic!("not malformed: {other:?}"),
    })
    .collect();
    assert_eq!(
        refused,
        [
            ("DataVersion".to_owned(), "an int"),
            ("size".to_owned(), "a list of three ints, none negative"),
            ("palette".to_owned(), "a list of block states"),
            (
                "palette".to_owned(),
                "a list of block states, each a `Name` and its `Properties`"
            ),
            ("blocks".to_owned(), "a list of compounds"),
            ("blocks[0].state".to_owned(), "an int"),
            ("blocks[0].pos".to_owned(), "a list of three ints"),
            ("blocks[0].nbt.id".to_owned(), "the block entity's id"),
        ]
    );
}

#[test]
fn bytes_that_are_not_gzip_are_an_nbt_error() {
    let err = read_structure(&b"not a structure"[..]).expect_err("refused");
    assert!(
        matches!(err, StructureReadError::Nbt(NbtReadError::Io(_))),
        "got {err:?}"
    );
}
