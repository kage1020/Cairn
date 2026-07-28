//! Integration tests for `cairn_lang_redstone::compile_routing`.
//!
//! Locks the observable behaviours of the Steiner-routing slice
//! (`spec/redstone` §14.5, stage 2 of place-and-route): the
//! `examples/redstone-door.crn` happy path (per edition), single-cell
//! `wire_length` attribution, multi-cell cascades with L-shape nets
//! whose `wire_length` values are pinned to exact Manhattan sums,
//! `E_ROUTE_CONGESTION` when the post-routing footprint exceeds the
//! reservation, pass-through of scopes elided by upstream stages,
//! the JSON wire form growing a `wire_length` field, and per-scope
//! independence when a module carries more than one scope.

use std::path::PathBuf;

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    DiagnosticCode, ScopedPlacementIr, compile_edition_netlist, compile_netlist, compile_placement,
    compile_routing, synthesize,
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

fn placement_from_source(source: &str, edition: Edition) -> ScopedPlacementIr {
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
        "fixture must place cleanly (routing tests are downstream of placement): {:?}",
        placement.diagnostics,
    );
    placement.scoped
}

/// AC1 — `examples/redstone-door.crn` compiled for Java routes its
/// sole `JavaRepeaterOr` cell with `wire_length = Some(3)`: the sum of
/// Manhattan distances from each input pad (v1 convention: `(0, 0,
/// 1+i)`) to the cell coord `(0, 0, 0)` — one step for `sig.step`,
/// two for `sig.exit`. `delay_ticks` stays `None` (routing does not
/// insert delay per `spec/redstone` §14.4; that is stage 3).
#[test]
fn redstone_door_java_fills_wire_length_from_input_pads() {
    let source = load_example("redstone-door.crn");
    let placement = placement_from_source(&source, Edition::Java);
    let routed = compile_routing(&placement);
    assert!(
        routed.diagnostics.is_empty(),
        "clean example must not raise routing diagnostics: {:?}",
        routed.diagnostics,
    );

    let entry = routed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "gatehouse")
        .expect("gatehouse scope");
    let ir = &entry.ir;
    assert_eq!(ir.cells.len(), 1);
    let cell = &ir.cells[0];
    assert_eq!(
        cell.wire_length(),
        Some(3),
        "wire_length must be Manhattan(step→cell) + Manhattan(exit→cell) = 1 + 2 = 3",
    );
    assert!(
        cell.delay_ticks().is_none(),
        "delay_ticks stays None: Stage 3 (delay insertion) is a follow-up",
    );
}

/// AC2 — the same example compiled for Bedrock produces the same
/// `wire_length` because the pad and cell coordinates are
/// edition-independent by the placement-pass layout contract. Only
/// the cell tag and edition field differ from AC1.
#[test]
fn redstone_door_bedrock_matches_java_wire_length() {
    let source = load_example("redstone-door.crn");
    let java = compile_routing(&placement_from_source(&source, Edition::Java));
    let bedrock = compile_routing(&placement_from_source(&source, Edition::Bedrock));

    assert_eq!(java.scoped.scopes.len(), bedrock.scoped.scopes.len());
    for (j, b) in java.scoped.scopes.iter().zip(bedrock.scoped.scopes.iter()) {
        assert_eq!(j.name, b.name);
        assert_eq!(j.ir.cells.len(), b.ir.cells.len());
        for (jc, bc) in j.ir.cells.iter().zip(b.ir.cells.iter()) {
            assert_eq!(jc.coord, bc.coord);
            assert_eq!(
                jc.wire_length(),
                bc.wire_length(),
                "wire_length is edition-independent by construction",
            );
            assert_ne!(jc.cell, bc.cell, "cell tag differs per edition");
        }
    }
}

