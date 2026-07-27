//! Integration tests for `cairn_lang_redstone::compile_placement`.
//!
//! Locks the observable behaviours of the Placement IR slice
//! (`spec/redstone` §14.5, stage 1 of place-and-route): the
//! `examples/redstone-door.crn` happy path (per edition), 1D topological
//! coordinate assignment across a multi-cell scope, `E_ROUTE_CONGESTION`
//! when the netlist exceeds the reservation, `E_NO_CIRCUIT_REGION` when
//! a scope has cells but no `circuit region=` line (or no `size=` on
//! the enclosing scope), empty-scope elision, the JSON wire form, and
//! per-scope independence when a module carries more than one scope.

use std::path::PathBuf;

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    DiagnosticCode, EditionCell, ScopedEditionNetlistIr, compile_edition_netlist, compile_netlist,
    compile_placement, synthesize,
};

fn load_example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn edition_netlist_from_source(
    source: &str,
    edition: Edition,
) -> (ScopedEditionNetlistIr, cairn_lang_core::IntentModule) {
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
    (edition_netlist, intent)
}

/// AC1 — `examples/redstone-door.crn` compiled for Java places its lone
/// `JavaRepeaterOr` cell at the origin of its `circuit region=floor
/// void=2` reservation, and the reservation copies the enclosing struct's
/// `size=7x5` footprint. `wire_length` and `delay_ticks` are absent
/// today because Steiner routing and delay insertion are follow-up
/// passes.
#[test]
fn redstone_door_java_places_or_cell_at_origin() {
    let source = load_example("redstone-door.crn");
    let (edition_netlist, intent) = edition_netlist_from_source(&source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);
    assert!(
        out.diagnostics.is_empty(),
        "clean example must not raise placement diagnostics: {:?}",
        out.diagnostics,
    );

    let entry = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "gatehouse")
        .expect("gatehouse scope");
    let ir = &entry.ir;

    assert_eq!(ir.edition, Edition::Java);
    let region = ir.region.as_ref().expect("gatehouse has a circuit region");
    assert_eq!(region.label, "floor");
    assert_eq!(region.void, 2);
    assert_eq!(region.width, 7);
    assert_eq!(region.depth, 5);

    assert_eq!(ir.cells.len(), 1);
    let cell = &ir.cells[0];
    assert_eq!(cell.cell, EditionCell::JavaRepeaterOr);
    assert_eq!(cell.coord.x, 0);
    assert_eq!(cell.coord.y, 0);
    assert_eq!(cell.coord.z, 0);
    assert!(
        cell.wire_length().is_none(),
        "wire_length is a follow-up pass output",
    );
    assert!(
        cell.delay_ticks().is_none(),
        "delay_ticks is a follow-up pass output",
    );
}

/// AC2 — the same example compiled for Bedrock places its `BedrockTorchOr`
/// cell at the same coordinate against the same reservation; only the
/// `cell` tag and the `edition` field differ from AC1.
#[test]
fn redstone_door_bedrock_matches_java_layout_apart_from_cell_tag() {
    let source = load_example("redstone-door.crn");
    let (java_netlist, intent) = edition_netlist_from_source(&source, Edition::Java);
    let (bedrock_netlist, _) = edition_netlist_from_source(&source, Edition::Bedrock);

    let java = compile_placement(&java_netlist, &intent);
    let bedrock = compile_placement(&bedrock_netlist, &intent);

    assert_eq!(java.scoped.scopes.len(), bedrock.scoped.scopes.len());
    for (j, b) in java.scoped.scopes.iter().zip(bedrock.scoped.scopes.iter()) {
        assert_eq!(j.kind, b.kind);
        assert_eq!(j.name, b.name);
        assert_eq!(j.ir.region, b.ir.region);
        assert_eq!(j.ir.inputs, b.ir.inputs);
        assert_eq!(j.ir.outputs, b.ir.outputs);
        assert_eq!(j.ir.signal_defs, b.ir.signal_defs);
        assert_ne!(j.ir.edition, b.ir.edition);
        assert_eq!(j.ir.edition, Edition::Java);
        assert_eq!(b.ir.edition, Edition::Bedrock);
        assert_eq!(j.ir.cells.len(), b.ir.cells.len());
        for (jc, bc) in j.ir.cells.iter().zip(b.ir.cells.iter()) {
            assert_eq!(jc.coord, bc.coord);
            assert_eq!(jc.drivers, bc.drivers);
            assert_eq!(jc.span, bc.span);
            assert_ne!(jc.cell, bc.cell, "cell tag must differ per edition");
        }
    }
}

