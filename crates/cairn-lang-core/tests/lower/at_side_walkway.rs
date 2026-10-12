//! `at-side-walkway.crn`'s `at=right` and `at=left` doors anchor one straight walkway at z=3.

use cairn_lang_core::block_array::{BlockArrayIr, Footprint, PaletteIndex};

use crate::common::{lowered_with_resolver_diagnostics, read_example};

fn lower_at_side_walkway() -> BlockArrayIr {
    lowered_with_resolver_diagnostics(&read_example("at-side-walkway.crn"))
}

const WALKWAY_KEY: &str = "walkway::duo::west.east_corner__east.west_corner";

#[test]
fn at_side_walkway_pins_endpoint_anchors_at_wall_corners() {
    // Ports at (2, 0, 3) and (8, 0, 3); `at=center` on either end would move dx off 7.
    let out = lower_at_side_walkway();
    let walkway = out.walkways.get(WALKWAY_KEY).expect("walkway present");
    assert_eq!(walkway.site, "duo");
    assert_eq!(walkway.from.place, "west");
    assert_eq!(walkway.from.port, "east_corner");
    assert_eq!(walkway.to.place, "east");
    assert_eq!(walkway.to.port, "west_corner");
    assert_eq!(walkway.path_material, "minecraft:gravel");
    assert_eq!(
        walkway.origin,
        (2, 0, 3),
        "at-side-walkway origin pins the (min_x, _, min_z) corner of the bounding box",
    );
    assert_eq!(
        walkway.footprint,
        Footprint { x: 7, z: 1 },
        "at-side-walkway is a single x-axis leg between the two corner ports",
    );
}

#[test]
fn at_side_walkway_paints_seven_gravel_cells_along_the_leg() {
    // One 7-cell x-leg at z = 3; dropping the endpoints would leave 5, an off-by-one 6 or 8.
    let out = lower_at_side_walkway();
    let ba = out
        .structures
        .get(WALKWAY_KEY)
        .expect("walkway BlockArray present");
    let gravel_idx = ba
        .palette
        .entries
        .iter()
        .position(|s| s.id == "minecraft:gravel")
        .map(|p| PaletteIndex(u16::try_from(p).expect("palette index fits in u16")))
        .expect("walkway palette contains gravel");
    let gravel_count = ba.voxels.iter().filter(|i| **i == gravel_idx).count();
    assert_eq!(
        gravel_count, 7,
        "expected 7 gravel cells (one per x in 2..=8), got {gravel_count}",
    );
}
