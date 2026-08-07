//! Integration tests for `cairn_lang_redstone::compile_crossing`.
//!
//! Locks the observable behaviours of the crossing-legalization slice
//! (`spec/redstone` §14.5, stage 4 of place-and-route): the
//! `examples/redstone-door.crn` happy path (per edition), empty-module
//! pass-through, the JSON wire form staying byte-identical to the
//! delayed IR apart from the `stage` tag when no crossing / buffer
//! coord landed on a non-`Plane` layer (both new fields serde-skip on
//! default), the tag itself keeping a zero-buffer legalized dump
//! distinguishable from its delayed input, and per-scope
//! independence when a module carries more than one scope. Fixtures
//! with genuine plane crossings are exercised in the crate-internal
//! unit tests (`src/crossing.rs`) because the example set does not yet
//! contain a `.crn` whose Steiner trees overlap.

use std::path::PathBuf;

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    DiagnosticCode, ScopedPlacementIr, compile_crossing, compile_delay, compile_edition_netlist,
    compile_netlist, compile_placement, compile_routing, synthesize,
};

mod common;

use common::normalize_stage_tags;

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
            .all(|d| d.severity() != Severity::Error),
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
        cell.buffer_coords().is_empty(),
        "short segments need no buffer, got {:?}",
        cell.buffer_coords(),
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
        cell.delay_ticks(),
        Some(0),
        "delay ticks preserved from stage 3",
    );
    assert!(cell.buffer_coords().is_empty(), "no buffer expected");
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

/// AC4 — apart from the `stage` tag, the JSON wire form of the
/// legalized IR equals the delayed IR verbatim when no crossing /
/// buffer coord landed on a non-`Plane` layer. Both `buffer_coords`
/// (empty) and `layer` (Plane) serde-skip under those conditions, so
/// the addition of stage 4 shifts nothing but the tag on scope IRs
/// that had nothing to legalize.
#[test]
fn json_output_byte_identical_apart_from_stage_tag_when_no_crossings_and_no_buffers() {
    let source = load_example("redstone-door.crn");
    let delayed = delayed_from_source(&source, Edition::Java);
    let legalized = compile_crossing(&delayed);

    let delayed_json = serde_json::to_string_pretty(&delayed).expect("delayed IR serialises");
    let legalized_json =
        serde_json::to_string_pretty(&legalized.scoped).expect("legalized IR serialises");

    assert_eq!(
        normalize_stage_tags(&delayed_json),
        normalize_stage_tags(&legalized_json),
        "stage 4 output must match stage 3 apart from the stage tag on a fixture without crossings",
    );
}

/// The reason the `stage` tag exists, pinned end-to-end:
/// `redstone-door.crn`
/// materialises zero buffers, so before the tag existed the
/// delayed and legalized dumps were byte-identical and a downstream
/// consumer could not tell "stage 3 output" from "stage 4 output with
/// nothing to legalize". The two dumps must now differ, and differ
/// *only* by the tag — the empty `buffer_coords` still serde-skips, so
/// presence-of-empty-array was deliberately not adopted as the
/// discriminator.
#[test]
fn legalized_with_zero_buffers_is_distinguishable_from_delayed() {
    let source = load_example("redstone-door.crn");
    let delayed = delayed_from_source(&source, Edition::Java);
    let legalized = compile_crossing(&delayed);

    let delayed_json = serde_json::to_string(&delayed).expect("delayed IR serialises");
    let legalized_json = serde_json::to_string(&legalized.scoped).expect("legalized IR serialises");
    assert_ne!(
        delayed_json, legalized_json,
        "a stage-4 dump must not be byte-identical to its stage-3 input",
    );

    for (json, expected) in [(&delayed_json, "delay"), (&legalized_json, "crossing")] {
        let value: serde_json::Value = serde_json::from_str(json).expect("dump parses");
        let cells = value
            .as_array()
            .and_then(|scopes| scopes.iter().find(|s| s["name"] == "gatehouse"))
            .map(|scope| &scope["ir"]["cells"])
            .and_then(serde_json::Value::as_array)
            .expect("gatehouse cells");
        assert!(!cells.is_empty(), "gatehouse must carry a placed cell");
        for cell in cells {
            assert_eq!(cell["stage"], expected, "stage tag on {json}");
            assert!(
                cell.get("buffer_coords").is_none(),
                "empty buffer_coords must still serde-skip: {json}",
            );
        }
    }
}

/// AC — mirror of
/// `json_output_byte_identical_apart_from_stage_tag_when_no_crossings_and_no_buffers`
/// for the `with-crossings` case. `examples/crossbar.crn` produces
/// genuine plane overlaps between its two cell-driven output-pad
/// nets, but because the current pipeline does not lift the wire
/// itself onto the bridge layer (the bridge budget is reserved for
/// buffer-repeater escapes) and every driver segment on this fixture
/// sits below `DUST_ATTENUATION_LIMIT`, the legalized JSON must
/// still equal the delayed JSON apart from the stage tag. Pins two
/// invariants at once: (a) both gate cells and both door outputs
/// survive legalization intact — a regression that elided the crossed
/// scope would trip the byte-identity assertion via a shorter left
/// side; (b) a silently-absorbed crossing does not shift the wire
/// form.
#[test]
fn json_output_byte_identical_apart_from_stage_tag_with_crossings() {
    let source = load_example("crossbar.crn");
    let delayed = delayed_from_source(&source, Edition::Java);
    let legalized = compile_crossing(&delayed);
    assert!(
        legalized.diagnostics.is_empty(),
        "crossbar.crn must legalize cleanly at void=2: {:?}",
        legalized.diagnostics,
    );

    let delayed_json = serde_json::to_string_pretty(&delayed).expect("delayed IR serialises");
    let legalized_json =
        serde_json::to_string_pretty(&legalized.scoped).expect("legalized IR serialises");

    assert_eq!(
        normalize_stage_tags(&delayed_json),
        normalize_stage_tags(&legalized_json),
        "silently-absorbed crossings must not shift the wire form of the legalized IR",
    );
}

/// AC — `examples/crossbar.crn` with `void=1` refuses with
/// `E_CROSSING_CONGESTION`, proving the fixture actually produces
/// plane overlaps the pass has to see: a fixture without any
/// overlap would legalize cleanly at `void=1` and the byte-identity
/// mirror above would still pass by vacuous truth.
#[test]
fn crossbar_void_one_refuses_with_crossing_congestion() {
    let source = load_example("crossbar.crn");
    let patched = source.replace("void=2", "void=1");
    assert_ne!(
        source, patched,
        "crossbar.crn no longer contains the `void=2` needle — fixture drifted \
         and the void=1 refusal path is not being exercised",
    );
    let delayed = delayed_from_source(&patched, Edition::Java);
    let legalized = compile_crossing(&delayed);
    assert!(
        legalized
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::CrossingCongestion),
        "void=1 crossbar must trip E_CROSSING_CONGESTION: {:?}",
        legalized.diagnostics,
    );
    assert!(
        legalized.scoped.scopes.iter().all(|e| e.name != "crossbar"),
        "failed scope must elide from the legalized IR",
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
