//! The streaming structure writers write the file the tree builders
//! describe.
//!
//! `JavaStructure::write_gzip` and `McStructure::write` encode the
//! per-voxel lists from the grid instead of from `build_structure_tag` /
//! `build_mcstructure_tag`'s tree, so the two paths are two encodings of one
//! layout. These tests hold them to the same bytes over every structure the
//! shipped examples lower to, on both editions, and over the shapes the
//! examples do not reach: an array with a different extent on each axis, and
//! an empty one, whose lists must declare `TAG_End`.

use cairn_lang_core::block_array::{
    BlockArray, BlockArrayIr, BlockState, Dims, Palette, PaletteIndex, lower_to_block_array,
};
use cairn_lang_core::{Edition, lower, parse, resolve};
use cairn_lang_formats::bedrock_structure::{
    build_mcstructure_tag, prepare_mcstructure, write_mcstructure,
};
use cairn_lang_formats::data_version::{resolve_bedrock_target, resolve_java_target};
use cairn_lang_formats::java_structure::{
    build_structure_tag, prepare_structure, write_compound_gzip, write_structure_gzip,
};
use cairn_lang_formats::registry::{RegistryPack, builtin_bedrock, builtin_java};

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
    assert!(
        streamed == tree,
        "{label}: streamed .nbt differs from the tree's"
    );
    let mut one_step = Vec::new();
    write_structure_gzip(&mut one_step, array, &target).expect("one-step write");
    assert!(
        one_step == tree,
        "{label}: write_structure_gzip differs from the tree's"
    );
}

/// Assert the Bedrock write path agrees with the tree for `array`,
/// parity notes included.
fn assert_bedrock_streams_the_tree(label: &str, array: &BlockArray) {
    let target = resolve_bedrock_target("latest").expect("latest");
    let (root, tree_notes) = build_mcstructure_tag(array, &target).expect("build");
    let mut tree = Vec::new();
    write_mcstructure(&mut tree, &root).expect("tree write");
    let (prepared, notes) = prepare_mcstructure(array, &target).expect("prepare");
    let mut streamed = Vec::new();
    prepared.write(&mut streamed).expect("stream write");
    assert!(
        streamed == tree,
        "{label}: streamed .mcstructure differs from the tree's"
    );
    assert_eq!(notes, tree_notes, "{label}: parity notes");
}

#[test]
fn every_shipped_structure_streams_the_bytes_its_tree_writes() {
    for (edition, pack) in [
        (Edition::Java, builtin_java()),
        (Edition::Bedrock, builtin_bedrock()),
    ] {
        let mut compared = 0;
        for (name, source) in examples() {
            let ir = lower_latest(&source, edition, pack);
            for (scope, array) in &ir.structures {
                let label = format!("{name} {scope} ({edition:?})");
                match edition {
                    Edition::Java => assert_java_streams_the_tree(&label, array),
                    Edition::Bedrock => assert_bedrock_streams_the_tree(&label, array),
                }
                compared += 1;
            }
        }
        assert!(compared > 0, "{edition:?}: the examples lowered to nothing");
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
    // `List::of_compounds` / `List::of_tags` do; the writer refuses an
    // empty list claiming any other element type.
    let array = BlockArray {
        dims: Dims { x: 0, y: 0, z: 0 },
        palette: Palette::new_with_air(),
        voxels: vec![],
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::empty".to_owned(),
    };
    assert_java_streams_the_tree("empty", &array);
    assert_bedrock_streams_the_tree("empty", &array);
}
