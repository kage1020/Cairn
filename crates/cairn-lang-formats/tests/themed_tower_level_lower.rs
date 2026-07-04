//! Integration coverage for `themed-tower.crn` after level-block lowering
//! and roof/stair theme-honouring landed together.
//!
//! Lives in `cairn-lang-formats` because the assertions need the built-in
//! registry pack (`builtin_java`) to resolve the tower's abstract material
//! tokens — `cairn-lang-core` cannot depend on `formats` without
//! introducing a cycle. Pins the dim math (with per-level walls extending
//! the tower to two stories), the palette (four resolved theme slots plus
//! air), the eave stair band, and the `repeat=/step=` arrow-slit window
//! pattern. Zero `W_DEFERRED_MEMBER` is the top-level M3 contract this
//! file replaces the older `c14b` "at least one deferred" pin with.

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

fn lower_themed_tower() -> BlockArrayIr {
    let source = std::fs::read_to_string(examples_dir().join("themed-tower.crn"))
        .expect("themed-tower.crn readable");
    let module = parse(&source).expect("parse themed-tower");
    let ir = lower(&module);
    let resolution = resolve(&ir);
    let pack = builtin_java();
    lower_to_block_array(&ir, &resolution, Some(&pack.materials))
}

#[test]
fn themed_tower_lowers_to_two_story_volume() {
    // size=11x9, roof overhang=1 → dims.x = 11+2 = 13, dims.z = 9+2 = 11.
    // Struct walls height=5, level y=5 walls height=4 → max_wall_top =
    // max(5, 5+4) = 9. Gable ridge span = min(13, 11) = 11, extra height =
    // ceil(11/2) = 6. dims.y = 1 + 9 + 6 = 16.
    let out = lower_themed_tower();
    let ba = out
        .structures
        .get("struct::keep")
        .expect("keep struct present");
    assert_eq!(ba.dims.x, 13);
    assert_eq!(ba.dims.y, 16);
    assert_eq!(ba.dims.z, 11);
}

#[test]
fn themed_tower_palette_carries_every_resolved_theme_slot() {
    // themed-tower's `keep_dark` theme lifts four abstract tokens through
    // the built-in registry pack:
    //   @floor.wood.broadleaf → oak_planks
    //   @wall.stone.cobble    → cobblestone
    //   @wood.dark            → dark_oak_planks   (used by trim window)
    //   @roof.dark_wood       → dark_oak_stairs   (roof + eave stair)
    // Level-scoped members must reach the palette for this to hold, so a
    // regression in flattening or in the mat_slot-honouring stair/roof
    // change will drop one of these ids and fail the test.
    let out = lower_themed_tower();
    let ba = out.structures.get("struct::keep").unwrap();
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    for expected in [
        "minecraft:air",
        "minecraft:oak_planks",
        "minecraft:cobblestone",
        "minecraft:dark_oak_planks",
        "minecraft:dark_oak_stairs",
    ] {
        assert!(
            ids.contains(&expected),
            "themed-tower palette missing `{expected}`; got {ids:?}",
        );
    }
}

#[test]
fn themed_tower_upper_walls_paint_at_second_floor_height() {
    // level y=5 walls id=upper height=4 → the upper wall ring lives at
    // y=6..=9. Pick a corner along the ring that must be cobblestone
    // (@wall.stone.cobble). Interior x∈[1, 11], z∈[1, 9] (overhang=1),
    // so the south-west corner is (1, y, 1).
    let out = lower_themed_tower();
    let ba = out.structures.get("struct::keep").unwrap();
    let cobble_idx = ba
        .palette
        .entries
        .iter()
        .position(|s| s.id == "minecraft:cobblestone")
        .expect("cobblestone palette entry present") as u16;
    for y in 6..=9 {
        let i = ba.dims.index(1, y, 1).expect("index within bounds");
        assert_eq!(
            ba.voxels[i].0, cobble_idx,
            "second-floor SW corner at y={y} should be cobblestone",
        );
    }
}

#[test]
fn themed_tower_eave_band_paints_outside_the_front_wall() {
    // level y=5 stair id=eave side=front half=top facing=out shape=outer_left
    // → 11 dark_oak_stairs blocks at world y=5, in the overhang row one
    // voxel outside the +z (front) wall (z = overhang + interior_h = 10).
    // `shape=outer_left` and `facing=south` are baked into the palette
    // entry, so we count matching entries by their properties.
    let out = lower_themed_tower();
    let ba = out.structures.get("struct::keep").unwrap();
    let mut count = 0;
    for x in 1..=11 {
        let i = ba.dims.index(x, 5, 10).expect("eave column within bounds");
        let idx = ba.voxels[i].0 as usize;
        let state = &ba.palette.entries[idx];
        if state.id == "minecraft:dark_oak_stairs"
            && state.properties.get("half").map(String::as_str) == Some("top")
            && state.properties.get("shape").map(String::as_str) == Some("outer_left")
            && state.properties.get("facing").map(String::as_str) == Some("south")
        {
            count += 1;
        }
    }
    assert_eq!(
        count, 11,
        "expected 11 eave stair blocks along the front wall's overhang row",
    );
}

#[test]
fn themed_tower_arrow_slit_repeat_step_carves_three_openings() {
    // level y=5 window class=arrow_slit side=front repeat=3 step=2 y=2
    // size=1x2 (no `mat_slot=`, `offset=` defaults to 0). A window without
    // a `mat_slot=` carves air, giving the `arrow_slit` class its slit
    // look — the second-floor wall ring (cobblestone) at those columns
    // becomes air. Wall-local u → world x: 0→1, 2→3, 4→5. World y =
    // y_offset (5) + local y (2) = 7 → world y=7..=8. Front wall is at
    // z = overhang + interior_h - 1 = 9.
    let out = lower_themed_tower();
    let ba = out.structures.get("struct::keep").unwrap();
    let mut carved = 0;
    for stamp_x in [1u32, 3, 5] {
        for y in 7..=8 {
            let i = ba
                .dims
                .index(stamp_x, y, 9)
                .expect("slit voxel within bounds");
            if ba.voxels[i].0 == 0 {
                carved += 1;
            }
        }
    }
    // 3 stamps × 2 rows tall = 6 carved air cells (air is palette index 0).
    assert_eq!(
        carved, 6,
        "arrow-slit repeat=3 step=2 size=1x2 should carve 6 air cells through the front wall",
    );
    // Cells outside the stamped columns must remain cobblestone (the level
    // walls). Sample the SW corner at y=7 which is not on the slit column.
    let cobble_idx = ba
        .palette
        .entries
        .iter()
        .position(|s| s.id == "minecraft:cobblestone")
        .expect("cobblestone palette entry present") as u16;
    let control_x = 7u32;
    let ci = ba
        .dims
        .index(control_x, 7, 9)
        .expect("control voxel within bounds");
    assert_eq!(
        ba.voxels[ci].0, cobble_idx,
        "control column (outside slit pattern) should remain cobblestone",
    );
}

#[test]
fn themed_tower_emits_zero_deferred_member_warnings() {
    let out = lower_themed_tower();
    let deferred = out
        .diagnostics
        .iter()
        .filter(|d| matches!(d.code, DiagnosticCode::DeferredMember))
        .collect::<Vec<_>>();
    assert!(
        deferred.is_empty(),
        "themed-tower must lower without deferred members; got {} — first: {}",
        deferred.len(),
        deferred
            .first()
            .map(|d| d.primary.as_str())
            .unwrap_or("<none>"),
    );
}
