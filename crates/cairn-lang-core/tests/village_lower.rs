//! End-to-end check that `village.crn` lowers to the expected site shape.
//!
//! The village example is the M3-PR4 gate: three placements connected by
//! two gravel walkways. The assertions pin the per-place block-array
//! emission (one `BlockArray` per place under the matching
//! `site::SITE::PLACE_ID` key), the walkway emission (one entry per
//! `connect` row, with the `walkway::SITE::FROM_PLACE.FROM_PORT__TO_PLACE.TO_PORT`
//! key shape), and the absence of `W_DEFERRED_MEMBER` warnings — a
//! regression in any of those would mean a future change quietly
//! re-broke either site lowering or walkway voxelisation.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
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
            key.starts_with("walkway::hamlet::"),
            "walkway key `{key}` should start with `walkway::hamlet::`",
        );
        assert_eq!(walkway.site, "hamlet");
        assert_eq!(walkway.from_port, "entry");
        assert_eq!(walkway.to_port, "entry");
        assert_eq!(walkway.path_material, "minecraft:gravel");
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
            .get(key)
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
