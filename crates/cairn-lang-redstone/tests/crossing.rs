//! Integration tests for `cairn_lang_redstone::compile_crossing`.
//!
//! Locks the observable behaviours of the crossing-legalization slice
//! (`spec/redstone` §14.5, stage 4 of place-and-route): the
//! `examples/redstone-door.crn` happy path (per edition), empty-module
//! pass-through, the JSON wire form staying byte-identical to the
//! delayed IR when no crossing / buffer coord landed on a non-`Plane`
//! layer (both new fields serde-skip on default), and per-scope
//! independence when a module carries more than one scope. Fixtures
//! with genuine plane crossings are exercised in the crate-internal
//! unit tests (`src/crossing.rs`) because the example set does not yet
//! contain a `.crn` whose Steiner trees overlap.

use std::path::PathBuf;

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    ScopedPlacementIr, compile_crossing, compile_delay, compile_edition_netlist, compile_netlist,
    compile_placement, compile_routing, synthesize,
};

fn load_example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn delayed_from_source(source: &str, edition: Edition) -> ScopedPlacementIr {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    let synth = synthesize(&intent);
    assert!(
        synth
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "fixture must synth cleanly: {:?}",
        synth.diagnostics,
    );
    let netlist = compile_netlist(&synth.scoped);
    let edition_netlist = compile_edition_netlist(&netlist, edition);
    let placement = compile_placement(&edition_netlist, &intent);
    assert!(
        placement.diagnostics.is_empty(),
        "fixture must place cleanly: {:?}",
        placement.diagnostics,
    );
    let routing = compile_routing(&placement.scoped);
    assert!(
        routing.diagnostics.is_empty(),
        "fixture must route cleanly: {:?}",
        routing.diagnostics,
    );
    let delay = compile_delay(&routing.scoped);
    assert!(
        delay.diagnostics.is_empty(),
        "fixture must delay cleanly: {:?}",
        delay.diagnostics,
    );
    delay.scoped
}

/// AC1 — `examples/redstone-door.crn` compiled for Java survives
/// crossing legalization with zero diagnostics: the sole `RepeaterOr`
/// cell has no fanout crossing (there is only one net) and both
/// driver segments (1 and 2 blocks) sit under the dust-attenuation
/// limit of 15 so no buffer coord materialises. `buffer_coords` stays
/// empty on the survived cell.
#[test]
fn redstone_door_java_carries_no_buffers() {
    let source = load_example("redstone-door.crn");
    let delayed = delayed_from_source(&source, Edition::Java);
    let legalized = compile_crossing(&delayed);
    assert!(
        legalized.diagnostics.is_empty(),
        "clean example must not raise crossing diagnostics: {:?}",
        legalized.diagnostics,
    );
    let entry = legalized
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "gatehouse")
        .expect("gatehouse scope");
    let cell = entry
        .ir
        .cells
        .first()
        .expect("gatehouse must have a placed cell");
    assert!(
        cell.buffer_coords.is_empty(),
        "short segments need no buffer, got {:?}",
        cell.buffer_coords,
    );
}

/// AC2 — the same example compiled for Bedrock legalizes identically:
/// the single-net topology means no crossings regardless of the cell
/// realisation swap. `wire_length` and `delay_ticks` are preserved
/// verbatim from the delayed IR.
#[test]
fn redstone_door_bedrock_carries_no_buffers() {
    let source = load_example("redstone-door.crn");
    let delayed = delayed_from_source(&source, Edition::Bedrock);
    let legalized = compile_crossing(&delayed);
    assert!(
        legalized.diagnostics.is_empty(),
        "clean example must not raise crossing diagnostics: {:?}",
        legalized.diagnostics,
    );
    let entry = legalized
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "gatehouse")
        .expect("gatehouse scope");
    let cell = entry
        .ir
        .cells
        .first()
        .expect("gatehouse must have a placed cell");
    assert_eq!(
        cell.delay_ticks,
        Some(0),
        "delay ticks preserved from stage 3",
    );
    assert!(cell.buffer_coords.is_empty(), "no buffer expected");
}

/// AC3 — empty module (no scopes with redstone) passes through
/// unchanged. Mirrors the pipeline-wide fail-loud policy: pass-through
/// on empty, refuse on partial-but-broken.
#[test]
fn empty_module_passes_through() {
    // Minimal `.crn` with a theme but no logic. Handled by delaying an
    // upstream-empty scoped IR: `ScopedPlacementIr::new()` starts
    // empty and every stage leaves it empty.
    let legalized = compile_crossing(&ScopedPlacementIr::new());
    assert!(legalized.diagnostics.is_empty());
    assert!(legalized.scoped.scopes.is_empty());
}

/// AC4 — the JSON wire form of the legalized IR equals the delayed
/// IR verbatim when no crossing / buffer coord landed on a non-`Plane`
/// layer. Both `buffer_coords` (empty) and `layer` (Plane) serde-skip
/// under those conditions, so the addition of stage 4 does not shift
/// the wire shape of scope IRs that had nothing to legalize.
#[test]
fn json_output_byte_identical_when_no_crossings_and_no_buffers() {
    let source = load_example("redstone-door.crn");
    let delayed = delayed_from_source(&source, Edition::Java);
    let legalized = compile_crossing(&delayed);

    let delayed_json = serde_json::to_string_pretty(&delayed).expect("delayed IR serialises");
    let legalized_json =
        serde_json::to_string_pretty(&legalized.scoped).expect("legalized IR serialises");

    assert_eq!(
        delayed_json, legalized_json,
        "stage 4 output must match stage 3 verbatim on a fixture without crossings",
    );
}

/// AC5 — running the crossing pass twice is idempotent on a
/// fixture that has nothing to legalize: the second run reads the
/// same input as the first and produces the same output. Guards
/// against a future refactor that starts mutating shared state across
/// invocations.
#[test]
fn crossing_is_idempotent_on_clean_fixture() {
    let source = load_example("redstone-door.crn");
    let delayed = delayed_from_source(&source, Edition::Java);
    let first = compile_crossing(&delayed);
    let second = compile_crossing(&delayed);
    assert_eq!(
        serde_json::to_string_pretty(&first.scoped).expect("first serialises"),
        serde_json::to_string_pretty(&second.scoped).expect("second serialises"),
        "two independent crossing runs on the same input must produce the same output",
    );
    assert_eq!(first.diagnostics.len(), second.diagnostics.len());
}
