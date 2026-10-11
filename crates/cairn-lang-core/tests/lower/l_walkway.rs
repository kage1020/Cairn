//! `l-walkway.crn` lowers to a Manhattan L between a front and a left port, its elbow laid once.

use cairn_lang_core::block_array::{BlockArrayIr, Footprint, PaletteIndex};

use crate::common::{lowered_with_resolver_diagnostics, read_example};

fn lower_l_walkway() -> BlockArrayIr {
    lowered_with_resolver_diagnostics(&read_example("l-walkway.crn"))
}

const WALKWAY_KEY: &str = "walkway::duo::home1.entry__home3.side_entry";

#[test]
fn l_walkway_spans_both_axes_with_pinned_bounding_box() {
    // The L runs +x from (1, 0, 3) to x=12, then -z to (12, 0, -12).
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
    // The elbow at world (12, 0, 3) is local (11, 0, 15).
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
    // A 12-cell x-leg and a 15-cell z-leg sharing the corner: 27 cells.
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
