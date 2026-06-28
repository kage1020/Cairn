//! Integration check that `window-walkway.crn` lowers to a flat strip
//! between `home1.entry` (door, front) and `home2.front` (window, front).
//!
//! Door-only port endpoints are exercised by `l_walkway_lower` and
//! `village_lower`; this fixture pins the window-port surface so a
//! regression in [`port_world_position`]'s window branch — wrong wall
//! mirror, missed `offset + size.w / 2`, or an unintended Y lift from
//! the authored `y=` — fails loud here rather than only at the spec
//! boundary. Geometry is chosen so the Manhattan strip is a single
//! x-leg that clears both cottage footprints, pinning `blocked_count
//! == 0` as the regression-free state.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, Footprint, PaletteIndex, lower_to_block_array};
use cairn_lang_core::check::{DiagnosticCode, Severity};
use cairn_lang_core::{lower, parse, resolve};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn lower_window_walkway() -> BlockArrayIr {
    let source = std::fs::read_to_string(examples_dir().join("window-walkway.crn"))
        .expect("window-walkway.crn must read");
    let module = parse(&source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir);
    let mut out = lower_to_block_array(&ir, &resolution, None);
    let mut combined = resolution.diagnostics;
    combined.append(&mut out.diagnostics);
    out.diagnostics = combined;
    out
}

const WALKWAY_KEY: &str = "walkway::pair::home1.entry__home2.front";

#[test]
fn window_walkway_emits_single_walkway_with_expected_key() {
    let out = lower_window_walkway();
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
fn window_walkway_endpoints_pin_door_and_window_ports() {
    let out = lower_window_walkway();
    let walkway = out.walkways.get(WALKWAY_KEY).expect("walkway present");
    assert_eq!(walkway.site, "pair");
    assert_eq!(walkway.from.place, "home1");
    assert_eq!(walkway.from.port, "entry");
    assert_eq!(walkway.to.place, "home2");
    assert_eq!(walkway.to.port, "front");
    assert_eq!(walkway.path_material, "minecraft:gravel");
    // home1 origin (0,0,0), size=5x5, no roof so overhang=0; door
    // `entry` at front + `at=center` resolves to (2, 0, 5).
    // home2 origin (9,0,0) via `east_of=home1 gap=4`; window `front`
    // on front wall with offset=2 size=1x1 has u = 2 + 1/2 = 2, so the
    // wall world cell is (9 + 2, 0 + 4) = (11, 4) and the port is one
    // step outward in +z → (11, 0, 5). The L-walkway bounding box is
    // dx = 11 - 2 + 1 = 10, dz = 1 (single z = 5); origin pins to the
    // (min_x, _, min_z) corner = (2, 0, 5).
    assert_eq!(
        walkway.origin,
        (2, 0, 5),
        "window-walkway origin pins the (min_x, _, min_z) corner",
    );
    assert_eq!(
        walkway.footprint,
        Footprint { x: 10, z: 1 },
        "window-walkway runs purely along +x at z = 5",
    );
}

#[test]
fn window_walkway_block_array_paints_ten_gravel_cells() {
    // A pure x-leg of 10 cells at z=5 (x = 2..=11). An off-by-one or an
    // unintended Y lift (e.g. honouring window.y=2) would shift the
    // strip into a blocked floor cell and drop the gravel count.
    let out = lower_window_walkway();
    let ba = out
        .structures
        .get(WALKWAY_KEY)
        .expect("walkway BlockArray present");
    assert_eq!(ba.dims.y, 1, "walkway is a 1-block-thick strip");
    let gravel_idx = ba
        .palette
        .entries
        .iter()
        .position(|s| s.id == "minecraft:gravel")
        .map(|p| PaletteIndex(u16::try_from(p).expect("palette index fits in u16")))
        .expect("walkway palette contains gravel");
    let gravel_count = ba.voxels.iter().filter(|i| **i == gravel_idx).count();
    assert_eq!(
        gravel_count, 10,
        "expected 10 gravel cells (single x-leg), got {gravel_count}",
    );
}

#[test]
fn window_walkway_emits_no_deferred_or_blocked_warnings() {
    // Window-port resolution must not cascade `W_DEFERRED_MEMBER`, and
    // the chosen geometry keeps the strip clear of both placement
    // floors so `W_WALKWAY_BLOCKED` must stay silent.
    let out = lower_window_walkway();
    let deferred: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .collect();
    assert!(
        deferred.is_empty(),
        "window-walkway must not surface W_DEFERRED_MEMBER, got {deferred:#?}",
    );
    let blocked: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::WalkwayBlocked)
        .collect();
    assert!(
        blocked.is_empty(),
        "window-walkway must not collide with placements, got {blocked:#?}",
    );
}

#[test]
fn window_walkway_emits_no_resolver_errors() {
    let out = lower_window_walkway();
    let errors: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "window-walkway must not produce error-severity diagnostics, got {errors:#?}",
    );
}
