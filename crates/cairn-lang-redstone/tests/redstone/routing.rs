//! `compile_routing`: each cell's `wire_length` from its routed nets, and `E_ROUTE_CONGESTION` when
//! the routed footprint outgrows the reservation.

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_redstone::{
    Diagnostic, DiagnosticCode, PlacedCellNode, ScopedPlacementIr, ScopedPlacementIrEntry,
    compile_routing,
};

use crate::common::{load_example, normalize_stage_tags, placement_from_source, strip_int_field};

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
        Some(4),
        "wire_length must be route(step→cell) + route(exit→cell) = 2 + 2 = 4",
    );
    assert!(
        cell.local_delay_ticks().is_none(),
        "the delay pass fills local_delay_ticks",
    );
}

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

/// cell[0]'s output climbs to `y=1` past the lanes either side of it, the escape `spec/redstone`
/// "Place-and-route" specifies; `void=3` leaves room for it.
#[test]
fn multi_cell_scope_pins_wire_length_including_detours() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct sim size=7x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b

  logic sig.and_ab   = sig.a and sig.b
  logic sig.or_ab    = sig.a or sig.b
  logic sig.combined = sig.and_ab and sig.or_ab

  door id=d side=front at=center mat_slot=wall opened_by=sig.combined

  circuit region=floor void=3
";
    let placement = placement_from_source(source, Edition::Java);
    let routed = compile_routing(&placement);
    assert_eq!(
        routed
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        vec![DiagnosticCode::RouteCrossLayerClearance],
        "the climbing fixture carries the advisory and no refusal: {:?}",
        routed.diagnostics,
    );

    let entry = routed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "sim")
        .expect("sim scope");
    assert_eq!(entry.ir.cells.len(), 3);
    assert_eq!(
        entry
            .ir
            .cells
            .iter()
            .map(PlacedCellNode::wire_length)
            .collect::<Vec<_>>(),
        vec![Some(4), Some(8), Some(8)],
        "each cell is charged for the dust into it, climbs included",
    );
    for cell in &entry.ir.cells {
        assert!(
            cell.local_delay_ticks().is_none(),
            "local_delay_ticks must not appear before delay insertion runs, got {:?}",
            cell.local_delay_ticks(),
        );
    }
}

/// `sig.a and sig.a` keeps both operands, so both ports arrive driven by one net.
#[test]
fn ports_sharing_a_net_are_measured_once() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct dup size=20x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a

  logic sig.s0 = sig.a and sig.a

  door id=d side=front at=center mat_slot=wall opened_by=sig.s0

  circuit region=floor void=2
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
        .find(|e| e.name == "dup")
        .expect("dup scope");
    assert_eq!(entry.ir.cells.len(), 1);
    let cell = &entry.ir.cells[0];
    assert_eq!(
        cell.drivers.len(),
        2,
        "the fixture needs both ports on one net to mean anything: {:?}",
        cell.drivers,
    );
    assert!(
        cell.drivers.iter().all(|d| d.net == cell.drivers[0].net),
        "both ports must read the same net: {:?}",
        cell.drivers,
    );
    assert_eq!(
        cell.wire_length(),
        Some(2),
        "the pad at (0,0,0) is two blocks from the cell at (1,0,1), laid once",
    );
}

#[test]
fn post_routing_congestion_fires_route_congestion_and_elides_scope() {
    // The cells fit placement's budget with seven blocks to spare, and the wire spends more. One
    // sensor, since a second would strand a sink and refuse the scope for a different reason.
    let source = r"
theme t:
  slot wall -> @oak_planks

struct pack size=5x3
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a

  logic sig.c0 = not sig.a
  logic sig.c1 = sig.c0 and sig.a

  door id=d side=front at=center mat_slot=wall opened_by=sig.c1

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
    assert_eq!(d.severity(), Severity::Error);
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
        d.primary.contains("5x3"),
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
            "footer is missing {phrase:?}: {:?}",
            footer.message,
        );
    }
    assert!(
        routed.scoped.scopes.iter().all(|e| e.name != "pack"),
        "failed scope must be elided from the routing output",
    );
}

#[test]
fn empty_placement_ir_produces_empty_routing_output() {
    let scoped = ScopedPlacementIr::new();
    let routed = compile_routing(&scoped);
    assert_eq!(routed.diagnostics, [] as [Diagnostic; 0]);
    assert_eq!(routed.scoped.scopes, [] as [ScopedPlacementIrEntry; 0]);
}

