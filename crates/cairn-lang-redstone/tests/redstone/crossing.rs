//! `compile_crossing` over real synth fixtures; neither example has anything to legalize.

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    BufferCoord, BufferSegment, CellCoord, DiagnosticCode, NetRef, PlacedCellNode, RouteLayer,
    compile_crossing, compile_edition_netlist, compile_netlist, compile_placement, compile_routing,
    synthesize,
};

use crate::common::{
    IDENTITY_WIRE_SOURCE, LONG_OUTWARD_WIRE_SOURCE, delayed_from_source, legalized_from_source,
    load_example, normalize_stage_tags, shared_bus_source,
};

/// The Error-severity findings: whether a scope survived, not whether it said nothing.
fn errors(
    diagnostics: &[cairn_lang_redstone::Diagnostic],
) -> Vec<&cairn_lang_redstone::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity() == Severity::Error)
        .collect()
}

/// Where `cell`'s buffers stand on the wires into it from the nets `from` accepts.
fn buffers_from(cell: &PlacedCellNode, from: impl Fn(NetRef) -> bool) -> Vec<CellCoord> {
    cell.buffer_coords()
        .iter()
        .filter(|b| {
            cell.drivers
                .iter()
                .any(|d| from(d.net) && BufferSegment::Port(d.port) == b.port)
        })
        .map(|b| b.coord)
        .collect()
}

/// The trunk's repeaters stand 14 apart, not 16: each Java comparator passes on the strength it
/// reads, so a tap may arrive with at most 14 spent.
#[test]
fn a_shared_bus_of_sixteen_cells_shares_its_repeaters() {
    let legalized = legalized_from_source(&shared_bus_source(), Edition::Java);

    let cells = &legalized.scopes[0].ir.cells;
    assert_eq!(cells.len(), 16, "the fixture is the 16-cell chain");
    let bus = NetRef::Input(1);
    let on_the_bus = |cell: &PlacedCellNode| buffers_from(cell, |net| net == bus);
    let blocks: std::collections::BTreeSet<(u32, u32, u32)> = cells
        .iter()
        .flat_map(on_the_bus)
        .map(|b| (b.x, b.y, b.z))
        .collect();
    assert_eq!(
        blocks,
        [(14, 0, 2), (28, 0, 2)].into_iter().collect(),
        "one block per refresh point on the bus, not one per cell",
    );
    let attributions = cells.iter().filter(|c| !on_the_bus(c).is_empty()).count();
    assert!(
        attributions >= 2,
        "the sharing is only pinned if more than one cell names it: {attributions}",
    );
}

/// The pass starts each comparator's wire with what the bus at its rear has spent, so the strand
/// leaves cells #6 and #13 with 14 blocks spent, and the 4-step wire to the next cell's side takes
/// a repeater on its second block, on the bridge or the plane. The strand leaves cell #15 with 4,
/// so the door's wire takes its first on its 12th block.
#[test]
fn the_wires_a_bus_chain_drives_take_repeaters_where_the_strand_runs_out() {
    let legalized = legalized_from_source(&shared_bus_source(), Edition::Java);
    let ir = &legalized.scopes[0].ir;
    let from_a_cell: Vec<Vec<CellCoord>> = ir
        .cells
        .iter()
        .map(|cell| buffers_from(cell, |net| matches!(net, NetRef::Cell(_))))
        .collect();
    let buffered: Vec<usize> = from_a_cell
        .iter()
        .enumerate()
        .filter(|(_, buffers)| !buffers.is_empty())
        .map(|(cell, _)| cell)
        .collect();
    assert_eq!(
        buffered,
        [7, 14],
        "of the wires between cells, only those out of cells #6 and #13, which the strand leaves with 14 blocks spent, take a repeater",
    );
    let mut expected = vec![Vec::new(); 16];
    for (cell, coord) in [
        (7, CellCoord::new(14, 1, 1)),
        (14, CellCoord::new(28, 0, 0)),
    ] {
        expected[cell] = vec![coord];
    }
    assert_eq!(from_a_cell, expected);
    assert_eq!(
        ir.outputs[0]
            .buffer_coords()
            .iter()
            .map(|b| b.coord)
            .collect::<Vec<_>>(),
        vec![CellCoord::new(43, 0, 1), CellCoord::new(58, 0, 1)],
    );
}