/// AC3 — a scope whose logic produces three cascaded cells fills
/// every cell's `wire_length` with a pinned Manhattan sum, so a
/// regression in either the input-pad coordinate convention or the
/// per-driver attribution walk trips this test. Cell placement lays
/// cells at `x = i, y = 0, z = 0`, and input pads land at
/// `(0, 0, 1+i)`, so the exact sums are:
///
/// - cell[0] `sig.and_ab = sig.a and sig.b`: `M((0,0,1)→(0,0,0)) +
///   M((0,0,2)→(0,0,0)) = 1 + 2 = 3`.
/// - cell[1] `sig.or_ab  = sig.a or sig.b`:  same source pair,
///   different cell coord: `M((0,0,1)→(1,0,0)) + M((0,0,2)→(1,0,0))
///   = 2 + 3 = 5`. This is the L-shape case (dx=1, dz≥1) that the
///   `l_shape_path` axis order regression would otherwise sneak past.
/// - cell[2] `sig.combined = sig.and_ab and sig.or_ab`: cell-to-cell
///   drivers: `M((0,0,0)→(2,0,0)) + M((1,0,0)→(2,0,0)) = 2 + 1 = 3`.
///
/// `delay_ticks` stays `None` at every cell.
#[test]
fn multi_cell_scope_pins_wire_length_including_l_shape_nets() {
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
    let placement = placement_from_source(source, Edition::Java);
    let routed = compile_routing(&placement);
    assert!(
        routed.diagnostics.is_empty(),
        "clean fixture must not raise routing diagnostics: {:?}",
        routed.diagnostics,
    );

    let entry = routed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "sim")
        .expect("sim scope");
    assert_eq!(entry.ir.cells.len(), 3);
    // cell[0] at (0,0,0), input pads at (0,0,1) and (0,0,2).
    assert_eq!(entry.ir.cells[0].wire_length(), Some(3));
    // cell[1] at (1,0,0), same input pad pair — L-shape drivers.
    assert_eq!(entry.ir.cells[1].wire_length(), Some(5));
    // cell[2] at (2,0,0), driven by cell[0]=(0,0,0) and cell[1]=(1,0,0).
    assert_eq!(entry.ir.cells[2].wire_length(), Some(3));
    for cell in &entry.ir.cells {
        assert!(
            cell.delay_ticks().is_none(),
            "delay_ticks must not appear (stage 3 is future work), got {:?}",
            cell.delay_ticks(),
        );
    }
}

/// AC4 — a scope whose synthesised netlist packs cells to the cell-only
/// budget boundary (`cells.len() * CELL_FOOTPRINT == reserved_area`)
/// passes placement but fires `E_ROUTE_CONGESTION` once the routing
/// pass adds any wire on top. The reservation footprint fits the
/// placement-pass pessimistic estimate exactly, so the routing pass's
/// stricter post-routing accounting is what promotes this to a
/// failure. The primary quotes the ratio, the scope name, and the
/// reservation shape in the "routed netlist for struct `<name>`
/// occupies ~N.Mx..." form, and the failed scope is elided from the
/// output so a downstream pass cannot silently consume a partial
/// layout.
#[test]
fn post_routing_congestion_fires_route_congestion_and_elides_scope() {
    // 3 cells × 4 blocks = 12 required cell area vs 4 × 3 × 1 = 12
    // reserved blocks — placement passes at the boundary, routing
    // then adds wire and overflows.
    let source = r"
theme t:
  slot wall -> @oak_planks

struct pack size=4x3
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b

  logic sig.and_ab   = sig.a and sig.b
  logic sig.or_ab    = sig.a or sig.b
  logic sig.combined = sig.and_ab and sig.or_ab

  door id=d side=front at=center mat_slot=wall opened_by=sig.combined

  circuit region=floor void=1
";
    let placement = placement_from_source(source, Edition::Java);
    let routed = compile_routing(&placement);

    let congestion: Vec<_> = routed
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::RouteCongestion)
        .collect();
    assert_eq!(
        congestion.len(),
        1,
        "expected exactly one E_ROUTE_CONGESTION, got {:?}",
        routed.diagnostics,
    );
    let d = congestion[0];
    assert_eq!(d.severity, Severity::Error);
    assert!(
        d.primary.starts_with("routed netlist for "),
        "primary should mark the routing-side origin, got {:?}",
        d.primary,
    );
    assert!(
        d.primary.contains("struct `pack`"),
        "primary should name the failed scope, got {:?}",
        d.primary,
    );
    assert!(
        d.primary.contains("void=1"),
        "primary should quote the reservation shape, got {:?}",
        d.primary,
    );
    assert!(
        d.primary.contains("4x3"),
        "primary should quote the region footprint, got {:?}",
        d.primary,
    );
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
    for phrase in ["increase", "void", "enlarge", "region", "split", "circuit"] {
        assert!(
            footer.message.contains(phrase),
            "footer should carry the spec §14.5 triple (missing {phrase:?}), got {:?}",
            footer.message,
        );
    }
    assert!(
        routed.scoped.scopes.iter().all(|e| e.name != "pack"),
        "failed scope must be elided from the routing output",
    );
}

