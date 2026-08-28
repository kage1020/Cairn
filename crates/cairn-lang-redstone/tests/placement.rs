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
            .all(|d| d.severity() != Severity::Error),
        "fixture must synth cleanly: {:?}",
        synth.diagnostics,
    );
    let netlist = compile_netlist(&synth.scoped);
    let edition_netlist = compile_edition_netlist(&netlist, edition);
    (edition_netlist, intent)
}

/// AC1 — `examples/redstone-door.crn` compiled for Java places its lone
/// `JavaRepeaterOr` cell one column in from the pad column of its
/// `circuit region=floor void=2` reservation, and the reservation
/// copies the enclosing struct's `size=7x5` footprint. `wire_length`
/// and `delay_ticks` are absent today because Steiner routing and
/// delay insertion are follow-up passes.
#[test]
fn redstone_door_java_places_or_cell_beside_the_pad_column() {
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
    assert_eq!(cell.coord.x, 1);
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
/// out at `x = 1, 3, 5` in the same topological order the Netlist IR
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
    // `1 + 2i`: one column in from the pad column, one clear column
    // between each pair. Spelled out rather than computed, so the
    // convention is written somewhere other than in the pass that
    // implements it.
    for (i, (cell, expected)) in ir.cells.iter().zip([1u32, 3, 5]).enumerate() {
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
    assert_eq!(d.severity(), Severity::Error);
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
    assert_eq!(missing[0].severity(), Severity::Error);
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
/// cells — a `pressure_plate -> sig.a` bound straight to `door
/// opened_by=sig.a`, which `spec/redstone` §14.2 permits — still has a
/// layout: the wire from the sensor pad to the actuator pad. It used to
/// be dropped here, and `--stage placement` onward printed `[]` at exit
/// 0 for a scope the netlist stage had just described in full.
#[test]
fn identity_wire_scope_places_its_actuator_pad() {
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
    let scope = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "wire")
        .expect("identity-wire scope must reach the Placement IR");
    assert!(scope.ir.cells.is_empty(), "there is no logic to place");
    assert_eq!(scope.ir.inputs.len(), 1, "the sensor survives");
    let region = scope.ir.region.as_ref().expect("the reservation survives");
    let output = scope.ir.outputs.first().expect("the actuator is placed");
    assert_eq!(
        output.pad.x,
        region.width - 1,
        "the pad sits on the region's right edge",
    );
    assert!(
        !scope.ir.signal_defs.is_empty(),
        "the signal table survives so a consumer can join the pad back to `sig.a`",
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
    assert_eq!(alpha.ir.cells[0].coord.x, 1);

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

// ---- the row has to hold the cells (`spec/redstone` §14.5) ----
//
// The v1 layout stamps `x = 1 + 2i`, so the reservation's *width* is
// the resource the cells consume — twice their count and one column
// more — and the area budget cannot see it. A `size=2x8` scope
// reserving `void=3` offers 48 cells' worth of volume and a row two
// columns long, so a three-cell netlist passes the volume test and
// overruns the row. Every later pass reads these coordinates, and
// `output_pad` sits at `width - 1`, so a cell past the edge drives a
// pad standing behind it.

use std::fmt::Write as _;

/// A scope whose netlist synthesises exactly `cells` cells inside a
/// `size={width}x{depth}` footprint reserving `void`.
///
/// Each `logic` line becomes one cell: the chain is written in
/// dependency order and every link takes the previous signal plus a
/// second sensor, which is the shape that keeps one cell per line.
fn source_with_cells(cells: usize, width: u32, depth: u32, void: u32) -> String {
    let mut source = String::from(
        "theme t:\n  \
         slot wall -> @oak_planks\n  \
         slot door -> @oak_door\n\n",
    );
    let _ = writeln!(source, "struct gen size={width}x{depth}");
    source.push_str(
        "  floor mat_slot=wall\n  \
         door id=front side=front at=center mat_slot=door\n  \
         pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n  \
         pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b\n",
    );
    let mut previous = String::from("sig.a");
    for index in 0..cells {
        let op = if index % 2 == 0 { "or" } else { "and" };
        let _ = writeln!(source, "  logic sig.c{index} = {previous} {op} sig.b");
        previous = format!("sig.c{index}");
    }
    let _ = writeln!(source, "  door[id=front] opened_by={previous}");
    let _ = writeln!(source, "  circuit region=floor void={void}");
    source
}

fn placement_of(source: &str) -> cairn_lang_redstone::PlacementOutput {
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    compile_placement(&edition_netlist, &intent)
}

/// The reported repro: `size=2x8 void=3` with three cells. The volume
/// test passes (12 <= 48) and the row, which wants six columns, is
/// overrun by four.
#[test]
fn a_netlist_wider_than_the_reserved_row_is_refused() {
    let out = placement_of(&source_with_cells(3, 2, 8, 3));

    assert!(
        out.scoped.scopes.is_empty(),
        "a scope that does not fit its row must not reach the Placement IR: {:?}",
        out.scoped.scopes,
    );
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .expect("the overrun must surface as E_ROUTE_CONGESTION");
    assert!(
        diagnostic.primary.contains("only 2 wide"),
        "the refusal must name the row, not an area ratio: {}",
        diagnostic.primary,
    );
    assert_eq!(diagnostic.severity(), Severity::Error);
}

/// A region wide enough for the cells and not for the spacing between
/// them is refused here, not two passes later.
///
/// Three cells want seven columns; four is more than enough to stand
/// them in and three short of the row they are laid in. If the check
/// counted cells rather than columns this would pass placement, put
/// the last cell outside the reservation, and surface at stage 2 as a
/// sink no route can reach — of a coord no route could enter, with a
/// message about components and dust that names nothing true.
#[test]
fn a_row_wide_enough_for_the_cells_and_not_the_spacing_is_refused() {
    let out = placement_of(&source_with_cells(3, 4, 8, 3));

    assert!(
        out.scoped.scopes.is_empty(),
        "a scope that does not fit its row must not reach the Placement IR: {:?}",
        out.scoped.scopes,
    );
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .expect("the overrun must surface as E_ROUTE_CONGESTION");
    assert!(
        diagnostic
            .primary
            .contains("needs 7 columns for a row of 3 cells"),
        "the refusal must name the columns the row wants, not the cells it \
         holds: {}",
        diagnostic.primary,
    );
}

/// The exact-fit boundary, with its neighbour one column down.
///
/// A row of `n` cells wants `2n + 1` columns: one for each cell, one
/// beside each, and one past the last so the cell at the end of the row
/// is not left with the actuator-pad column on one side and the edge of
/// the reservation on the other. At `2n` that cell has one free plane
/// neighbour, and the layout is refused two stages later under `void=1`
/// or climbs and pays for it above — measured on a four-cell chain:
/// `2n` refuses at `void=1`, and at `void=3` reports `wire_length` 13
/// for the last cell where `2n + 1` reports 11.
///
/// Both sides are here rather than only the accepting one, because the
/// number the check compares against is only pinned by a pair that
/// straddles it: a threshold one column out in either direction passes
/// a test that names one side alone. The depth guard is what keeps the
/// actuator pad off the cell row, and
/// `a_region_one_row_deep_cannot_hold_a_pad_beside_its_cells` covers
/// the case where it does not.
#[test]
fn a_row_of_twice_the_cell_count_plus_one_places_every_cell() {
    assert!(
        !placement_of(&source_with_cells(4, 8, 8, 3))
            .diagnostics
            .is_empty(),
        "one column short of the row is refused",
    );

    let out = placement_of(&source_with_cells(4, 9, 8, 3));

    assert!(
        out.diagnostics.is_empty(),
        "an exact fit must not be refused: {:?}",
        out.diagnostics,
    );
    let scope = out.scoped.scopes.first().expect("the scope places");
    assert_eq!(scope.ir.cells.len(), 4);
    assert_eq!(
        scope.ir.cells.last().expect("last cell").coord.x,
        7,
        "the last cell must stand in the final column of the row, one short \
         of the pad column",
    );
}

/// One over the boundary, holding everything else fixed against the
/// exact-fit case above.
#[test]
fn one_cell_more_than_the_row_holds_is_refused() {
    let out = placement_of(&source_with_cells(5, 4, 8, 3));

    assert!(out.scoped.scopes.is_empty());
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .expect("the overrun must surface");
    assert!(
        diagnostic.primary.contains("only 4 wide"),
        "one over the row must be refused as a row, not as an area: {}",
        diagnostic.primary,
    );
}

/// The two ways of not fitting are separate resources: this netlist has
/// a row long enough and a volume too small, and must still be told
/// about the volume.
#[test]
fn a_netlist_short_of_volume_but_not_of_row_still_reports_the_area() {
    // width 40 holds 11 cells in a row; `40 * 1 * 1` reserves 40 cells'
    // worth of volume against the 44 the estimate asks for.
    let out = placement_of(&source_with_cells(11, 40, 1, 1));

    let diagnostic = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .expect("the shortfall must surface");
    assert!(
        diagnostic.primary.contains("reserved area"),
        "a volume shortfall must be explained as one: {}",
        diagnostic.primary,
    );
}

/// A sensor nothing reads is not a layout. `EditionNetlistIr::is_empty`
/// wants all three of inputs / outputs / cells empty, so keying elision
/// on it made a lone `pressure_plate -> sig.step` claim a reservation
/// for nothing to place — and refuse at byte 0 when there was none. The
/// predicate is the one routing, delay, and crossing already use.
#[test]
fn a_sensor_nothing_reads_is_not_something_to_place() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct sens size=8x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.step
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    assert!(
        out.diagnostics.is_empty(),
        "a scope with nothing to place must not be refused: {:?}",
        out.diagnostics,
    );
    assert!(out.scoped.scopes.is_empty(), "and must not be emitted");
}

/// The pads need rows of their own: they step along `z` from 1 and
/// saturate at `depth - 1`, and the cells hold `z = 0`. At `depth == 1`
/// the saturation drops the actuator pad onto the last cell, which is
/// the very collision the row check reasons about — so the row check
/// cannot be the whole of it.
#[test]
fn a_region_one_row_deep_cannot_hold_a_pad_beside_its_cells() {
    let out = placement_of(&source_with_cells(4, 9, 1, 4));

    assert!(out.scoped.scopes.is_empty());
    let diagnostic = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .expect("the shortfall must surface");
    assert!(
        diagnostic.primary.contains("only 1 deep"),
        "the refusal must name the depth: {}",
        diagnostic.primary,
    );
}

/// Pads saturate onto each other one row before they reach the cells,
/// so the guard is about the count rather than about `depth == 1`.
#[test]
fn more_actuators_than_rows_is_refused_before_their_pads_collide() {
    let source = r"
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct three size=8x3
  floor mat_slot=wall
  door id=d1 side=front at=center mat_slot=door
  door id=d2 side=back  at=center mat_slot=door
  door id=d3 side=left  at=center mat_slot=door
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front  offset=0 y=0 -> sig.b
  logic sig.f = sig.a and sig.b
  door[id=d1] opened_by=sig.f
  door[id=d2] opened_by=sig.f
  door[id=d3] opened_by=sig.f
  circuit region=floor void=2
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    assert!(
        out.scoped.scopes.is_empty(),
        "three actuators do not fit three rows beside a cell row",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::RouteCongestion),
    );
}

/// The `E_NO_CIRCUIT_REGION` span for a scope with no cells. Deleting
/// the fallback leaves it at byte 0, which is a position in the file
/// that has nothing to do with the binding that needs the reservation.
#[test]
fn a_cell_less_scope_reports_its_missing_region_at_the_actuator() {
    let source = r"
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct wire size=5x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  door id=d side=front at=center mat_slot=door opened_by=sig.a
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    let diagnostic = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::NoCircuitRegion)
        .expect("a scope with a pad to place needs a reservation");
    assert!(
        diagnostic.span.start > 0,
        "the finding must point at the actuator binding, not at byte 0",
    );
    assert!(
        source[diagnostic.span.start..diagnostic.span.end].contains("sig.a"),
        "the span must cover the binding that needs the reservation, got {:?}",
        &source[diagnostic.span.start..diagnostic.span.end],
    );
}