/// A repeater on the fork at `(15,0,0)` would face one door and starve the other.
#[test]
fn two_doors_share_the_repeater_before_their_fork() {
    let source = r"
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct s size=17x3
  floor mat_slot=wall
  door id=d0 side=front at=center mat_slot=door
  door id=d1 side=back at=center mat_slot=door
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  door[id=d0] opened_by=sig.a
  door[id=d1] opened_by=sig.a
  circuit region=floor void=1
";
    let legalized = legalized_from_source(source, Edition::Java);
    let outputs = &legalized.scopes[0].ir.outputs;
    assert_eq!(outputs.len(), 2, "one actuator pad per door");
    let pads: Vec<((u32, u32, u32), Option<u32>)> = outputs
        .iter()
        .map(|o| ((o.pad.x, o.pad.y, o.pad.z), o.wire_length()))
        .collect();
    assert_eq!(pads, vec![((16, 0, 0), Some(16)), ((16, 0, 1), Some(17))]);
    let fork = (15, 0, 0);
    for (index, output) in outputs.iter().enumerate() {
        let buffers: Vec<(u32, u32, u32)> = output
            .buffer_coords()
            .iter()
            .map(|b| (b.coord.x, b.coord.y, b.coord.z))
            .collect();
        assert_eq!(
            buffers,
            vec![(14, 0, 0)],
            "output #{index} is fed through the trunk repeater, one short of the fork at {fork:?}",
        );
        assert_eq!(output.local_delay_ticks(), Some(1), "output #{index}");
    }
}

fn four_plates_one_door(op: &str) -> String {
    format!(
        r"
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct s size=20x5
  floor mat_slot=wall
  door id=d side=front at=center mat_slot=door
  pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=pb at=inside.front offset=1 y=0 -> sig.b
  pressure_plate id=pc at=front.outside offset=2 y=0 -> sig.c
  pressure_plate id=pd at=inside.front offset=2 y=0 -> sig.d
  logic sig.open = sig.a {op} sig.b {op} sig.c {op} sig.d
  door[id=d] opened_by=sig.open
  circuit region=floor void=2
"
    )
}

/// Every segment is at most 15 blocks, yet the strand from a plate through a cell to the door is
/// over 20 with nothing restoring it. A comparator's side ends at its inverter, so the Java strand
/// reaches the door's wire with a block less spent than the one through three merges.
#[test]
fn cells_that_pass_strength_on_share_one_strand_with_their_inputs() {
    for (op, edition, repeater) in [
        ("or", Edition::Bedrock, (12, 0, 1)),
        ("and", Edition::Java, (13, 0, 1)),
    ] {
        let source = four_plates_one_door(op);
        let legalized = legalized_from_source(&source, edition);
        let ir = &legalized.scopes[0].ir;
        assert_eq!(ir.cells.len(), 3, "{op}: three two-input cells");
        let door = &ir.outputs[0];
        assert_eq!(
            door.wire_length(),
            Some(15),
            "{op}: the door's own segment is at the limit, not past it"
        );
        assert_eq!(
            door.buffer_coords()
                .iter()
                .map(|b| (b.coord.x, b.coord.y, b.coord.z))
                .collect::<Vec<_>>(),
            vec![repeater],
            "{op}: one repeater on the wire out to the door",
        );
        assert_eq!(
            door.local_delay_ticks(),
            Some(1),
            "{op}: and one tick for it"
        );
    }
}