/// AC5 — an empty `ScopedPlacementIr` (no scopes) routes to an empty
/// `ScopedPlacementIr` with zero diagnostics: routing is a
/// per-scope map, so no scopes ⇒ no work ⇒ no findings.
#[test]
fn empty_placement_ir_produces_empty_routing_output() {
    let scoped = ScopedPlacementIr::new();
    let routed = compile_routing(&scoped);
    assert!(routed.diagnostics.is_empty());
    assert!(routed.scoped.scopes.is_empty());
}

/// AC6 — a scope whose Placement IR carries inputs and outputs but
/// no cells (a pressure-plate wired straight to a door via `sig.a`)
/// is elided by `compile_placement` already, so `compile_routing`
/// never sees it. The routing output must still be diagnostic-clean.
#[test]
fn identity_wire_scope_is_elided_before_routing() {
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
    let placement = placement_from_source(source, Edition::Java);
    let routed = compile_routing(&placement);
    assert!(
        routed.diagnostics.is_empty(),
        "identity-wire scope must not raise routing diagnostics, got {:?}",
        routed.diagnostics,
    );
    assert!(
        routed.scoped.scopes.iter().all(|e| e.name != "wire"),
        "identity-wire scope must stay elided in the routed output",
    );
}

/// AC7 — the JSON dump of a routed Placement IR carries a
/// `"wire_length": N` field on every cell object, while `delay_ticks`
/// stays elided (`skip_serializing_if = "Option::is_none"`). Pins the
/// wire-form contract that distinguishes `--stage placement` (no
/// `wire_length`) from `--stage route` (`wire_length` present)
/// output.
#[test]
fn json_dump_carries_wire_length_and_omits_delay_ticks() {
    let source = load_example("redstone-door.crn");
    let placement = placement_from_source(&source, Edition::Java);
    let routed = compile_routing(&placement);

    let json = serde_json::to_string(&routed.scoped).expect("serialise");
    assert!(
        json.contains("\"stage\":\"route\""),
        "every routed cell must carry the route stage tag: {json}",
    );
    assert!(
        json.contains("\"wire_length\":"),
        "wire_length must appear in routed JSON: {json}",
    );
    assert!(
        !json.contains("\"delay_ticks\""),
        "delay_ticks must be elided at this stage: {json}",
    );
    // Sanity: the region and coord shapes carried across from
    // placement stay intact after routing.
    assert!(
        json.contains("\"region\":{"),
        "region object should still be present: {json}",
    );
    assert!(
        json.contains("\"coord\":{"),
        "coord object should still be present: {json}",
    );
}

/// `AC7b` — routing must not perturb any Placement IR field other than
/// `wire_length` and the `stage` tag. Serialise the placement and
/// routing outputs to compact JSON, strip the routing side's
/// `,"wire_length":<int>` entries, normalise both sides' stage tags,
/// and byte-compare the remainder. Pins the "field-write only"
/// wire-form contract the pass docstring on `PlacedCellNode` declares:
/// a downstream JSON consumer that inspects `--stage placement` output
/// today should see byte-identical bytes from `--stage route` once
/// `wire_length` and the tag are peeled off, so field reordering,
/// added/removed fields, or key-name typos in unrelated structs trip
/// here even when the JSON parses equivalently.
#[test]
fn routing_leaves_placement_fields_byte_identical_apart_from_wire_length_and_stage() {
    let source = load_example("redstone-door.crn");
    let placement = placement_from_source(&source, Edition::Java);
    let placement_json = serde_json::to_string(&placement).expect("serialise placement");

    let routed = compile_routing(&placement);
    assert!(
        routed.diagnostics.is_empty(),
        "clean fixture must route without diagnostics: {:?}",
        routed.diagnostics,
    );
    let routed_json = serde_json::to_string(&routed.scoped).expect("serialise routed");

    // `wire_length` sits after `coord` in `PlacedCellNode`'s emission
    // order, so in compact JSON it always shows up as
    // `,"wire_length":<int>`. Strip that pattern, normalise the stage
    // tag each side carries, and the routed and placement bytes must
    // match exactly.
    let stripped_routed = normalize_stage_tags(&strip_wire_length(&routed_json));
    assert_eq!(
        stripped_routed,
        normalize_stage_tags(&placement_json),
        "routing must not perturb placement fields — routed compact JSON with wire_length stripped and the stage tag normalised should match placement compact JSON byte-for-byte",
    );
}

