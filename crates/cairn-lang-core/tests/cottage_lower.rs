//! End-to-end check that `cottage.crn` lowers to the expected voxel volume.
//!
//! This is the example the M2 milestone gates on. As of M2-PR6 cottage.crn
//! exercises every voxel role the milestone ships (floor, walls, door,
//! window, gable roof with overhang), so the integration test pins the
//! inflated dims, the door/window/roof voxel placements, the full palette,
//! and the *zero* deferred-warning bar. A regression in any of those —
//! lowering arithmetic, phase ordering, overhang shift, or roof
//! orientation — fails immediately.

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
fn cottage_lowers_to_overhang_inflated_volume() {
    // size=9x7 walls height=4 roof kind=gable overhang=1.
    // dims.x = 9 + 2*1 = 11. dims.z = 7 + 2*1 = 9.
    // ridge span (short axis of inflated bbox) = min(11, 9) = 9.
    // ridge_extra = ceil(9/2) = 5 → dims.y = 1 + 4 + 5 = 10.
    let out = lower_example("cottage.crn");
    let ba = out
        .structures
        .get("struct::cottage")
        .expect("cottage struct present");
    assert_eq!(ba.dims.x, 11);
    assert_eq!(ba.dims.y, 10);
    assert_eq!(ba.dims.z, 9);
}

#[test]
fn cottage_palette_contains_all_five_materials() {
    let out = lower_example("cottage.crn");
    let ba = out.structures.get("struct::cottage").unwrap();
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"minecraft:air"));
    assert!(ids.contains(&"minecraft:oak_planks"));
    assert!(ids.contains(&"minecraft:cobblestone"));
    assert!(ids.contains(&"minecraft:spruce_stairs"));
    assert!(ids.contains(&"minecraft:glass_pane"));
    // spruce_stairs occurs multiple times because each (facing, half)
    // pair is its own palette entry.
    let stair_entries = ba
        .palette
        .entries
        .iter()
        .filter(|s| s.id == "minecraft:spruce_stairs")
        .count();
    assert!(
        stair_entries >= 3,
        "expected at least 3 spruce_stairs blockstates (north slope, south slope, apex), got {stair_entries}",
    );
}

#[test]
fn cottage_floor_plane_fills_interior_only() {
    let out = lower_example("cottage.crn");
    let ba = out.structures.get("struct::cottage").unwrap();
    // Interior x∈[1, 9], z∈[1, 7] (overhang=1). 9 * 7 = 63 oak_planks cells.
    let mut interior_non_air = 0;
    let mut overhang_non_air = 0;
    for z in 0..ba.dims.z {
        for x in 0..ba.dims.x {
            let i = ba.dims.index(x, 0, z).expect("in-range coord");
            if ba.voxels[i].0 == 0 {
                continue;
            }
            let in_interior = (1..=9).contains(&x) && (1..=7).contains(&z);
            if in_interior {
                interior_non_air += 1;
            } else {
                overhang_non_air += 1;
            }
        }
    }
    assert_eq!(interior_non_air, 9 * 7);
    assert_eq!(
        overhang_non_air, 0,
        "overhang ring of the floor plane must stay air",
    );
}

#[test]
fn cottage_wall_ring_at_y1_has_perimeter_minus_door() {
    let out = lower_example("cottage.crn");
    let ba = out.structures.get("struct::cottage").unwrap();
    // Walls form a 9x7 outline at x∈[1,9], z∈[1,7]. Perimeter cells:
    // 2 * (9 + 7) - 4 = 28. The door at side=front (z=7) at=center
    // (x=5) carves one cell at y=1, leaving 27.
    let mut wall_cells = 0;
    for z in 0..ba.dims.z {
        for x in 0..ba.dims.x {
            let i = ba.dims.index(x, 1, z).expect("in-range coord");
            let pi = ba.voxels[i];
            if ba.palette.entries[usize::from(pi.0)].id == "minecraft:cobblestone" {
                wall_cells += 1;
            }
        }
    }
    assert_eq!(wall_cells, 28 - 1);
}

#[test]
fn cottage_door_carves_front_opening() {
    let out = lower_example("cottage.crn");
    let ba = out.structures.get("struct::cottage").unwrap();
    // Front wall at z = overhang + size.h - 1 = 7. Center x = 1 + 4 = 5.
    let i1 = ba.dims.index(5, 1, 7).unwrap();
    let i2 = ba.dims.index(5, 2, 7).unwrap();
    assert_eq!(ba.voxels[i1].0, 0, "door y=1 should be air");
    assert_eq!(ba.voxels[i2].0, 0, "door y=2 should be air");
}

#[test]
fn cottage_window_pair_lowered_to_glass_panes() {
    let out = lower_example("cottage.crn");
    let ba = out.structures.get("struct::cottage").unwrap();
    // Front wall at z=7. Primary x∈[3,5), y∈[2,4) (offset=2 shifted by
    // overhang=1, size=2x2). Mirror at x∈[6,8) for sym=true.
    for x in [3, 4, 6, 7] {
        for y in [2, 3] {
            let i = ba.dims.index(x, y, 7).unwrap();
            let pi = ba.voxels[i];
            assert_eq!(
                ba.palette.entries[usize::from(pi.0)].id,
                "minecraft:glass_pane",
                "expected glass at ({x},{y},7)",
            );
        }
    }
}

#[test]
fn cottage_roof_palette_includes_stairs_with_facing() {
    let out = lower_example("cottage.crn");
    let ba = out.structures.get("struct::cottage").unwrap();
    // At least one entry with facing=south + half=bottom (north slope) and
    // one with facing=north + half=bottom (south slope) and one half=top
    // (apex cap).
    let mut north_slope = false;
    let mut south_slope = false;
    let mut apex = false;
    for entry in &ba.palette.entries {
        if entry.id != "minecraft:spruce_stairs" {
            continue;
        }
        let facing = entry.properties.get("facing").map(String::as_str);
        let half = entry.properties.get("half").map(String::as_str);
        match (facing, half) {
            (Some("south"), Some("bottom")) => north_slope = true,
            (Some("north"), Some("bottom")) => south_slope = true,
            (_, Some("top")) => apex = true,
            _ => {}
        }
    }
    assert!(north_slope, "missing north-slope stair entry");
    assert!(south_slope, "missing south-slope stair entry");
    assert!(apex, "missing apex stair entry");
}

#[test]
fn cottage_emits_zero_deferred_warnings() {
    let out = lower_example("cottage.crn");
    let deferred = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .count();
    assert_eq!(
        deferred, 0,
        "M2-PR6 cottage.crn should voxelise without W_DEFERRED_MEMBER warnings, diagnostics: {:?}",
        out.diagnostics,
    );
}