/// A Bedrock `and` is torches and a Java `or` a repeater, both restoring strength.
#[test]
fn cells_that_restore_strength_start_the_count_again() {
    for (op, edition) in [("and", Edition::Bedrock), ("or", Edition::Java)] {
        let legalized = legalized_from_source(&four_plates_one_door(op), edition);
        let ir = &legalized.scopes[0].ir;
        assert_eq!(ir.cells.len(), 3, "{op}: three two-input cells");
        assert!(
            ir.cells.iter().all(|cell| cell.cell.regenerates()),
            "{op}: every cell restores strength on {edition:?}",
        );
        assert_eq!(
            ir.outputs[0].wire_length(),
            Some(15),
            "{op}: the same layout as the pass-through case"
        );
        let buffers: Vec<_> = ir
            .cells
            .iter()
            .flat_map(|cell| cell.buffer_coords().iter())
            .chain(
                ir.outputs
                    .iter()
                    .flat_map(|output| output.buffer_coords().iter()),
            )
            .collect();
        assert_eq!(
            buffers,
            Vec::<&BufferCoord>::new(),
            "{op}: no repeater anywhere"
        );
    }
}

#[test]
fn redstone_door_java_carries_no_buffers() {
    let source = load_example("redstone-door.crn");
    let delayed = delayed_from_source(&source, Edition::Java);
    let legalized = compile_crossing(&delayed);
    assert!(
        errors(&legalized.diagnostics).is_empty(),
        "nothing here refuses the scope: {:?}",
        legalized.diagnostics,
    );
    assert!(
        legalized.diagnostics.is_empty(),
        "nothing to legalize: {:?}",
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

#[test]
fn redstone_door_bedrock_carries_no_buffers() {
    let source = load_example("redstone-door.crn");
    let delayed = delayed_from_source(&source, Edition::Bedrock);
    let legalized = compile_crossing(&delayed);
    assert!(
        legalized.diagnostics.is_empty(),
        "the edition does not move the wire: {:?}",
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
        cell.local_delay_ticks(),
        Some(0),
        "delay ticks preserved from stage 3",
    );
    assert!(cell.buffer_coords().is_empty(), "no buffer expected");
}

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

#[test]
fn json_output_byte_identical_apart_from_stage_tag_on_a_scope_with_escapes() {
    let source = load_example("crossbar.crn");
    let delayed = delayed_from_source(&source, Edition::Java);
    let legalized = compile_crossing(&delayed);
    assert!(
        legalized.diagnostics.is_empty(),
        "crossbar.crn must survive legalization: {:?}",
        legalized.diagnostics,
    );

    let delayed_json = serde_json::to_string_pretty(&delayed).expect("delayed IR serialises");
    let legalized_json =
        serde_json::to_string_pretty(&legalized.scoped).expect("legalized IR serialises");

    assert_eq!(
        normalize_stage_tags(&delayed_json),
        normalize_stage_tags(&legalized_json),
        "an escape laid at stage 2 reaches this pass as wire, not as work",
    );
}

/// `sig.a` has the most sinks, so it is laid first and takes the faces of the front door's pad.
#[test]
fn two_nets_that_want_one_coord_with_no_layer_above_are_refused() {
    let source = "\
theme cross:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct thin size=4x4
  floor mat_slot=wall
  door  id=front side=front at=center mat_slot=door
  door  id=back  side=back  at=center mat_slot=door

  pressure_plate id=plate1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=plate2 at=inside.front  offset=1 y=0 -> sig.b

  logic sig.f = sig.a and sig.b

  door[id=front] opened_by=sig.f
  door[id=back]  opened_by=sig.a

  circuit region=floor void=1
";
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    let synth = synthesize(&intent);
    let netlist = compile_netlist(&synth.scoped);
    let edition_netlist = compile_edition_netlist(&netlist, Edition::Java);
    let placement = compile_placement(&edition_netlist, &intent);
    let routed = compile_routing(&placement.scoped);
    let refusal = routed
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .unwrap_or_else(|| {
            panic!(
                "void=1 with two nets wanting one coord must refuse: {:?}",
                routed.diagnostics,
            )
        });
    assert!(
        refusal
            .primary
            .contains("another net's dust, on the coord or one step from it in the same plane"),
        "the refusal names the obstacle and its reach: {}",
        refusal.primary,
    );
    assert!(
        refusal
            .primary
            .contains("the faces it could arrive through are taken by sig.a"),
        "and names the net in the way: {}",
        refusal.primary,
    );
    assert!(
        refusal
            .notes
            .iter()
            .any(|n| n.message.contains("raise `void` above 1")),
        "and the fix names the layer that is missing: {:?}",
        refusal.notes,
    );
    assert!(
        routed.scoped.scopes.iter().all(|e| e.name != "thin"),
        "failed scope must elide before anything downstream reads it",
    );

    let module = parse(&source.replace("void=1", "void=2")).expect("parse");
    let intent = lower(&module);
    let edition_netlist =
        compile_edition_netlist(&compile_netlist(&synthesize(&intent).scoped), Edition::Java);
    let routed = compile_routing(&compile_placement(&edition_netlist, &intent).scoped);
    assert!(
        routed.scoped.scopes.iter().any(|e| e.name == "thin"),
        "void=2 gives the escape a layer: {:?}",
        routed.diagnostics,
    );
}

#[test]
fn a_cell_with_its_faces_taken_and_no_layer_above_is_refused() {
    let source = "\
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct chain size=5x3
  floor mat_slot=wall
  door id=d0 side=front at=center mat_slot=door
  pressure_plate id=p0 at=front.outside offset=0 y=0 -> sig.s0
  pressure_plate id=p1 at=inside.front offset=1 y=0 -> sig.s1
  logic sig.c0 = sig.s0 and sig.s1
  logic sig.c1 = sig.c0 and sig.s0
  door[id=d0] opened_by=sig.c1
  circuit region=floor void=1
";
    let routed_at = |source: &str| {
        let intent = lower(&parse(source).expect("parse"));
        let edition_netlist =
            compile_edition_netlist(&compile_netlist(&synthesize(&intent).scoped), Edition::Java);
        compile_routing(&compile_placement(&edition_netlist, &intent).scoped)
    };

    let routed = routed_at(source);
    let refusal = routed
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .unwrap_or_else(|| panic!("void=1 must refuse: {:?}", routed.diagnostics));
    assert!(
        refusal
            .primary
            .contains("cannot reach (3,0,1) from the driver at (1,0,1)"),
        "the stranded sink is cell #1's body, reached from cell #0's: {}",
        refusal.primary,
    );
    assert!(
        refusal
            .primary
            .contains("the faces it could arrive through are taken by sig.s0 and cell #1"),
        "and the refusal names the nets on its faces: {}",
        refusal.primary,
    );
    assert!(
        routed.scoped.scopes.iter().all(|e| e.name != "chain"),
        "failed scope must elide before anything downstream reads it",
    );

    let routed = routed_at(&source.replace("void=1", "void=2"));
    assert!(
        routed.scoped.scopes.iter().any(|e| e.name == "chain"),
        "void=2 gives the escape a layer: {:?}",
        routed.diagnostics,
    );
}

#[test]
fn crossbar_void_one_is_refused_before_any_crossing_is_computed() {
    let source = load_example("crossbar.crn");
    let patched = source.replace("void=2", "void=1");
    assert_ne!(source, patched, "crossbar.crn lost its `void=2` needle");
    let module = parse(&patched).expect("parse");
    let intent = lower(&module);
    let synth = synthesize(&intent);
    let netlist = compile_netlist(&synth.scoped);
    let edition_netlist = compile_edition_netlist(&netlist, Edition::Java);
    let placement = compile_placement(&edition_netlist, &intent);
    let routed = compile_routing(&placement.scoped);
    let refusal = routed
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .unwrap_or_else(|| panic!("void=1 crossbar must refuse: {:?}", routed.diagnostics));
    assert!(
        refusal.primary.contains("cannot reach (5,0,0)"),
        "the refusal names the pad nothing can reach: {}",
        refusal.primary,
    );
    assert!(
        refusal
            .primary
            .contains("the faces it could arrive through are taken by sig.a and cell #1"),
        "and names every net that took one, in net order: {}",
        refusal.primary,
    );
    assert!(
        routed.scoped.scopes.is_empty(),
        "failed scope must elide from the routed IR",
    );
}

#[test]
fn two_runs_over_the_same_delayed_input_agree() {
    // The door carries no repeaters, so it would agree on `buffer_coords` vacuously.
    let delayed = delayed_from_source(&shared_bus_source(), Edition::Java);
    let first = compile_crossing(&delayed);
    let second = compile_crossing(&delayed);
    assert_eq!(
        serde_json::to_string_pretty(&first.scoped).expect("first serialises"),
        serde_json::to_string_pretty(&second.scoped).expect("second serialises"),
        "two independent crossing runs on the same input must produce the same output",
    );
    assert!(
        first.scoped.scopes[0]
            .ir
            .cells
            .iter()
            .any(|cell| !cell.buffer_coords().is_empty()),
        "the fixture carries repeaters, so the agreement covers where they stand",
    );
    assert_eq!(first.diagnostics.len(), second.diagnostics.len());
}

/// The panic naming cell #0, not an output, pins that cells are checked before outputs.
#[test]
#[should_panic(
    expected = "for cell #0 at (1,0,1) in struct `gatehouse` — crossing legalization must run exactly once per delayed IR"
)]
fn re_running_crossing_pass_panics_loudly() {
    let source = load_example("redstone-door.crn");
    let legalized = compile_crossing(&delayed_from_source(&source, Edition::Java));
    let _twice = compile_crossing(&legalized.scoped);
}