fn strip_wire_length(compact: &str) -> String {
    const PATTERN: &str = ",\"wire_length\":";
    let mut out = String::with_capacity(compact.len());
    let mut rest = compact;
    while let Some(idx) = rest.find(PATTERN) {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + PATTERN.len()..];
        let end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// AC8 — two non-empty scopes route independently. A clean scope
/// passes through with `wire_length` populated; a scope that fails
/// congestion elides without poisoning the sibling. The scope order
/// on the output matches the input order for the survivors.
#[test]
fn multiple_scopes_route_independently() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct alpha size=7x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b
  logic sig.open = sig.a or sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.open
  circuit region=floor void=2

struct beta size=4x3
  floor mat_slot=wall
  pressure_plate id=r at=front.outside offset=0 y=0 -> sig.c
  pressure_plate id=s at=inside.front  offset=0 y=0 -> sig.d
  logic sig.and_cd   = sig.c and sig.d
  logic sig.or_cd    = sig.c or sig.d
  logic sig.combined = sig.and_cd and sig.or_cd
  door id=e side=front at=center mat_slot=wall opened_by=sig.combined
  circuit region=floor void=1
";
    let placement = placement_from_source(source, Edition::Java);
    let routed = compile_routing(&placement);

    // alpha routes cleanly.
    let alpha = routed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "alpha")
        .expect("alpha scope survives routing");
    assert!(
        alpha.ir.cells.iter().all(|c| c.wire_length().is_some()),
        "every alpha cell must carry a routed wire_length",
    );

    // beta hits congestion (same shape as AC4) → elided with a
    // diagnostic without shifting alpha.
    assert!(
        routed.scoped.scopes.iter().all(|e| e.name != "beta"),
        "beta must elide because routing exceeds the reservation",
    );
    let beta_congestion = routed
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .expect("beta's congestion must surface as E_ROUTE_CONGESTION");
    assert!(
        beta_congestion.primary.contains("struct `beta`"),
        "congestion primary must name the failed scope, got {:?}",
        beta_congestion.primary,
    );
}

/// AC9 — Placement IR guarantees Java and Bedrock produce the same
/// per-cell coordinate given the same source. Routing therefore
/// produces the same `wire_length` values on both editions; only the
/// cell tag and edition field differ. Pins the "routing is
/// edition-agnostic in its numeric output" invariant so a future
/// change that accidentally couples `wire_length` to edition-specific
/// cell footprints trips the regression here.
#[test]
fn edition_parity_wire_length_matches_across_java_and_bedrock() {
    let source = load_example("redstone-door.crn");
    let java = compile_routing(&placement_from_source(&source, Edition::Java));
    let bedrock = compile_routing(&placement_from_source(&source, Edition::Bedrock));

    for (jscope, bscope) in java.scoped.scopes.iter().zip(bedrock.scoped.scopes.iter()) {
        for (jc, bc) in jscope.ir.cells.iter().zip(bscope.ir.cells.iter()) {
            assert_eq!(jc.wire_length(), bc.wire_length());
            assert_eq!(jc.coord, bc.coord);
        }
    }
}

/// AC10 — chaining `compile_routing(&routed.scoped)` is forbidden by
/// the producer↔variant table on `PlacementPhase`, and the panic it
/// raises names
/// the cell that tripped it. `#[track_caller]` alone would put only
/// the pass's `.rs:line` in the backtrace, leaving an operator to walk
/// back into the IR to find out which cell was already routed; the
/// expected substring pins the breadcrumb that spares them that walk.
#[test]
#[should_panic(
    expected = "for cell #0 at (0,0,0) in struct `gatehouse` — routing must run exactly once per placement"
)]
fn re_running_routing_pass_panics_loudly() {
    let source = load_example("redstone-door.crn");
    let routed = compile_routing(&placement_from_source(&source, Edition::Java));
    let _twice = compile_routing(&routed.scoped);
}
