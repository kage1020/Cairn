//! `village.crn` lowers to one block array per place and one walkway per `connect` row, with no
//! deferred member.

use cairn_lang_core::block_array::Footprint;
use cairn_lang_core::check::{DiagnosticCode, Severity};

use crate::common::{lowered_with_resolver_diagnostics, read_example};

#[test]
fn village_emits_three_placements_and_two_walkways() {
    let out = lowered_with_resolver_diagnostics(&read_example("village.crn"));
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
        // home1's front port is (5, 0, 8), home2's (20, 0, 8) and home3's (5, 0, -6).
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
                // A straight run would cut home1's floor, so the route detours round its east face:
                // the router expands +x first, which wins the equal-length tie.
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
    let out = lowered_with_resolver_diagnostics(&read_example("village.crn"));
    for key in out.walkways.keys() {
        let ba = out
            .structures
            .get(key.as_str())
            .unwrap_or_else(|| panic!("structures missing entry for walkway key {key}"));
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
    let out = lowered_with_resolver_diagnostics(&read_example("village.crn"));
    let blocked: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::WalkwayBlocked)
        .collect();
    assert!(
        blocked.is_empty(),
        "village.crn must route every walkway without collisions, got {blocked:#?}",
    );
    // Manhattan distance 14, plus 5 cells out to x=10 and 5 back: 24 steps, 25 cells.
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
    // Local coordinates are relative to the origin (5, 0, -6).
    let id_at = |x: u32, z: u32| -> &str {
        let i = ba.dims.index(x, 0, z).expect("in-range cell");
        ba.palette.entries[usize::from(ba.voxels[i].0)].id.as_str()
    };
    // home3's and home1's front ports.
    assert_eq!(id_at(0, 0), "minecraft:gravel");
    assert_eq!(id_at(0, 14), "minecraft:gravel");
    // The two east-face corners of the U (world (10,0,8) / (10,0,-6)).
    assert_eq!(id_at(5, 14), "minecraft:gravel");
    assert_eq!(id_at(5, 0), "minecraft:gravel");
    // Inside home1's floor, on the straight route the detour avoids.
    assert_eq!(id_at(0, 7), "minecraft:air");
}

#[test]
fn village_emits_zero_deferred_member_warnings() {
    let out = lowered_with_resolver_diagnostics(&read_example("village.crn"));
    let deferred = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .count();
    assert_eq!(
        deferred, 0,
        "village.crn must voxelise without W_DEFERRED_MEMBER warnings, diagnostics: {:#?}",
        out.diagnostics,
    );
}

#[test]
fn village_emits_no_resolver_errors() {
    let out = lowered_with_resolver_diagnostics(&read_example("village.crn"));
    let errors: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity() == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "village.crn must not produce any error-severity diagnostics, got {errors:#?}",
    );
}