/// AC3 — a scope whose logic produces three distinct cells lays them
/// out at `x = 0, 1, 2` in the same topological order the Netlist IR
/// carried through. `y` and `z` stay `0` for every cell (1D placement).
#[test]
fn multi_cell_scope_places_in_topological_order() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct sim size=7x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b

  logic sig.and_ab   = sig.a and sig.b
  logic sig.or_ab    = sig.a or sig.b
  logic sig.combined = sig.and_ab and sig.or_ab

  door id=d side=front at=center mat_slot=wall opened_by=sig.combined

  circuit region=floor void=3
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);
    assert!(
        out.diagnostics.is_empty(),
        "fixture must not raise diagnostics: {:?}",
        out.diagnostics,
    );

    let ir = &out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "sim")
        .expect("sim scope")
        .ir;
    assert_eq!(ir.cells.len(), 3);
    for (i, cell) in ir.cells.iter().enumerate() {
        let expected = u32::try_from(i).expect("small index");
        assert_eq!(
            cell.coord.x, expected,
            "cell[{i}] should sit at x={expected}",
        );
        assert_eq!(cell.coord.y, 0);
        assert_eq!(cell.coord.z, 0);
    }
}

/// AC4 — a scope whose synthesised netlist needs more area than its
/// reservation offers fires `E_ROUTE_CONGESTION`, anchors the primary
/// span at the `circuit region=` line, quotes the ratio in the primary
/// prose per `spec/redstone` §14.5, carries the three-fix footer, and
/// drops the failed scope so a downstream pass cannot consume a
/// partially-placed layout.
#[test]
fn congestion_fires_route_congestion_and_elides_scope() {
    // 3 cells × 4 blocks each = 12 required blocks vs 3 × 3 × 1 = 9
    // reserved blocks — ~1.3× over budget, well under the ~3.2× the
    // spec example shows so a future footprint tweak has headroom.
    let source = r"
theme t:
  slot wall -> @oak_planks

struct tiny size=3x3
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b

  logic sig.and_ab   = sig.a and sig.b
  logic sig.or_ab    = sig.a or sig.b
  logic sig.combined = sig.and_ab and sig.or_ab

  door id=d side=front at=center mat_slot=wall opened_by=sig.combined

  circuit region=floor void=1
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    let congestion: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::RouteCongestion)
        .collect();
    assert_eq!(
        congestion.len(),
        1,
        "expected exactly one E_ROUTE_CONGESTION, got {:?}",
        out.diagnostics,
    );
    let d = congestion[0];
    assert_eq!(d.severity, Severity::Error);
    assert!(
        d.primary.contains("~1.3x"),
        "primary should quote the ratio, got {:?}",
        d.primary,
    );
    assert!(
        d.primary.contains("void=1"),
        "primary should quote the reservation shape, got {:?}",
        d.primary,
    );
    assert!(
        d.primary.contains("3x3") || d.primary.contains("3×3"),
        "primary should quote the region footprint, got {:?}",
        d.primary,
    );
    // The primary span must anchor to the `circuit region=` line so an
    // LSP quick-fix or editor jump lands on the reservation declaration,
    // not the first cell's source offset. `spec/redstone` §14.5's
    // example diagnostic anchors at the region.
    let source_at_span = &source[d.span.clone()];
    assert!(
        source_at_span.starts_with("circuit region="),
        "congestion span must anchor to the `circuit region=` line, got {source_at_span:?}",
    );
    let footer = d
        .notes
        .iter()
        .find(|n| n.span.is_none())
        .expect("congestion has a fix footer");
    // Spec §14.5's canonical fix triple: increase `void`, enlarge
    // region, or split into multiple `circuit` blocks.
    for phrase in ["increase", "void", "enlarge", "region", "split", "circuit"] {
        assert!(
            footer.message.contains(phrase),
            "footer should carry the spec §14.5 triple (missing {phrase:?}), got {:?}",
            footer.message,
        );
    }

    assert!(
        out.scoped.scopes.iter().all(|e| e.name != "tiny"),
        "failed scope must be elided from the output",
    );
}

