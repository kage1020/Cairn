//! End-to-end check that `cottage.crn` lowers to the expected voxel volume.
//!
//! This is the example the M2 milestone gates on: floor + walls (plus a few
//! deferred members the lowering pass should warn about, not fail on). The
//! integration test pins the dims, palette contents, and voxel counts so a
//! future refactor that quietly changes the lowering arithmetic is caught
//! immediately.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::{lower, parse, resolve};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn lower_example(name: &str) -> BlockArrayIr {
    let source =
        std::fs::read_to_string(examples_dir().join(name)).expect("example file must be readable");
    let module = parse(&source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir);
    lower_to_block_array(&ir, &resolution)
}

#[test]
fn cottage_lowers_to_nine_by_five_by_seven_volume() {
    let out = lower_example("cottage.crn");
    let ba = out
        .structures
        .get("struct::cottage")
        .expect("cottage struct present");
    assert_eq!(ba.dims.x, 9);
    assert_eq!(ba.dims.y, 5); // floor (y=0) + walls height 4 (y=1..=4)
    assert_eq!(ba.dims.z, 7);
}

#[test]
fn cottage_palette_includes_air_floor_and_wall_materials() {
    let out = lower_example("cottage.crn");
    let ba = out.structures.get("struct::cottage").unwrap();
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"minecraft:air"));
    assert!(ids.contains(&"minecraft:oak_planks"));
    assert!(ids.contains(&"minecraft:cobblestone"));
}

#[test]
fn cottage_floor_plane_is_fully_filled() {
    let out = lower_example("cottage.crn");
    let ba = out.structures.get("struct::cottage").unwrap();
    let mut non_air = 0;
    for z in 0..ba.dims.z {
        for x in 0..ba.dims.x {
            let i = ba.dims.index(x, 0, z).expect("in-range coord");
            if ba.voxels[i].0 != 0 {
                non_air += 1;
            }
        }
    }
    assert_eq!(non_air, (ba.dims.x * ba.dims.z) as usize);
}

#[test]
fn cottage_wall_ring_at_y1_has_perimeter_only() {
    let out = lower_example("cottage.crn");
    let ba = out.structures.get("struct::cottage").unwrap();
    let mut non_air = 0;
    for z in 0..ba.dims.z {
        for x in 0..ba.dims.x {
            let i = ba.dims.index(x, 1, z).expect("in-range coord");
            if ba.voxels[i].0 != 0 {
                non_air += 1;
            }
        }
    }
    // 2 * (W + Z) - 4 corners counted once, not twice.
    let expected = 2 * (ba.dims.x + ba.dims.z) - 4;
    assert_eq!(non_air, expected as usize);
}

#[test]
fn cottage_emits_one_deferred_warning_per_unhandled_member() {
    let out = lower_example("cottage.crn");
    let deferred = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .count();
    // Cottage members: floor + walls (lowered) + door + window + roof
    // (deferred) = 3 warnings.
    assert_eq!(deferred, 3, "diagnostics: {:?}", out.diagnostics);
}
