//! Integration check that `at-side-walkway.crn` lowers two corner-anchored
//! doors (`at=right` on the west cottage's front wall, `at=left` on the
//! east cottage's) into a straight x-axis walkway.
//!
//! The unit tests in `block_array::walkway` already pin
//! [`door_anchor_offset`](cairn_lang_core::block_array::walkway) for every
//! `at=center|left|right` × wall-side combination. This fixture pins the
//! same vocabulary at the integration boundary: a regression that silently
//! folded `at=left|right` back to `at=center` would shift one of the two
//! ports by exactly one cell and break the bounding-box assertions below.
//! Geometry is chosen so the L collapses to a single x-axis run at z=3 and
//! `blocked_count == 0`, isolating the anchor-resolution behaviour from
//! the L-corner and collision behaviours covered by `l_walkway_lower`.
//! That the fixture lowers to one unblocked walkway with no deferred
//! door is pinned for every walkway example in `walkway_examples_lower`.

use cairn_lang_core::block_array::{BlockArrayIr, Footprint, PaletteIndex};

mod common;
use common::{lowered_with_resolver_diagnostics, read_example};

fn lower_at_side_walkway() -> BlockArrayIr {
    lowered_with_resolver_diagnostics(&read_example("at-side-walkway.crn"))
}

const WALKWAY_KEY: &str = "walkway::duo::west.east_corner__east.west_corner";

#[test]
fn at_side_walkway_pins_endpoint_anchors_at_wall_corners() {
    // west origin = (0, 0, 0); place_dims = (3, 1, 3); front wall length =
    // 3 → `at=right` pins u = 2. World wall = (0 + 0 + 2, _, 0 + 0 + 3 - 1)
    // = (2, _, 2); +z normal step → port (2, 0, 3).
    //
    // east_of=west gap=5 + prev.dims.x = 3 → east origin = (8, 0, 0). Front
    // wall `at=left` pins u = 0. World wall = (8, _, 2); +z → port
    // (8, 0, 3). The L collapses to a single x-axis leg at z = 3 because
    // the two ports share their z.
    //
    // Bounding box: dx = 8 - 2 + 1 = 7, dz = 1; origin = (min_x, _,
    // min_z) = (2, 0, 3). A regression that rounds `at=right` back to
    // `at=center` (u = 1) would shift the west port to (1, 0, 3) and
    // make dx = 8 - 1 + 1 = 8. A symmetric break on `at=left` would
    // make dx = 7 - 1 = 6.
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
    // x-leg of 7 cells (x = 2..=8 at z = 3) and a zero-cell z-leg (the two
    // ports share z), so every cell of the dims (7, 1, 1) BlockArray should
    // resolve to gravel. A regression that drops the endpoints would land
    // at 5 cells; one that off-by-ones a single anchor would land at 6 or 8.
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