/// AC5 — a scope with cells to place but no `circuit region=` line
/// fires `E_NO_CIRCUIT_REGION`. Same elision policy as AC4: the failed
/// scope drops out so a caller cannot mistake a partial layout for a
/// finished one.
#[test]
fn missing_region_fires_no_circuit_region() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct nomarker size=7x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b

  logic sig.open = sig.a or sig.b

  door id=d side=front at=center mat_slot=wall opened_by=sig.open
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    let missing: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::NoCircuitRegion)
        .collect();
    assert_eq!(
        missing.len(),
        1,
        "expected exactly one E_NO_CIRCUIT_REGION, got {:?}",
        out.diagnostics,
    );
    assert_eq!(missing[0].severity, Severity::Error);
    assert!(
        out.scoped.scopes.iter().all(|e| e.name != "nomarker"),
        "failed scope must be elided from the output",
    );
}

/// AC6 — a scope whose Edition Netlist IR was empty stays elided in
/// the Placement IR (nothing to place, nothing to say).
#[test]
fn empty_edition_netlist_produces_no_placement_entry() {
    let module = parse("").expect("empty parses");
    let intent = lower(&module);
    let scoped = ScopedEditionNetlistIr::new();
    let out = compile_placement(&scoped, &intent);
    assert!(out.diagnostics.is_empty());
    assert!(out.scoped.is_empty());
    assert!(out.scoped.scopes.is_empty());
}

/// AC7 — the JSON dump carries `edition`, a `region` object with the
/// four reservation fields, a per-cell `coord` object, and the
/// `stage` tag naming the pass that produced it. `wire_length` and
/// `delay_ticks` are absent (the phase this stage stamps carries
/// neither) so the wire form does not carry future-only fields today.
#[test]
fn json_dump_carries_stage_region_and_coord_and_omits_reserved_fields() {
    let source = load_example("redstone-door.crn");
    let (edition_netlist, intent) = edition_netlist_from_source(&source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    let json = serde_json::to_string(&out.scoped).expect("serialise");
    assert!(
        json.contains("\"edition\":\"java\""),
        "edition tag missing: {json}",
    );
    assert!(
        json.contains("\"stage\":\"placement\""),
        "every placed cell must carry the placement stage tag: {json}",
    );
    assert!(
        json.contains("\"region\":{"),
        "region object missing: {json}",
    );
    assert!(
        json.contains("\"label\":\"floor\""),
        "region label missing: {json}",
    );
    assert!(json.contains("\"void\":2"), "region void missing: {json}");
    assert!(json.contains("\"coord\":{"), "coord object missing: {json}");
    assert!(
        !json.contains("\"wire_length\""),
        "wire_length must be elided today: {json}",
    );
    assert!(
        !json.contains("\"delay_ticks\""),
        "delay_ticks must be elided today: {json}",
    );
}

/// `AC5b` — a scope that DECLARED a `circuit region=` line but whose
/// enclosing struct is missing a `size=WxH` header cannot be placed:
/// there is no reservation footprint to budget against, so the pass
/// falls back to `E_NO_CIRCUIT_REGION`. The primary message must
/// name the missing-`size=` cause so an author who sees the error on
/// a source line that clearly declares `circuit region=...` can
/// still identify the fix.
#[test]
fn missing_size_falls_through_to_no_circuit_region() {
    let source = r"
theme t:
  slot wall -> @oak_planks

def gadget
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b
  logic sig.open = sig.a or sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.open
  circuit region=floor void=2
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    let missing: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::NoCircuitRegion)
        .collect();
    assert_eq!(
        missing.len(),
        1,
        "expected exactly one E_NO_CIRCUIT_REGION for the size-less def, got {:?}",
        out.diagnostics,
    );
    assert!(
        missing[0].primary.contains("size=")
            || missing[0].notes.iter().any(|n| n.message.contains("size=")),
        "diagnostic must name the missing `size=` cause, got primary={:?} notes={:?}",
        missing[0].primary,
        missing[0].notes,
    );
}