#[test]
fn identity_wire_routes_its_sensor_to_actuator_segment() {
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
    let scope = routed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "wire")
        .expect("identity-wire scope must reach the routed IR");
    let output = scope.ir.outputs.first().expect("the actuator is placed");
    assert_eq!(
        output.wire_length(),
        Some(4),
        "the pads sit at (0,0,0) and (4,0,0) of a 5-wide region",
    );
}

#[test]
fn json_dump_carries_wire_length_and_omits_local_delay_ticks() {
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
        !json.contains("\"local_delay_ticks\""),
        "local_delay_ticks must be elided at this stage: {json}",
    );
    assert!(
        json.contains("\"region\":{"),
        "region object should still be present: {json}",
    );
    assert!(
        json.contains("\"coord\":{"),
        "coord object should still be present: {json}",
    );
}

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

    // `wire_length` is never the first key, so it always reads `,"wire_length":<int>`.
    let stripped_routed = normalize_stage_tags(&strip_int_field(&routed_json, "wire_length"));
    assert_eq!(
        stripped_routed,
        normalize_stage_tags(&placement_json),
        "routing must not perturb placement fields — routed compact JSON with wire_length stripped and the stage tag normalised should match placement compact JSON byte-for-byte",
    );
}

#[test]
fn multiple_scopes_route_independently() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct alpha size=7x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b
  logic sig.open = sig.a or sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.open
  circuit region=floor void=2

struct beta size=8x3
  floor mat_slot=wall
  pressure_plate id=r at=front.outside offset=0 y=0 -> sig.c
  pressure_plate id=s at=inside.front  offset=1 y=0 -> sig.d
  logic sig.and_cd   = sig.c and sig.d
  logic sig.or_cd    = sig.c or sig.d
  logic sig.combined = sig.and_cd and sig.or_cd
  door id=e side=front at=center mat_slot=wall opened_by=sig.combined
  circuit region=floor void=1
";
    let placement = placement_from_source(source, Edition::Java);
    let routed = compile_routing(&placement);

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

#[test]
#[should_panic(
    expected = "for cell #0 at (1,0,1) in struct `gatehouse` — routing must run exactly once per placement"
)]
fn re_running_routing_pass_panics_loudly() {
    let source = load_example("redstone-door.crn");
    let routed = compile_routing(&placement_from_source(&source, Edition::Java));
    let _twice = compile_routing(&routed.scoped);
}

/// `spec/redstone` "Place-and-route" leaves these pairs to the physical tile layer.
#[test]
fn crossbar_names_the_pairs_the_tile_layer_has_to_separate() {
    let source = load_example("crossbar.crn");
    for edition in [Edition::Java, Edition::Bedrock] {
        let routed = compile_routing(&placement_from_source(&source, edition));
        let findings: Vec<_> = routed
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::RouteCrossLayerClearance)
            .collect();
        assert_eq!(
            findings.len(),
            1,
            "{edition:?}: one finding per scope, not one per pair: {:?}",
            routed.diagnostics,
        );
        let finding = findings[0];
        assert_eq!(
            finding.severity(),
            Severity::Warning,
            "{edition:?}: the pairs are what the escape costs, not a fault in the layout",
        );
        assert!(
            finding.primary.contains(
                "leaves 12 pairs of dust within one step of each other across layers (2 stacked, 10 staircase)"
            ),
            "{edition:?}: the primary counts both shapes: {}",
            finding.primary,
        );
        let notes: Vec<&str> = finding.notes.iter().map(|n| n.message.as_str()).collect();
        assert_eq!(
            notes,
            vec![
                "(4,1,1) on cell #0 stands directly over (4,0,1) on cell #1",
                "(5,1,1) on cell #0 stands directly over (5,0,1) on cell #1",
                "(1,1,1) on cell #0 stands a layer over, and one step across from, (1,0,0) on sig.a",
                "(1,1,1) on cell #0 stands a layer over, and one step across from, (1,0,2) on sig.b",
                "and 8 more of the same two shapes",
                "`spec/redstone` \"Place-and-route\" makes separating them the physical tile \
                 layer's obligation, and the same chapter's \"Edition differences\" states it: a \
                 `bridge` coord renders as a tile that conducts to neither another net's coord \
                 under it nor another net's coords diagonally under it — its own net's coord \
                 under it is the climb, and has to conduct",
                "Fix: nothing in the source is wrong — the pairs are what the escape costs, and \
                 enlarging the region is not a remedy: where a net has to climb at its own \
                 doorstep, more room only lengthens the run it then makes on the upper layer",
            ],
            "{edition:?}: the notes, in order",
        );
        assert!(
            routed.scoped.scopes.iter().any(|e| e.name == "crossbar"),
            "{edition:?}: an advisory elides nothing — the scope routes",
        );
    }
}
