//! `themed-tower.crn` lowered: a two-story keep with a theme, an eave band and arrow slits.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::DiagnosticCode;

use crate::common::{latest, lower_example};

fn lower_themed_tower() -> BlockArrayIr {
    lower_example(
        "themed-tower.crn",
        Edition::Java,
        Some(latest(Edition::Java)),
    )
}

#[test]
fn themed_tower_lowers_to_two_story_volume() {
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
    // The upper ring spans y=6..=9, and (1, y, 1) is its back-left corner.
    let out = lower_themed_tower();
    let ba = out.structures.get("struct::keep").unwrap();
    let cobble_idx = u16::try_from(
        ba.palette
            .entries
            .iter()
            .position(|s| s.id == "minecraft:cobblestone")
            .expect("cobblestone palette entry present"),
    )
    .expect("palette index fits in u16");
    for y in 6..=9 {
        let i = ba.dims.index(1, y, 1).expect("index within bounds");
        assert_eq!(
            ba.voxels[i].0, cobble_idx,
            "second-floor back-left corner at y={y} should be cobblestone",
        );
    }
}

#[test]
fn themed_tower_eave_band_paints_outside_the_front_wall() {
    // The overhang row outside the front wall is z=10.
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
    // Three 1x2 slits at x=1, 3, 5 and y=7..=8, through the front wall at z=9.
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
    assert_eq!(
        carved, 6,
        "arrow-slit repeat=3 step=2 size=1x2 should carve 6 air cells through the front wall",
    );
    let cobble_idx = u16::try_from(
        ba.palette
            .entries
            .iter()
            .position(|s| s.id == "minecraft:cobblestone")
            .expect("cobblestone palette entry present"),
    )
    .expect("palette index fits in u16");
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
        deferred.first().map_or("<none>", |d| d.primary.as_str()),
    );
}
