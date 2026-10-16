//! The streaming structure writers write the tree builders' bytes, and `prepare_*` refuses every
//! grid they could not stream.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_formats::bedrock_structure::{
    BedrockStructureError, ParityNote, build_mcstructure_tag, prepare_mcstructure,
    write_mcstructure,
};
use cairn_lang_formats::data_version::{resolve_bedrock_target, resolve_java_target};
use cairn_lang_formats::java_structure::{
    JavaStructureError, build_structure_tag, prepare_structure, write_compound_gzip,
    write_structure_gzip,
};
use cairn_lang_nbt::NbtIoError;

use crate::common::{block_palette, examples, latest, lower_source};

/// Asserts two encodings match, reporting the first offset where they differ.
fn assert_same_bytes(label: &str, what: &str, actual: &[u8], expected: &[u8]) {
    if actual == expected {
        return;
    }
    let first_difference = actual
        .iter()
        .zip(expected)
        .position(|(a, b)| a != b)
        .unwrap_or(actual.len().min(expected.len()));
    panic!(
        "{label}: {what} differs from the tree's: {} bytes against {}, first difference at \
         offset {first_difference}",
        actual.len(),
        expected.len(),
    );
}

/// Asserts both Java write paths agree with the tree for `array`.
fn assert_java_streams_the_tree(label: &str, array: &BlockArray) {
    let target = resolve_java_target("latest").expect("latest");
    let mut tree = Vec::new();
    write_compound_gzip(
        &mut tree,
        &build_structure_tag(array, &target).expect("build"),
    )
    .expect("tree write");
    let mut streamed = Vec::new();
    prepare_structure(array, &target)
        .expect("prepare")
        .write_gzip(&mut streamed)
        .expect("stream write");
    assert_same_bytes(label, "streamed .nbt", &streamed, &tree);
    let mut one_step = Vec::new();
    write_structure_gzip(&mut one_step, array, &target).expect("one-step write");
    assert_same_bytes(label, "write_structure_gzip", &one_step, &tree);
}

/// Asserts the Bedrock write path agrees with the tree and its `block_palette` is a set, returning
/// the parity notes and how many palette entries translation merged.
fn assert_bedrock_streams_the_tree(label: &str, array: &BlockArray) -> (Vec<ParityNote>, usize) {
    let target = resolve_bedrock_target("latest").expect("latest");
    let (root, _) = build_mcstructure_tag(array, &target).expect("build");
    let mut tree = Vec::new();
    write_mcstructure(&mut tree, &root).expect("tree write");
    let (prepared, notes) = prepare_mcstructure(array, &target).expect("prepare");
    let mut streamed = Vec::new();
    prepared.write(&mut streamed).expect("stream write");
    assert_same_bytes(label, "streamed .mcstructure", &streamed, &tree);
    let written = block_palette(&root);
    for (i, entry) in written.iter().enumerate() {
        assert!(
            !written[..i].contains(entry),
            "{label}: block_palette lists {entry:?} twice"
        );
    }
    (notes, array.palette.entries.len() - written.len())
}

#[test]
fn every_shipped_structure_streams_the_bytes_its_tree_writes() {
    for edition in [Edition::Java, Edition::Bedrock] {
        let mut compared = 0;
        let mut notes = 0;
        let mut merged = 0;
        for (name, source) in examples() {
            let ir = lower_source(&source, edition, Some(latest(edition)));
            for (scope, array) in &ir.structures {
                let label = format!("{name} {scope} ({edition:?})");
                match edition {
                    Edition::Java => assert_java_streams_the_tree(&label, array),
                    Edition::Bedrock => {
                        let (raised, folded) = assert_bedrock_streams_the_tree(&label, array);
                        notes += raised.len();
                        merged += folded;
                    }
                }
                compared += 1;
            }
        }
        assert!(compared > 0, "{edition:?}: the examples lowered to nothing");
        // `themed-tower` raises a note, and `roof-hip` folds two corner stairs into one block.
        if matches!(edition, Edition::Bedrock) {
            assert!(
                notes > 0,
                "no shipped example raised a parity note on Bedrock"
            );
            assert!(
                merged > 0,
                "no shipped example merged two palette entries on Bedrock"
            );
        }
    }
}

