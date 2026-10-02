//! The streaming structure writers write the file the tree builders
//! describe.
//!
//! `JavaStructure::write_gzip` and `McStructure::write` encode the
//! per-voxel lists from the grid instead of from `build_structure_tag` /
//! `build_mcstructure_tag`'s tree, so the two paths are two encodings of one
//! layout. These tests hold them to the same bytes over every structure the
//! shipped examples lower to, on both editions, and over the shapes the
//! examples do not reach: an array with a different extent on each axis, and
//! empty ones, whose lists must declare `TAG_End`.
//!
//! The write path indexes the grid and declares list lengths without
//! re-checking them, so the last tests hold `prepare_*` to refusing every
//! grid it could not stream: one whose voxel count is past a list's length
//! limit, one shorter than its dims, and a palette string the encoder
//! refuses.

use cairn_lang_core::block_array::{
    BlockArray, BlockArrayIr, BlockState, Dims, Palette, PaletteIndex, lower_to_block_array,
};
use cairn_lang_core::{Edition, lower, parse, resolve};
use cairn_lang_formats::bedrock_structure::{
    BedrockStructureError, ParityNote, build_mcstructure_tag, prepare_mcstructure,
    write_mcstructure,
};
use cairn_lang_formats::data_version::{resolve_bedrock_target, resolve_java_target};
use cairn_lang_formats::java_structure::{
    JavaStructureError, build_structure_tag, prepare_structure, write_compound_gzip,
    write_structure_gzip,
};
use cairn_lang_formats::registry::{RegistryPack, builtin_bedrock, builtin_java};
use cairn_lang_nbt::NbtIoError;

mod common;
use common::examples;

fn lower_latest(source: &str, edition: Edition, pack: &RegistryPack) -> BlockArrayIr {
    let module = parse(source).expect("shipped example parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition));
    lower_to_block_array(
        &ir,
        &resolution,
        Some(&pack.view(Some(&pack.data_versions.latest))),
    )
}

/// Assert two encodings are the same bytes, reporting the lengths and the
/// first offset where they part rather than two megabyte-scale buffers.
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

/// Assert both Java write paths agree with the tree for `array`.
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

/// Assert the Bedrock write path agrees with the tree for `array`, and
/// return the parity notes `prepare_mcstructure` raised.
///
/// The tree builder takes its notes from `prepare_mcstructure`, so
/// comparing the two would compare one function with itself; the sweep
/// below checks the notes are raised at all instead.
fn assert_bedrock_streams_the_tree(label: &str, array: &BlockArray) -> Vec<ParityNote> {
    let target = resolve_bedrock_target("latest").expect("latest");
    let (root, _) = build_mcstructure_tag(array, &target).expect("build");
    let mut tree = Vec::new();
    write_mcstructure(&mut tree, &root).expect("tree write");
    let (prepared, notes) = prepare_mcstructure(array, &target).expect("prepare");
    let mut streamed = Vec::new();
    prepared.write(&mut streamed).expect("stream write");
    assert_same_bytes(label, "streamed .mcstructure", &streamed, &tree);
    notes
}

#[test]
fn every_shipped_structure_streams_the_bytes_its_tree_writes() {
    for (edition, pack) in [
        (Edition::Java, builtin_java()),
        (Edition::Bedrock, builtin_bedrock()),
    ] {
        let mut compared = 0;
        let mut notes = 0;
        for (name, source) in examples() {
            let ir = lower_latest(&source, edition, pack);
            for (scope, array) in &ir.structures {
                let label = format!("{name} {scope} ({edition:?})");
                match edition {
                    Edition::Java => assert_java_streams_the_tree(&label, array),
                    Edition::Bedrock => {
                        notes += assert_bedrock_streams_the_tree(&label, array).len();
                    }
                }
                compared += 1;
            }
        }
        assert!(compared > 0, "{edition:?}: the examples lowered to nothing");
        // A stair with a corner `shape` degrades on Bedrock (`themed-tower`
        // has one), so the sweep passes notes through `prepare_mcstructure`
        // rather than only structures that raise none.
        if matches!(edition, Edition::Bedrock) {
            assert!(
                notes > 0,
                "no shipped example raised a parity note on Bedrock"
            );
        }
    }
}

/// A 2×3×4 array whose voxels all differ from their neighbours along each
/// axis, so a loop nested in the wrong order writes a different sequence.
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
    // Every per-voxel list is empty, so each must declare `TAG_End` the way
    // `List::of_compounds` / `List::of_tags` do. One axis at zero empties
    // the grid as surely as all three, so both shapes are compared.
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
    // 2000 fits an `i32` on every axis, but 2000^3 does not, so the `blocks`
    // list and each `block_indices` layer could not declare their length.
    // Without the check the Java write failed with `LengthOverflow` after
    // the palette had already gone to the writer.
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

#[test]
fn a_grid_shorter_than_its_dims_is_refused_before_a_byte_is_written() {
    // 2x2x2 holds 8 voxels and this grid has 3. Without the check the write
    // indexed past the end of `voxels` and panicked part-way through the
    // per-voxel list, with the file's head already written.
    let array = grid(Dims { x: 2, y: 2, z: 2 }, 3);
    let java = resolve_java_target("latest").expect("latest");
    let err = prepare_structure(&array, &java).expect_err("java prepare");
    assert!(
        matches!(
            err,
            JavaStructureError::VoxelCountMismatch {
                volume: 8,
                voxels: 3,
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
            BedrockStructureError::VoxelCountMismatch {
                volume: 8,
                voxels: 3,
            }
        ),
        "bedrock: got {err:?}",
    );
}

#[test]
fn a_palette_string_the_encoder_refuses_is_named_with_its_entry() {
    // `.mcstructure` writes its palette after both per-voxel layers, so the
    // writer would meet this string only once the whole volume was encoded,
    // and its error named the byte alone.
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

    // Java writes property names and values too, and names the property.
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
