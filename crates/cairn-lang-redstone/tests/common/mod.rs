//! Helpers shared by the redstone integration test binaries.
//!
//! Two kinds of thing live here. The fixture builders — `load_example`,
//! `synth_source` and the `*_from_source` ladder — pin one prefix of the
//! pipeline each, and every rung is the previous one plus a stage and the
//! assertion that the stage ran clean, so a change to a pass's signature
//! or to what "clean" means for a fixture lands in one place rather than
//! in each binary downstream of it. And `normalize_stage_tags` is
//! machinery whose *correctness* is shared: it has to agree with the JSON
//! wire form across every stage, and would rot silently if each binary
//! kept its own copy.

// Every test binary compiles this module on its own and calls a subset of
// it, so the unused-item lint would fire per binary. The workspace has no
// other `#![allow]`; the fix that removes this one is a dev-dependency
// helper crate, worth doing if the set keeps growing.
#![allow(dead_code)]

use std::path::PathBuf;

use cairn_lang_core::check::Severity;
use cairn_lang_core::{Edition, IntentModule, lower, parse};
use cairn_lang_redstone::{
    DiagnosticCode, ScopedEditionNetlistIr, ScopedPlacementIr, SynthOutput, compile_delay,
    compile_edition_netlist, compile_netlist, compile_placement, compile_routing, synthesize,
};

/// Load `examples/<name>` relative to the workspace root.
pub fn load_example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// Parse, lower and synthesise `source` into the Logic IR, diagnostics
/// and all — the front end's output before any downstream stage.
pub fn synth_source(source: &str) -> SynthOutput {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    synthesize(&intent)
}

/// 1-based line of a byte offset, counted the way the source reads.
pub fn line_of(source: &str, offset: usize) -> usize {
    source[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

/// Synthesise and compile `source` to the Edition Netlist IR for
/// `edition`, refusing a fixture the front end reports an error on. The
/// intent module comes back with it because the placement pass reads the
/// circuit reservations from it.
pub fn edition_netlist_from_source(
    source: &str,
    edition: Edition,
) -> (ScopedEditionNetlistIr, IntentModule) {
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
    (edition_netlist, intent)
}

/// [`edition_netlist_from_source`] plus placement, which must run clean:
/// everything downstream of placement reads the placed IR.
pub fn placement_from_source(source: &str, edition: Edition) -> ScopedPlacementIr {
    let (edition_netlist, intent) = edition_netlist_from_source(source, edition);
    let placement = compile_placement(&edition_netlist, &intent);
    assert!(
        placement.diagnostics.is_empty(),
        "fixture must place cleanly (these tests are downstream of placement): {:?}",
        placement.diagnostics,
    );
    placement.scoped
}

/// [`placement_from_source`] plus routing.
///
/// One warning rides through, by code rather than by severity: a scope
/// whose nets climbed past each other carries
/// `W_ROUTE_CROSS_LAYER_CLEARANCE`, which names the pairs the physical
/// tile layer has to separate and elides nothing. Anything else — a
/// refusal, which would take the scope out of the IR the tests read, or a
/// warning routing has yet to grow — is the fixture set saying something
/// new, and worth failing on rather than passing unread.
pub fn routed_from_source(source: &str, edition: Edition) -> ScopedPlacementIr {
    let routing = compile_routing(&placement_from_source(source, edition));
    assert!(
        routing
            .diagnostics
            .iter()
            .all(|d| d.code == DiagnosticCode::RouteCrossLayerClearance),
        "fixture must route with nothing but the cross-layer advisory (these tests are downstream of routing): {:?}",
        routing.diagnostics,
    );
    routing.scoped
}

/// [`routed_from_source`] plus the delay pass, which must run clean.
pub fn delayed_from_source(source: &str, edition: Edition) -> ScopedPlacementIr {
    let delay = compile_delay(&routed_from_source(source, edition));
    assert!(
        delay.diagnostics.is_empty(),
        "fixture must delay cleanly: {:?}",
        delay.diagnostics,
    );
    delay.scoped
}

/// Rewrite every `"stage": "<name>"` value to a fixed placeholder so
/// two adjacent stages' dumps can be byte-compared on everything
/// *except* the tag that distinguishes them.
///
/// The routing, delay, and crossing binaries each assert that their
/// pass perturbs nothing but the field it writes. Since every cell
/// carries a `stage` tag whose value moves from stage to stage, that
/// assertion has to neutralise the tag first — and it has to do so
/// identically in all three, or a change to the key name would be
/// caught in one binary and papered over in another.
///
/// Handles both the compact and the pretty spelling: the separator
/// between the key and the value is copied through verbatim rather
/// than assumed.
pub fn normalize_stage_tags(json: &str) -> String {
    const KEY: &str = "\"stage\":";
    let mut out = String::with_capacity(json.len());
    let mut rest = json;
    while let Some(idx) = rest.find(KEY) {
        let (before, after) = rest.split_at(idx + KEY.len());
        out.push_str(before);
        let open = after.find('"').expect("stage tag value is a string");
        let close = after[open + 1..]
            .find('"')
            .expect("stage tag value is a closed string");
        out.push_str(&after[..open]);
        out.push_str("\"<stage>\"");
        rest = &after[open + close + 2..];
    }
    out.push_str(rest);
    out
}