/// A 2×3×4 array that a loop nested in the wrong order writes differently.
fn asymmetric_array() -> BlockArray {
    let mut palette = Palette::new_with_air();
    let slots = [
        PaletteIndex::AIR,
        palette.intern(BlockState::bare("minecraft:oak_planks")),
        palette.intern(BlockState::bare("minecraft:cobblestone")),
    ];
    let dims = Dims { x: 2, y: 3, z: 4 };
    let mut voxels = vec![PaletteIndex::AIR; dims.volume()];
    for y in 0..dims.y {
        for z in 0..dims.z {
            for x in 0..dims.x {
                let i = dims.index(x, y, z).expect("in dims");
                voxels[i] = slots[i % slots.len()];
            }
        }
    }
    BlockArray {
        dims,
        palette,
        voxels,
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::asymmetric".to_owned(),
    }
}

#[test]
fn an_asymmetric_array_streams_the_bytes_its_tree_writes() {
    let array = asymmetric_array();
    assert_java_streams_the_tree("asymmetric", &array);
    assert_bedrock_streams_the_tree("asymmetric", &array);
}

#[test]
fn an_empty_array_streams_the_bytes_its_tree_writes() {
    for dims in [Dims { x: 0, y: 0, z: 0 }, Dims { x: 2, y: 0, z: 3 }] {
        let array = BlockArray {
            dims,
            palette: Palette::new_with_air(),
            voxels: vec![],
            block_entities: vec![],
            entities: vec![],
            source_scope: "struct::empty".to_owned(),
        };
        let label = format!("empty {}x{}x{}", dims.x, dims.y, dims.z);
        assert_java_streams_the_tree(&label, &array);
        assert_bedrock_streams_the_tree(&label, &array);
    }
}

/// `dims` over `voxels` air voxels.
fn grid(dims: Dims, voxels: usize) -> BlockArray {
    BlockArray {
        dims,
        palette: Palette::new_with_air(),
        voxels: vec![PaletteIndex::AIR; voxels],
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::grid".to_owned(),
    }
}

#[test]
fn a_voxel_count_past_the_list_limit_is_refused_before_a_byte_is_written() {
    // 2000 fits an `i32` on every axis, but 2000^3 does not.
    let array = grid(
        Dims {
            x: 2000,
            y: 2000,
            z: 2000,
        },
        0,
    );
    let java = resolve_java_target("latest").expect("latest");
    let err = prepare_structure(&array, &java).expect_err("java prepare");
    assert!(
        matches!(
            err,
            JavaStructureError::ListTooLong {
                list: "blocks",
                len: 8_000_000_000,
            }
        ),
        "java: got {err:?}",
    );
    let mut written = Vec::new();
    write_structure_gzip(&mut written, &array, &java).expect_err("java one-step write");
    assert!(written.is_empty(), "java wrote {} bytes", written.len());

    let bedrock = resolve_bedrock_target("latest").expect("latest");
    let err = prepare_mcstructure(&array, &bedrock).expect_err("bedrock prepare");
    assert!(
        matches!(
            err,
            BedrockStructureError::ListTooLong {
                list: "block_indices",
                len: 8_000_000_000,
            }
        ),
        "bedrock: got {err:?}",
    );
}

#[test]
fn a_voxel_count_past_usize_is_refused() {
    let side = u32::try_from(i32::MAX).expect("fits");
    let array = grid(
        Dims {
            x: side,
            y: side,
            z: side,
        },
        0,
    );
    let err = prepare_structure(&array, &resolve_java_target("latest").expect("latest"))
        .expect_err("java prepare");
    assert!(
        matches!(err, JavaStructureError::VolumeOverflow { x, .. } if x == side),
        "java: got {err:?}",
    );
    let err = prepare_mcstructure(&array, &resolve_bedrock_target("latest").expect("latest"))
        .expect_err("bedrock prepare");
    assert!(
        matches!(err, BedrockStructureError::VolumeOverflow { x, .. } if x == side),
        "bedrock: got {err:?}",
    );
}

/// A refusal of a grid that disagrees with its dims, as both backends' errors carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridRefusal {
    Mismatch { volume: usize, voxels: usize },
    Overflow { x: u32, y: u32, z: u32 },
}

fn java_grid_refusal(err: &JavaStructureError) -> Option<GridRefusal> {
    match *err {
        JavaStructureError::VoxelCountMismatch { volume, voxels } => {
            Some(GridRefusal::Mismatch { volume, voxels })
        }
        JavaStructureError::VolumeOverflow { x, y, z } => Some(GridRefusal::Overflow { x, y, z }),
        _ => None,
    }
}

fn bedrock_grid_refusal(err: &BedrockStructureError) -> Option<GridRefusal> {
    match *err {
        BedrockStructureError::VoxelCountMismatch { volume, voxels } => {
            Some(GridRefusal::Mismatch { volume, voxels })
        }
        BedrockStructureError::VolumeOverflow { x, y, z } => {
            Some(GridRefusal::Overflow { x, y, z })
        }
        _ => None,
    }
}

