//! Integration check that `l-walkway.crn` lowers to a Manhattan L between
//! `home1.entry` (front, +z) and `home3.side_entry` (left, -x).
//!
//! The unit tests in `block_array::walkway` already pin `l_path`'s corner
//! dedup directly, but until this fixture landed every shipping example
//! laid its walkways purely along one axis. A regression that broke the
//! L's elbow (corner omitted, axis swap, off-by-one in either leg) would
//! pass `village_lower` and only fail at this boundary. The fixture
//! intentionally places the middle home so the elbow column clears every
//! footprint, pinning `blocked_count == 0` as the regression-free state;
//! that, and the walkway's key, are asserted for every walkway example in
//! `walkway_examples_lower`.

use cairn_lang_core::block_array::{BlockArrayIr, Footprint, PaletteIndex};

mod common;
use common::{lowered_with_resolver_diagnostics, read_example};

fn lower_l_walkway() -> BlockArrayIr {
    lowered_with_resolver_diagnostics(&read_example("l-walkway.crn"))
}

const WALKWAY_KEY: &str = "walkway::duo::home1.entry__home3.side_entry";

#[test]
fn l_walkway_spans_both_axes_with_pinned_bounding_box() {
    // home1.entry resolves to (1, 0, 3); home3.side_entry to (12, 0, -12).
    // The Manhattan L runs +x from x=1 to x=12 at z=3, then -z from z=3 to
    // z=-12 at x=12. Bounding box is dx = 12-1+1 = 12, dz = 3-(-12)+1 = 16;
    // origin is the (min_x, _, min_z) corner = (1, 0, -12).
    let out = lower_l_walkway();
    let walkway = out.walkways.get(WALKWAY_KEY).expect("walkway present");
    assert_eq!(walkway.site, "duo");
    assert_eq!(walkway.from.place, "home1");
    assert_eq!(walkway.from.port, "entry");
    assert_eq!(walkway.to.place, "home3");
    assert_eq!(walkway.to.port, "side_entry");
    assert_eq!(walkway.path_material, "minecraft:gravel");
    assert_eq!(
        walkway.origin,
        (1, 0, -12),
        "L-walkway origin pins the (min_x, _, min_z) corner of the bounding box",
    );
    assert_eq!(
        walkway.footprint,
        Footprint { x: 12, z: 16 },
        "L-walkway spans both axes; pure-x or pure-z geometry cannot reproduce these dims",
    );
}

#[test]
fn l_walkway_block_array_corner_cell_holds_gravel() {
    // The elbow at world (12, 0, 3) lives at local (lx=11, 0, lz=15) inside
    // the walkway BlockArray. If `l_path` ever omits the corner cell, this
    // index would stay AIR; if it ever writes the wrong cell, the corner
    // would be AIR while a neighbour ends up gravel.
    let out = lower_l_walkway();
    let ba = out
        .structures
        .get(WALKWAY_KEY)
        .expect("walkway BlockArray present");
    let corner_idx = ba
        .dims
        .index(11, 0, 15)
        .expect("corner cell (11, 0, 15) within walkway dims");
    let palette_idx = ba.voxels[corner_idx];
    assert_ne!(
        palette_idx,
        PaletteIndex::AIR,
        "L-walkway corner cell must hold gravel, not air",
    );
    let block_id = ba.palette.entries[usize::from(palette_idx.0)].id.as_str();
    assert_eq!(
        block_id, "minecraft:gravel",
        "L-walkway corner palette entry must resolve to gravel",
    );
}

#[test]
fn l_walkway_block_array_paints_27_gravel_cells() {
    // x-leg of 12 cells (x=1..=12 at z=3) + z-leg of 15 fresh cells
    // (z=2..=-12 at x=12, the corner is shared with the x-leg) = 27 gravel
    // voxels. A leg off-by-one would shift this by ±1; an axis swap would
    // change it to ~16 or ~12.
    let out = lower_l_walkway();
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
        gravel_count, 27,
        "expected 27 gravel cells (x-leg 12 + z-leg 15 after corner-dedup), got {gravel_count}",
    );
}