#[test]
fn a_row_filled_to_its_last_column_survives_legalization() {
    let source = "\
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct gen size=9x8
  floor mat_slot=wall
  door id=front side=front at=center mat_slot=door
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=1 y=0 -> sig.b
  logic sig.c0 = sig.a or sig.b
  logic sig.c1 = sig.c0 and sig.b
  logic sig.c2 = sig.c1 or sig.b
  logic sig.c3 = sig.c2 and sig.b
  door[id=front] opened_by=sig.c3
  circuit region=floor void=3
";
    let delayed = delayed_from_source(source, Edition::Java);
    let out = compile_crossing(&delayed);

    assert!(
        out.diagnostics.is_empty(),
        "an exactly-filled row must legalize: {:?}",
        out.diagnostics,
    );
    let scope = out.scoped.scopes.first().expect("the scope legalizes");
    let region = scope.ir.region.as_ref().expect("reservation");
    let last = scope.ir.cells.last().expect("last cell");
    assert_eq!(
        last.coord.x,
        region.width - 2,
        "the last cell must sit in the row's last column",
    );
}

#[test]
fn the_wire_out_to_an_actuator_carries_buffer_repeaters() {
    let delayed = delayed_from_source(LONG_OUTWARD_WIRE_SOURCE, Edition::Java);
    let out = compile_crossing(&delayed);
    assert!(
        out.diagnostics.is_empty(),
        "the fixture legalizes: {:?}",
        out.diagnostics,
    );

    let scope = out.scoped.scopes.first().expect("the scope legalizes");
    let cell = scope.ir.cells.first().expect("the one cell");
    let output = scope.ir.outputs.first().expect("the actuator");

    assert!(
        output.wire_length().expect("routed") > 16,
        "the fixture only means something while the outward segment is past the dust limit",
    );
    assert_eq!(
        cell.buffer_coords().len(),
        0,
        "the inward segments are short"
    );

    assert!(
        output.local_delay_ticks().is_some_and(|ticks| ticks >= 1),
        "the outward segment must be charged for its repeaters, got {:?}",
        output.local_delay_ticks(),
    );
    assert_eq!(
        u32::try_from(output.buffer_coords().len()).expect("buffer count fits")
            * cairn_lang_redstone::BUFFER_REPEATER_TICKS,
        output.local_delay_ticks().expect("delayed"),
        "every tick the delay pass counted must have a coord behind it",
    );
    assert!(
        output
            .buffer_coords()
            .iter()
            .all(|b| b.port == BufferSegment::Out),
        "a buffer on the outward wire belongs to no input port: {:?}",
        output.buffer_coords(),
    );
}

