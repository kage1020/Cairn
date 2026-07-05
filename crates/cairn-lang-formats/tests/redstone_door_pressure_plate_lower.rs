//! Integration coverage for `redstone-door.crn` under `pressure_plate`
//! lowering. Pins the palette (air, floor material, wall material,
//! `oak_pressure_plate`), the two plate voxels, and the invariant that
//! no `W_DEFERRED_MEMBER` fires on either `pressure_plate` line. Once
//! `circuit region=…` and the selector-form `door[id=…] opened_by=…`
//! actuator patch grow their own lowering passes each will shave one
//! entry off the remaining deferred stream this file measures.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::{lower, parse, resolve};
use cairn_lang_formats::registry::builtin_java;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn lower_redstone_door() -> BlockArrayIr {
    let source = std::fs::read_to_string(examples_dir().join("redstone-door.crn"))
        .expect("redstone-door.crn readable");
    let module = parse(&source).expect("parse redstone-door");
    let ir = lower(&module);
    let resolution = resolve(&ir);
    let pack = builtin_java();
    lower_to_block_array(&ir, &resolution, Some(&pack.materials))
}

#[test]
fn redstone_door_lowers_to_a_flat_gatehouse_volume() {
    // struct gatehouse size=7x5, walls height=3, no roof, no overhang →
    // dims.x = 7, dims.z = 5, dims.y = floor(1) + wall_top(3) = 4.
    let out = lower_redstone_door();
    let ba = out
        .structures
        .get("struct::gatehouse")
        .expect("gatehouse struct present");
    assert_eq!(ba.dims.x, 7);
    assert_eq!(ba.dims.y, 4);
    assert_eq!(ba.dims.z, 5);
}

#[test]
fn redstone_door_palette_contains_oak_pressure_plate() {
    let out = lower_redstone_door();
    let ba = out.structures.get("struct::gatehouse").unwrap();
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert!(
        ids.contains(&"minecraft:oak_pressure_plate"),
        "palette missing oak_pressure_plate; got {ids:?}",
    );
}

#[test]
fn redstone_door_outside_plate_falls_back_to_wall_column_without_overhang() {
    // `pressure_plate id=plate at=front.outside offset=0 y=0` — the
    // shift-outward step lands at z=dims.z (out of range) because the
    // struct has no overhang. The `y=0` foundation fallback then paints
    // on the wall's own column at (x=0, y=0, z=4). The main-line
    // "shift into overhang column" semantics is covered by a unit test
    // in `cairn-lang-core::block_array::lower::tests`.
    let out = lower_redstone_door();
    let ba = out.structures.get("struct::gatehouse").unwrap();
    let plate_idx = u16::try_from(
        ba.palette
            .entries
            .iter()
            .position(|s| s.id == "minecraft:oak_pressure_plate")
            .expect("oak_pressure_plate palette entry present"),
    )
    .expect("palette index fits in u16");
    let i = ba
        .dims
        .index(0, 0, 4)
        .expect("front-wall corner within bounds");
    assert_eq!(ba.voxels[i].0, plate_idx);
}

#[test]
fn redstone_door_inside_plate_paints_one_voxel_inside_the_front_wall() {
    // `pressure_plate id=inner at=inside.front offset=0 y=0` — shift-inward
    // from (0, 0, 4) is (0, 0, 3), well within dims.
    let out = lower_redstone_door();
    let ba = out.structures.get("struct::gatehouse").unwrap();
    let plate_idx = u16::try_from(
        ba.palette
            .entries
            .iter()
            .position(|s| s.id == "minecraft:oak_pressure_plate")
            .expect("oak_pressure_plate palette entry present"),
    )
    .expect("palette index fits in u16");
    let i = ba.dims.index(0, 0, 3).expect("interior row within bounds");
    assert_eq!(ba.voxels[i].0, plate_idx);
}

#[test]
fn redstone_door_pressure_plate_lines_emit_no_deferred_warnings() {
    // Two plates → zero `W_DEFERRED_MEMBER` diagnostics whose primary
    // message names the `pressure_plate` role. Other deferred warnings
    // (`circuit`, the selector-form actuator patch) are still expected
    // and are covered by their own tests once their lowering rules are
    // spec'd.
    let out = lower_redstone_door();
    let plate_deferred = out
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, DiagnosticCode::DeferredMember))
        .filter(|d| d.primary.contains("pressure_plate"))
        .collect::<Vec<_>>();
    assert!(
        plate_deferred.is_empty(),
        "pressure_plate must lower without deferred members; got {} — first: {}",
        plate_deferred.len(),
        plate_deferred
            .first()
            .map_or("<none>", |d| d.primary.as_str()),
    );
}
