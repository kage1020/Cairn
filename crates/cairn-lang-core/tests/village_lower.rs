//! End-to-end check that `village.crn` lowers to the expected site shape.
//!
//! Three placements connected by two gravel walkways. The assertions pin
//! the per-place block-array emission (one `BlockArray` per place under
//! the matching `site::SITE::PLACE_ID` key), the walkway emission (one
//! entry per `connect` row, with the
//! `walkway::SITE::FROM_PLACE.FROM_PORT__TO_PLACE.TO_PORT` key shape),
//! and the absence of `W_DEFERRED_MEMBER` warnings — a regression in any
//! of those would mean a future change quietly re-broke either site
//! lowering or walkway voxelisation.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, Footprint, lower_to_block_array};
use cairn_lang_core::check::{DiagnosticCode, Severity};
use cairn_lang_core::{lower, parse, resolve};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn lower_village() -> BlockArrayIr {
    let source =
        std::fs::read_to_string(examples_dir().join("village.crn")).expect("village.crn must read");
    let module = parse(&source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir);
    let mut out = lower_to_block_array(&ir, &resolution, None);
    // Resolver diagnostics are produced before lowering; mirror the CLI
    // wiring so the assertion below sees both lists merged.
    let mut combined = resolution.diagnostics;
    combined.append(&mut out.diagnostics);
    out.diagnostics = combined;
    out
}

#[test]
fn village_emits_three_placements_and_two_walkways() {
    let out = lower_village();
    // Three named places — `home1`, `home2`, `home3` — should each have
    // their own `BlockArray` and `Placement` under the matching
    // `site::hamlet::*` key.
    for place_id in ["home1", "home2", "home3"] {
        let key = format!("site::hamlet::{place_id}");
        assert!(
            out.structures.contains_key(&key),
            "missing per-place block array for {key}",
        );
        assert!(
            out.placements.contains_key(&key),
            "missing placement record for {key}",
        );
    }
    // Two `connect` rows → two walkway IR entries, both at the same
    // site, with `path_material` lifted to the concrete `minecraft:gravel`
    // canonical id.
    assert_eq!(
        out.walkways.len(),
        2,
        "expected exactly two walkway entries, got {:?}",
        out.walkways.keys().collect::<Vec<_>>(),
    );
    for (key, walkway) in &out.walkways {
        assert!(
            key.as_str().starts_with("walkway::hamlet::"),
            "walkway key `{key}` should start with `walkway::hamlet::`",
        );
        assert_eq!(walkway.site, "hamlet");
        assert_eq!(walkway.from.port, "entry");
        assert_eq!(walkway.to.port, "entry");
        assert_eq!(walkway.path_material, "minecraft:gravel");
        // Pin origin/dims so an axis swap (x↔z) or off-by-one in the
        // overhang shift fails loud here. home1 sits at (0,0,0) with
        // overhang=1, so its front port resolves to (5, 0, 8);
        // home2 east_of home1 gap=4 → origin (15, 0, 0), front port
        // (20, 0, 8); home3 north_of home1 gap=5 → origin (0, 0, -14),
        // front port (5, 0, -6).
        match (walkway.from.place.as_str(), walkway.to.place.as_str()) {
            ("home1", "home2") => {
                assert_eq!(
                    walkway.origin,
                    (5, 0, 8),
                    "home1↔home2 walkway should start at home1's front port",
                );
                assert_eq!(
                    walkway.footprint,
                    Footprint { x: 16, z: 1 },
                    "home1↔home2 walkway runs purely along +x",
                );
            }
            ("home1", "home3") => {
                // A straight z run from (5, 8) to (5, -6) would cut
                // through home1's floor, so the router detours around
                // home1's east face: east along z=8 to x=10 (one cell
                // past the floor's x∈[1,9]), north to z=-6, back west
                // to the port at x=5. The east side wins the
                // equal-length tie against the west side because the
                // router expands +x first — these pins depend on the
                // `STEP_DIRS` order in `block_array::walkway` (unit
                // test `route_path_breaks_symmetric_ties_toward_positive_x`
                // pins the same tie-break in isolation).
                assert_eq!(
                    walkway.origin,
                    (5, 0, -6),
                    "home1↔home3 walkway bounding box starts at home3's front port",
                );
                assert_eq!(
                    walkway.footprint,
                    Footprint { x: 6, z: 15 },
                    "home1↔home3 walkway detours around home1's east face",
                );
            }
            (from, to) => panic!("unexpected walkway pair {from}↔{to}"),
        }
    }
}