#[test]
fn an_outward_segment_is_charged_like_an_inward_one_of_the_same_length() {
    let delayed = delayed_from_source(LONG_OUTWARD_WIRE_SOURCE, Edition::Java);
    let out = compile_crossing(&delayed);
    let scope = out.scoped.scopes.first().expect("the scope legalizes");
    let output = scope.ir.outputs.first().expect("the actuator");

    // `(s - 1) / 16` is exact only on a straight, unbranched route like this one.
    let segment = output.wire_length().expect("routed");
    let expected = (segment - 1) / 16;
    assert_eq!(
        output.buffer_coords().len(),
        expected as usize,
        "segment {segment} needs {expected} repeaters",
    );
}

#[test]
fn one_net_feeding_two_actuators_shares_the_repeater_on_their_common_span() {
    let source = "\
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct fan size=40x6
  floor mat_slot=wall
  door id=d1 side=front at=center mat_slot=door
  door id=d2 side=back at=center mat_slot=door
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=1 y=0 -> sig.b
  logic sig.f = sig.a and sig.b
  door[id=d1] opened_by=sig.f
  door[id=d2] opened_by=sig.f
  circuit region=floor void=2
";
    let delayed = delayed_from_source(source, Edition::Java);
    let out = compile_crossing(&delayed);

    assert!(
        out.diagnostics.is_empty(),
        "the fixture legalizes: {:?}",
        out.diagnostics,
    );
    let scope = out.scoped.scopes.first().expect("the scope legalizes");
    let first = scope.ir.outputs.first().expect("actuator #0");
    let second = scope.ir.outputs.get(1).expect("actuator #1");
    assert!(
        !first.buffer_coords().is_empty(),
        "the fixture only means something while the shared span needs a repeater",
    );
    let shared: Vec<_> = first.buffer_coords().iter().map(|b| b.coord).collect();
    let other: Vec<_> = second.buffer_coords().iter().map(|b| b.coord).collect();
    assert_eq!(
        shared, other,
        "both actuators are refreshed by the repeaters on the span they share",
    );
    assert!(
        shared.iter().all(|c| c.layer == RouteLayer::Plane),
        "the shared span is this net's own wire, so nothing escapes: {shared:?}",
    );
}