#[test]
fn a_grid_that_disagrees_with_its_dims_is_refused_by_every_builder_before_a_byte_is_written() {
    let small = Dims { x: 2, y: 2, z: 2 };
    // Each axis fits an `i32`, and the product overflows `usize`.
    let side = 2_000_000_000;
    let cases = [
        (
            "short",
            grid(small, 3),
            GridRefusal::Mismatch {
                volume: 8,
                voxels: 3,
            },
        ),
        (
            "long",
            grid(small, 100),
            GridRefusal::Mismatch {
                volume: 8,
                voxels: 100,
            },
        ),
        (
            "overflowing",
            grid(
                Dims {
                    x: side,
                    y: side,
                    z: side,
                },
                0,
            ),
            GridRefusal::Overflow {
                x: side,
                y: side,
                z: side,
            },
        ),
    ];
    let java = resolve_java_target("latest").expect("latest");
    let bedrock = resolve_bedrock_target("latest").expect("latest");
    for (label, array, expected) in cases {
        let java_refusals = [
            (
                "prepare_structure",
                prepare_structure(&array, &java).map(drop),
            ),
            (
                "build_structure_tag",
                build_structure_tag(&array, &java).map(drop),
            ),
        ];
        for (builder, result) in java_refusals {
            let err = result.expect_err(builder);
            assert_eq!(
                java_grid_refusal(&err),
                Some(expected),
                "{label} grid, {builder}: got {err:?}",
            );
        }
        let mut written = Vec::new();
        let err = write_structure_gzip(&mut written, &array, &java).expect_err("one-step write");
        assert_eq!(
            java_grid_refusal(&err),
            Some(expected),
            "{label} grid, write_structure_gzip: got {err:?}",
        );
        assert!(
            written.is_empty(),
            "{label} grid: wrote {} bytes",
            written.len()
        );

        let bedrock_refusals = [
            (
                "prepare_mcstructure",
                prepare_mcstructure(&array, &bedrock).map(drop),
            ),
            (
                "build_mcstructure_tag",
                build_mcstructure_tag(&array, &bedrock).map(drop),
            ),
        ];
        for (builder, result) in bedrock_refusals {
            let err = result.expect_err(builder);
            assert_eq!(
                bedrock_grid_refusal(&err),
                Some(expected),
                "{label} grid, {builder}: got {err:?}",
            );
        }
    }
}

#[test]
fn a_palette_string_the_encoder_refuses_is_named_with_its_entry() {
    let mut array = grid(Dims { x: 1, y: 1, z: 1 }, 1);
    array.voxels[0] = array
        .palette
        .intern(BlockState::bare("minecraft:caf\u{e9}"));

    let err = prepare_structure(&array, &resolve_java_target("latest").expect("latest"))
        .expect_err("java prepare");
    assert!(
        matches!(
            &err,
            JavaStructureError::UnencodablePaletteString {
                id,
                field,
                source: NbtIoError::InvalidString { byte: 0xc3, index: 13 },
            } if id == "minecraft:caf\u{e9}" && field == "its id"
        ),
        "java: got {err:?}",
    );
    let err = prepare_mcstructure(&array, &resolve_bedrock_target("latest").expect("latest"))
        .expect_err("bedrock prepare");
    assert!(
        matches!(
            &err,
            BedrockStructureError::UnencodablePaletteString {
                id,
                field,
                source: NbtIoError::InvalidString { byte: 0xc3, index: 13 },
            } if id == "minecraft:caf\u{e9}" && field == "its id"
        ),
        "bedrock: got {err:?}",
    );
    assert_eq!(
        err.to_string(),
        "palette entry `minecraft:caf\u{e9}`: its id cannot be written as NBT: nbt: string \
         contains byte 0xc3 at index 13, not encodable here",
    );

    let mut array = grid(Dims { x: 1, y: 1, z: 1 }, 1);
    let mut state = BlockState::bare("minecraft:oak_stairs");
    state
        .properties
        .insert("facing".to_owned(), "nor\u{f0}".to_owned());
    array.voxels[0] = array.palette.intern(state);
    let err = prepare_structure(&array, &resolve_java_target("latest").expect("latest"))
        .expect_err("java prepare");
    assert!(
        matches!(
            &err,
            JavaStructureError::UnencodablePaletteString { field, .. }
                if field == "the value of property `facing`"
        ),
        "java property: got {err:?}",
    );
}
