//! Helpers shared by the redstone integration tests. The fixture builders (`load_example`,
//! `synth_source`, `gate_source` and the `*_from_source` ladder) each pin one prefix of the
//! pipeline, every rung the previous one plus a stage that must run clean, so a change to a
//! pass's signature lands in one place. `normalize_stage_tags` and `strip_int_field` have to agree
//! with the JSON wire form across every stage, so they exist once rather than per module.

use std::fmt::Write as _;
use std::path::PathBuf;

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{Edition, IntentModule, lower, parse};
use cairn_lang_redstone::{
    Diagnostic, DiagnosticCode, ScopedEditionNetlistIr, ScopedPlacementIr, SynthOutput,
    compile_crossing, compile_delay, compile_edition_netlist, compile_netlist, compile_placement,
    compile_routing, synthesize,
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

const GATE_PRELUDE: &str = concat!(
    "@cairn 2026.06\n",
    "\n",
    "theme t:\n",
    "  slot wall -> @cobblestone\n",
    "\n",
    "struct gate size=5x5\n",
    "  walls class=outer mat_slot=wall height=3\n",
);

/// A walled `struct gate` with `body` as the rest of its members.
pub fn gate_source(body: &str) -> String {
    format!("{GATE_PRELUDE}{body}")
}

/// Every finding's code, in the order `out` carries them.
pub fn codes(out: &SynthOutput) -> Vec<&'static str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// The one finding of `code` in `out`, failing on none or several.
pub fn only(out: &SynthOutput, code: DiagnosticCode) -> &Diagnostic {
    let found: Vec<_> = out.diagnostics.iter().filter(|d| d.code == code).collect();
    assert_eq!(
        found.len(),
        1,
        "expected one {code:?}: {:#?}",
        out.diagnostics
    );
    found[0]
}

/// `name` (`sig.open`) as the `DottedRef` the IRs key their signal maps
/// by.
pub fn sig(name: &str) -> DottedRef {
    let mut parts = name.split('.').map(str::to_owned);
    let head = parts.next().expect("dotted ref has head");
    DottedRef::new(head, parts.collect())
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
/// One warning rides through, by code rather than by severity: a scope whose nets climbed past
/// each other carries `W_ROUTE_CROSS_LAYER_CLEARANCE`, which names the pairs the physical tile
/// layer has to separate and elides nothing. Anything else, a refusal that would take the scope
/// out of the IR the tests read or any other warning, is the fixture set saying something new and
/// fails rather than passing unread.
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

/// [`delayed_from_source`] plus crossing legalization, which must run
/// clean.
pub fn legalized_from_source(source: &str, edition: Edition) -> ScopedPlacementIr {
    let legalized = compile_crossing(&delayed_from_source(source, edition));
    assert!(
        legalized.diagnostics.is_empty(),
        "fixture must legalize cleanly: {:?}",
        legalized.diagnostics,
    );
    legalized.scoped
}

/// A struct whose sixteen cells all read `sig.b` off one shared trunk,
/// long enough that crossing legalization has to put buffer repeaters
/// on it — the smallest fixture the corpus has whose legalized dump
/// carries `buffer_coords`. Each cell also reads the one before it, so
/// the placement pass lays them out as one row, `2 * 16` columns long.
///
/// Shared because more than one module is built on what it does: the
/// crossing tests pin where its repeaters go, and the Placement IR
/// round-trip tests need a real dump with buffers in it to read back.
pub fn shared_bus_source() -> String {
    let mut source = String::from(
        r"
theme t:
  slot wall -> @oak_planks

struct chain size=60x5
  floor mat_slot=wall

  pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=pb at=inside.front  offset=1 y=0 -> sig.b

  logic sig.s0 = sig.a and sig.b
",
    );
    for i in 1..16 {
        writeln!(
            source,
            "  logic sig.s{i} = sig.s{prev} and sig.b",
            prev = i - 1
        )
        .expect("writing to a String cannot fail");
    }
    source.push_str(
        r"
  door id=d side=front at=center mat_slot=wall opened_by=sig.s15

  circuit region=floor void=2
",
    );
    source
}

/// Rewrite every `"stage": "<name>"` value to a fixed placeholder so two adjacent stages' dumps
/// can be byte-compared on everything *except* the tag that distinguishes them.
///
/// The routing, delay and crossing modules each assert that their pass perturbs nothing but the
/// field it writes, and must neutralise the tag identically, or a change to the key name would be
/// caught in one module and papered over in another. The separator between key and value is
/// copied through verbatim, so both the compact and the pretty spelling are handled.
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

/// Drop every `,"<key>":<unsigned int>` from compact JSON, so a stage's
/// dump can be byte-compared with the previous stage's on everything but
/// the integer field the pass adds.
pub fn strip_int_field(compact: &str, key: &str) -> String {
    let pattern = format!(",\"{key}\":");
    let mut out = String::with_capacity(compact.len());
    let mut rest = compact;
    while let Some(idx) = rest.find(&pattern) {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + pattern.len()..];
        let end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}
