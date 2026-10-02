//! The structure writers hold a bounded amount of memory whatever the
//! structure's volume.
//!
//! `MAX_STRUCTURE_VOLUME` is sized against the block-array IR, at two bytes
//! a voxel; a writer that builds a tag per voxel before writing needs
//! hundreds of times that, so a structure inside the bound could still
//! exhaust memory during `compile`. This binary counts the heap the check
//! and the write allocate at two volumes and asserts the peak does not grow
//! with the volume. The tree builders, measured the same way, are the
//! control: they show the harness sees a per-voxel allocation when there is
//! one.
//!
//! One test in its own binary: the counter is process-wide, and a second
//! test on another thread would allocate into the measurement.

use cairn_lang_core::block_array::{BlockArray, BlockState, Dims, Palette, PaletteIndex};
use cairn_lang_formats::bedrock_structure::{build_mcstructure_tag, prepare_mcstructure};
use cairn_lang_formats::data_version::{resolve_bedrock_target, resolve_java_target};
use cairn_lang_formats::java_structure::{build_structure_tag, prepare_structure};
use peak_alloc::PeakAlloc;

// A dependency rather than a local `GlobalAlloc`: implementing one takes
// `unsafe`, which the workspace denies outside the tree-sitter binding.
#[global_allocator]
static ALLOCATOR: PeakAlloc = PeakAlloc;

/// The most bytes `f` holds live at once beyond what was live before it.
///
/// The peak is reset before `before` is read: the reset pins the peak at
/// the live total, so anything freed or allocated between the two reads
/// leaves the peak at or above `before` and the subtraction cannot wrap.
fn peak_during(f: impl FnOnce()) -> usize {
    ALLOCATOR.reset_peak_usage();
    let before = ALLOCATOR.current_usage();
    f();
    ALLOCATOR.peak_usage() - before
}

/// A cube of side `side`, alternating two blocks so every voxel is written.
fn cube(side: u32) -> BlockArray {
    let mut palette = Palette::new_with_air();
    let planks = palette.intern(BlockState::bare("minecraft:oak_planks"));
    let dims = Dims {
        x: side,
        y: side,
        z: side,
    };
    let voxels = (0..dims.volume())
        .map(|i| {
            if i % 2 == 0 {
                planks
            } else {
                PaletteIndex::AIR
            }
        })
        .collect();
    BlockArray {
        dims,
        palette,
        voxels,
        block_entities: vec![],
        entities: vec![],
        source_scope: "struct::cube".to_owned(),
    }
}

/// Headroom between the two volumes' peaks. The writers' fixed costs
/// (the gzip encoder's window, a palette, a formatted error) do not depend
/// on the volume, so the two peaks should match to within noise; 64 KiB
/// is far below the smaller of the two controls' growth.
const SLACK: usize = 64 * 1024;

#[test]
fn writing_a_structure_does_not_hold_memory_per_voxel() {
    let java = resolve_java_target("latest").expect("latest");
    let bedrock = resolve_bedrock_target("latest").expect("latest");
    let small = cube(8);
    let large = cube(48);
    let grown = large.dims.volume() - small.dims.volume();

    let java_write = |array: &BlockArray| {
        peak_during(|| {
            prepare_structure(array, &java)
                .expect("prepare")
                .write_gzip(&mut std::io::sink())
                .expect("write");
        })
    };
    let bedrock_write = |array: &BlockArray| {
        peak_during(|| {
            let (prepared, _notes) = prepare_mcstructure(array, &bedrock).expect("prepare");
            prepared.write(&mut std::io::sink()).expect("write");
        })
    };
    let java_tree = |array: &BlockArray| {
        peak_during(|| drop(build_structure_tag(array, &java).expect("build")))
    };
    let bedrock_tree = |array: &BlockArray| {
        peak_during(|| drop(build_mcstructure_tag(array, &bedrock).expect("build")))
    };

    // Controls: the trees grow by at least a byte per added voxel, which is
    // what a writer that builds one would show.
    for (label, tree) in [
        ("java tree", &java_tree as &dyn Fn(&BlockArray) -> usize),
        ("bedrock tree", &bedrock_tree),
    ] {
        let (small_peak, large_peak) = (tree(&small), tree(&large));
        assert!(
            large_peak > small_peak + grown.max(SLACK),
            "{label}: {small_peak} -> {large_peak} bytes did not grow with the volume, so the \
             harness cannot see a per-voxel allocation",
        );
    }

    for (label, write) in [
        ("java write", &java_write as &dyn Fn(&BlockArray) -> usize),
        ("bedrock write", &bedrock_write),
    ] {
        let (small_peak, large_peak) = (write(&small), write(&large));
        // Both writers allocate something (the gzip encoder's window, the
        // palette), so a zero means the allocation moved out of the
        // counter's view, for example into a C allocator behind a
        // different flate2 backend, and the bound below would hold
        // vacuously.
        assert!(
            small_peak > 0,
            "{label}: the counter saw no allocation, so it cannot bound one",
        );
        assert!(
            large_peak <= small_peak + SLACK,
            "{label}: peak grew from {small_peak} to {large_peak} bytes for {grown} more voxels",
        );
    }
}