#[test]
fn an_identity_wire_is_legalized_like_any_other_segment() {
    let delayed = delayed_from_source(IDENTITY_WIRE_SOURCE, Edition::Java);
    let out = compile_crossing(&delayed);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);

    let scope = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "pass")
        .expect("the identity wire reaches stage 4");
    assert_eq!(scope.ir.cells, [] as [PlacedCellNode; 0]);
    let output = scope.ir.outputs.first().expect("the actuator");
    assert_eq!(
        output.stage(),
        cairn_lang_redstone::PlacementStage::Crossing
    );
    assert_eq!(
        u32::try_from(output.buffer_coords().len()).expect("buffer count fits")
            * cairn_lang_redstone::BUFFER_REPEATER_TICKS,
        output.local_delay_ticks().expect("delayed"),
        "every tick counted at stage 3 has a coord at stage 4",
    );
}

/// Pinned by key name, since the two hand-written `Serialize` impls could share a typo.
#[test]
fn the_json_for_a_placed_actuator_carries_its_own_keys() {
    let delayed = delayed_from_source(LONG_OUTWARD_WIRE_SOURCE, Edition::Java);
    let out = compile_crossing(&delayed);
    let json = serde_json::to_string(&out.scoped).expect("serialise");

    // The output's own object, since a cell emits the same keys elsewhere in the dump.
    let value: serde_json::Value = serde_json::from_str(&json).expect("dump parses");
    let output = value
        .as_array()
        .and_then(|scopes| scopes.iter().find(|s| s["name"] == "reach"))
        .map(|scope| &scope["ir"]["outputs"][0])
        .expect("the reach scope carries one actuator");

    for key in [
        "name",
        "driver",
        "pad",
        "stage",
        "wire_length",
        "local_delay_ticks",
        "buffer_coords",
    ] {
        assert!(
            output.get(key).is_some(),
            "the actuator's own object must carry {key}: {output}",
        );
    }
    assert_eq!(output["stage"], "crossing");
    assert_eq!(output["buffer_coords"][0]["port"], "out");
    assert!(
        output.get("delay_ticks").is_none(),
        "the actuator must not carry the pre-rename key: {output}",
    );
}
