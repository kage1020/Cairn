//! Fixture builders and JSON helpers shared by the redstone integration tests.

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

/// Reads `examples/<name>` from the workspace root.
pub fn load_example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

pub fn synth_source(source: &str) -> SynthOutput {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    synthesize(&intent)
}

/// The 1-based line holding byte `offset`.
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

/// `sig.open` as a [`DottedRef`].
pub fn sig(name: &str) -> DottedRef {
    let mut parts = name.split('.').map(str::to_owned);
    let head = parts.next().expect("dotted ref has head");
    DottedRef::new(head, parts.collect())
}

/// Compiles `source` to the Edition Netlist IR, failing on any front-end error.
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

/// [`edition_netlist_from_source`] plus placement, which must run clean.
pub fn placement_from_source(source: &str, edition: Edition) -> ScopedPlacementIr {
    let (edition_netlist, intent) = edition_netlist_from_source(source, edition);
    let placement = compile_placement(&edition_netlist, &intent);
    assert!(
        placement.diagnostics.is_empty(),
        "fixture must place cleanly: {:?}",
        placement.diagnostics,
    );
    placement.scoped
}

/// [`placement_from_source`] plus routing, allowing only `W_ROUTE_CROSS_LAYER_CLEARANCE`.
pub fn routed_from_source(source: &str, edition: Edition) -> ScopedPlacementIr {
    let routing = compile_routing(&placement_from_source(source, edition));
    assert!(
        routing
            .diagnostics
            .iter()
            .all(|d| d.code == DiagnosticCode::RouteCrossLayerClearance),
        "fixture must route with only the cross-layer advisory: {:?}",
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

/// [`delayed_from_source`] plus crossing legalization, which must run clean.
pub fn legalized_from_source(source: &str, edition: Edition) -> ScopedPlacementIr {
    let legalized = compile_crossing(&delayed_from_source(source, edition));
    assert!(
        legalized.diagnostics.is_empty(),
        "fixture must legalize cleanly: {:?}",
        legalized.diagnostics,
    );
    legalized.scoped
}

/// Sixteen chained cells reading `sig.b` off one trunk, long enough to need buffer repeaters.
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

/// One cell driving a door across a 40-wide floor, on a wire long enough to need buffer repeaters.
pub const LONG_OUTWARD_WIRE_SOURCE: &str = "\
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct reach size=40x6
  floor mat_slot=wall
  door id=front side=front at=center mat_slot=door
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=1 y=0 -> sig.b
  logic sig.c = sig.a or sig.b
  door[id=front] opened_by=sig.c
  circuit region=floor void=3
";

/// A door opened straight off a pressure plate across a 40-wide floor: a wire and no cell.
pub const IDENTITY_WIRE_SOURCE: &str = "\
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct pass size=40x6
  floor mat_slot=wall
  door id=d side=front at=center mat_slot=door
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  door[id=d] opened_by=sig.a
  circuit region=floor void=2
";

/// Replaces every `"stage"` value with a placeholder, so adjacent stages' dumps compare equal.
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

/// Drops every `,"<key>":<unsigned int>` from compact JSON.
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