/// `AC5c` — a scope that declared `circuit region=floor void=0` (an
/// explicitly malformed reservation the parser rejects as unusable)
/// must not silently look like "no reservation declared". The
/// `E_NO_CIRCUIT_REGION` message therefore has to enumerate the
/// malformed-`void=` cause alongside the missing-line and missing-`size=`
/// cases, so an author staring at an obvious `void=0` on the line above
/// can still connect the error to their input.
#[test]
fn void_zero_surfaces_no_circuit_region_with_malformed_hint() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct simple size=5x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b
  logic sig.open = sig.a or sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.open
  circuit region=floor void=0
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    let d = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::NoCircuitRegion)
        .expect("void=0 must surface as E_NO_CIRCUIT_REGION");
    assert!(
        d.primary.contains("malformed") || d.primary.contains("void"),
        "diagnostic must name the malformed-`void=` cause, got {:?}",
        d.primary,
    );
}

/// A scope whose Edition Netlist IR carries inputs and outputs but no
/// cells — a `pressure_plate -> sig.a` bound directly to `door
/// opened_by=sig.a` with no `logic` line in between — has nothing to
/// place. The pass elides such scopes cleanly (no panic, no diagnostic,
/// no orphan `PlacementIr` entry), so the routing pass can re-scan the
/// Edition Netlist IR for these no-cell wires without a broken
/// intermediate state.
#[test]
fn identity_wire_scope_is_elided_cleanly() {
    let source = r"
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct wire size=5x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  door id=d side=front at=center mat_slot=door opened_by=sig.a
  circuit region=floor void=2
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    assert!(
        out.diagnostics.is_empty(),
        "identity-wire scope must not raise diagnostics, got {:?}",
        out.diagnostics,
    );
    assert!(
        out.scoped.scopes.iter().all(|e| e.name != "wire"),
        "identity-wire scope must be elided from the Placement IR",
    );
}

/// A scope with two `circuit region=` lines silently keeps the first
/// and drops the rest — the v1 policy. The follow-up routing PR may
/// warn or route into every reservation, but today's placement pass
/// must not fail loud on duplicates because the block-array pass
/// already accepts them without complaint.
#[test]
fn duplicate_circuit_region_first_wins_silently() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct dup size=7x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b
  logic sig.open = sig.a or sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.open
  circuit region=floor void=2
  circuit region=basement void=3
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    assert!(
        out.diagnostics.is_empty(),
        "v1 must not raise diagnostics on duplicate `circuit region=` lines, got {:?}",
        out.diagnostics,
    );
    let ir = &out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "dup")
        .expect("dup scope survives")
        .ir;
    let region = ir.region.as_ref().expect("dup has a reservation");
    assert_eq!(
        region.label, "floor",
        "first `circuit region=` line must win, got label={:?}",
        region.label,
    );
    assert_eq!(region.void, 2);
}

/// AC8 — two non-empty scopes are placed independently: each looks up
/// its own reservation, one scope's failure does not poison the other,
/// and per-scope contents do not bleed across.
#[test]
fn multiple_scopes_place_independently() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct alpha size=7x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  logic sig.na = not sig.a
  door id=d side=front at=center mat_slot=wall opened_by=sig.na
  circuit region=floor void=2

struct beta size=7x5
  floor mat_slot=wall
  pressure_plate id=q at=front.outside offset=0 y=0 -> sig.b1
  pressure_plate id=r at=inside.front  offset=0 y=0 -> sig.b2
  logic sig.both = sig.b1 and sig.b2
  door id=e side=front at=center mat_slot=wall opened_by=sig.both
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Bedrock);
    let out = compile_placement(&edition_netlist, &intent);

    // alpha placed cleanly.
    let alpha = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "alpha")
        .expect("alpha scope survives");
    assert_eq!(alpha.ir.cells[0].cell, EditionCell::BedrockInverterTorch);
    assert_eq!(alpha.ir.cells[0].coord.x, 0);

    // beta has cells but no circuit region → elided with a diagnostic.
    assert!(
        out.scoped.scopes.iter().all(|e| e.name != "beta"),
        "beta must elide because it declares no circuit region",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::NoCircuitRegion),
        "beta's missing reservation must surface as E_NO_CIRCUIT_REGION: {:?}",
        out.diagnostics,
    );
}