#[test]
fn village_walkway_block_arrays_share_keys_with_walkways_map() {
    let out = lower_village();
    // The `BlockArray` for each walkway must live under the same scope
    // key as its `Walkway` metadata so a downstream consumer can join the
    // two maps without an extra translation step.
    for key in out.walkways.keys() {
        let ba = out
            .structures
            .get(key.as_str())
            .unwrap_or_else(|| panic!("structures missing entry for walkway key {key}"));
        // Walkways are flat strips: dims.y == 1 by construction.
        assert_eq!(
            ba.dims.y, 1,
            "walkway {key} should be a 1-block-thick strip"
        );
        assert!(
            ba.palette
                .entries
                .iter()
                .any(|s| s.id == "minecraft:gravel"),
            "walkway palette must contain gravel, got {:?}",
            ba.palette.entries.iter().map(|s| &s.id).collect::<Vec<_>>(),
        );
    }
}

#[test]
fn village_emits_zero_walkway_blocked_warnings() {
    // The home1↔home3 row used to warn `W_WALKWAY_BLOCKED` (its straight
    // L cut through home1's floor and shipped a strip with a 7-cell
    // hole). With the ground-plane router the walkway detours around the
    // building instead, so the whole example must lower warning-free and
    // the laid strip must be unbroken.
    let out = lower_village();
    let blocked: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::WalkwayBlocked)
        .collect();
    assert!(
        blocked.is_empty(),
        "village.crn must route every walkway without collisions, got {blocked:#?}",
    );
    // The detour is a shortest route: Manhattan distance 14 plus 5 cells
    // out to x=10 and 5 cells back → 24 steps, 25 gravel cells.
    let ba = out
        .structures
        .get("walkway::hamlet::home1.entry__home3.entry")
        .expect("home1↔home3 walkway block array present");
    let gravel_count = (0..ba.dims.volume())
        .filter(|&i| ba.palette.entries[usize::from(ba.voxels[i].0)].id == "minecraft:gravel")
        .count();
    assert_eq!(
        gravel_count, 25,
        "detour must lay an unbroken 25-cell gravel strip",
    );
    // Representative cells, so a count-preserving deformation (a strip
    // that wanders but still lays 25 cells) cannot slip past the count
    // assertion. Local coordinates are relative to origin (5, 0, -6).
    let id_at = |x: u32, z: u32| -> &str {
        let i = ba.dims.index(x, 0, z).expect("in-range cell");
        ba.palette.entries[usize::from(ba.voxels[i].0)].id.as_str()
    };
    // home3's front port (world (5,0,-6)) and home1's front port
    // (world (5,0,8)) anchor the two ends.
    assert_eq!(id_at(0, 0), "minecraft:gravel");
    assert_eq!(id_at(0, 14), "minecraft:gravel");
    // The two east-face corners of the U (world (10,0,8) / (10,0,-6)).
    assert_eq!(id_at(5, 14), "minecraft:gravel");
    assert_eq!(id_at(5, 0), "minecraft:gravel");
    // A cell inside home1's floor on the straight line the old L took
    // (world (5,0,1)) must stay air — the detour goes around it.
    assert_eq!(id_at(0, 7), "minecraft:air");
}

#[test]
fn village_emits_zero_deferred_member_warnings() {
    let out = lower_village();
    let deferred = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .count();
    assert_eq!(
        deferred, 0,
        "village.crn must voxelise without W_DEFERRED_MEMBER warnings now that connect rows lower, diagnostics: {:#?}",
        out.diagnostics,
    );
}

#[test]
fn village_emits_no_resolver_errors() {
    let out = lower_village();
    let errors: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "village.crn must not produce any error-severity diagnostics, got {errors:#?}",
    );
}
