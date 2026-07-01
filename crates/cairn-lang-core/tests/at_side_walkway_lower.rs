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

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, Footprint, PaletteIndex, lower_to_block_array};
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::{lower, parse, resolve};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn lower_at_side_walkway() -> BlockArrayIr {
    let source = std::fs::read_to_string(examples_dir().join("at-side-walkway.crn"))
        .expect("at-side-walkway.crn must read");
    let module = parse(&source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir);
    let mut out = lower_to_block_array(&ir, &resolution, None);
    // Mirror the CLI wiring: resolver diagnostics fire before lowering, so
    // merge both lists for the W_WALKWAY_BLOCKED / W_DEFERRED_MEMBER
    // assertions below.
    let mut combined = resolution.diagnostics;
    combined.append(&mut out.diagnostics);
    out.diagnostics = combined;
    out
}

const WALKWAY_KEY: &str = "walkway::duo::west.east_corner__east.west_corner";

#[test]
fn at_side_walkway_emits_single_walkway_with_expected_key() {
    let out = lower_at_side_walkway();
    assert_eq!(
        out.walkways.len(),
        1,
        "expected exactly one walkway, got {:?}",
        out.walkways.keys().collect::<Vec<_>>(),
    );
    assert!(
        out.walkways.contains_key(WALKWAY_KEY),
        "missing walkway under key `{WALKWAY_KEY}`, keys = {:?}",
        out.walkways.keys().collect::<Vec<_>>(),
    );
}

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

#[test]
fn at_side_walkway_emits_no_walkway_blocked_warning() {
    // Geometry is chosen so the x-leg at z=3 sits north of both cottages
    // (their max z is 2), so `build_walkway_array` should report
    // `blocked_count == 0`. The diagnostic's absence is the integration
    // proxy for that count.
    let out = lower_at_side_walkway();
    let blocked: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::WalkwayBlocked)
        .collect();
    assert!(
        blocked.is_empty(),
        "at-side-walkway must not collide with any placement, got {blocked:#?}",
    );
}

#[test]
fn at_side_walkway_emits_no_deferred_member_warning() {
    // `at=left|right` must lower cleanly — no W_DEFERRED_MEMBER cascade
    // from `carve_door` or from `port_world_position`. A regression that
    // restores the old `Some("center") only` arm would fire one defer per
    // corner door (carve) plus one per connect endpoint (port).
    let out = lower_at_side_walkway();
    let deferred: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .collect();
    assert!(
        deferred.is_empty(),
        "at-side-walkway must not defer any door member, got {deferred:#?}",
    );
}
