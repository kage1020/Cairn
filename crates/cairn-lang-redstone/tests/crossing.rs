//! Integration tests for `cairn_lang_redstone::compile_crossing`.
//!
//! Locks the observable behaviours of the crossing-legalization slice
//! (`spec/redstone` §14.5, stage 4 of place-and-route): the
//! `examples/redstone-door.crn` happy path (per edition), empty-module
//! pass-through, the JSON wire form staying byte-identical to the
//! delayed IR apart from the `stage` tag when no buffer coord landed
//! (the field serde-skips on its default), the tag itself keeping a
//! zero-buffer legalized dump distinguishable from its delayed input,
//! and per-scope independence when a module carries more than one
//! scope.
//!
//! Both redstone examples make a net go round another. The pad column
//! at `x=0` is where: the pads are packed down it by index, so the
//! second sensor's wire has to come round the first sensor's pad, and
//! the row it comes round through is the row the cell drives its
//! actuators out along. One cell and two sensors is enough —
//! `redstone-door.crn` is that shape, and its cell's outward wire
//! climbs a layer at its own doorstep rather than merging with the
//! sensor's. Neither example reaches this pass with anything to
//! legalize, which is what the assertions here say.

use std::fmt::Write as _;
use std::path::PathBuf;

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    BufferSegment, DiagnosticCode, PlacedCellNode, RouteLayer, ScopedPlacementIr, compile_crossing,
    compile_delay, compile_edition_netlist, compile_netlist, compile_placement, compile_routing,
    synthesize,
};

mod common;

use common::normalize_stage_tags;

/// The Error-severity half of a pass's findings.
///
/// "Did this scope survive" and "did this scope say nothing" are
/// different questions, and the tests that ask the first say so by
/// calling this. Nothing this pass raises is a warning today, so the
/// two answers agree — the distinction is kept because a future
/// warning arriving should not silently turn a survival check into a
/// silence check.
fn errors(
    diagnostics: &[cairn_lang_redstone::Diagnostic],
) -> Vec<&cairn_lang_redstone::Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity() == Severity::Error)
        .collect()
}

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

/// A shared bus of 16 cells legalizes at `void=2`, with one repeater
/// per refresh point rather than one per cell.
///
/// Every cell reads `sig.b`. The trunk that carries it runs along the
/// free row beside the cell row and each cell taps off it, so the
/// 15-step point of the route into each of them is the same coord and
/// one repeater refreshes all of them. Two coords rather than one,
/// because the row is `2 * cells` columns long and the far half of it
/// is past the second refresh point.
///
/// That the tree reaches the far cells along a trunk beside the row
/// and not *through* the near ones is what makes this a small number
/// at all: a chain threaded through the cell bodies would ask for a
/// repeater per hop.
///
/// Built from source rather than by hand because the claim is about
/// what a `.crn` a user can write does, and because the shape depends
/// on where the placement pass lays the cell row.
#[test]
fn a_shared_bus_of_sixteen_cells_shares_its_repeaters() {
    let mut source = String::from(
        r"
theme t:
  slot wall -> @oak_planks

struct chain size=60x5
  floor mat_slot=wall

  pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=pb at=inside.front  offset=0 y=0 -> sig.b

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

    let delayed = delayed_from_source(&source, Edition::Java);
    let legalized = compile_crossing(&delayed);
    assert!(
        legalized.diagnostics.is_empty(),
        "a bus every cell hangs off needs one repeater, not one per cell: {:?}",
        legalized.diagnostics,
    );

    let cells = &legalized.scoped.scopes[0].ir.cells;
    assert_eq!(cells.len(), 16, "the fixture is the 16-cell chain");
    let blocks: std::collections::BTreeSet<(u32, u32, u32)> = cells
        .iter()
        .flat_map(PlacedCellNode::buffer_coords)
        .map(|b| (b.coord.x, b.coord.y, b.coord.z))
        .collect();
    assert_eq!(
        blocks,
        [(14, 0, 1), (29, 0, 1)].into_iter().collect(),
        "two blocks, on the plane, beside the row rather than over it — one \
         per refresh point and not one per cell",
    );
    let attributions = cells
        .iter()
        .filter(|c| !c.buffer_coords().is_empty())
        .count();
    assert!(
        attributions >= 2,
        "the sharing is only pinned if more than one cell names it: {attributions}",
    );
}

/// AC1 — `examples/redstone-door.crn` compiled for Java survives
/// crossing legalization: every driver segment sits under the
/// dust-attenuation limit of 15 so no buffer coord materialises, and
/// `buffer_coords` stays empty on the survived cell.
///
/// Three nets run in this scope, not one: each sensor drives the cell,
/// and the cell drives the door. `sig.exit`'s pad sits behind
/// `sig.step`'s in the `x=0` column, so its wire has to come round the
/// pad in front of it, and the row it comes round through is the row
/// the cell drives its actuator out along. The cell's wire therefore
/// climbs onto the bridge layer at its own doorstep and runs the
/// length of the region up there — the escape §14.5 specifies, in the
/// smallest circuit the corpus has.
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
        "the second sensor's wire comes round the first sensor's pad and the \
         cell's outward run goes round that, so there is nothing left to \
         report: {:?}",
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
/// the cell realisation swaps and the geometry does not, so the same
/// nets take the same coords. `wire_length` and `delay_ticks` are
/// preserved verbatim from the delayed IR.
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
/// for a scope whose nets had to go round each other.
/// `examples/crossbar.crn` sends one of its sensor signals up onto the
/// bridge layer to clear the other, and that decision belongs to stage
/// 2: by the time this pass runs it is in the routed lengths already,
/// and every driver segment here sits below `DUST_ATTENUATION_LIMIT`,
/// so the legalized JSON equals the delayed JSON apart from the stage
/// tag. Pins two invariants at once: both gate cells and both door
/// outputs survive legalization intact — a regression that elided the
/// scope would trip the byte-identity assertion via a shorter left
/// side — and an escape upstream does not reach this pass as a change
/// to the wire form.
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

/// AC — two nets that want one coord, in a reservation with no layer
/// above the plane to climb to.
///
/// The escape §14.5 specifies is "a bridge tile or a vertical layer",
/// and `void=1` reserves neither. So the second net has to find its
/// way round on the plane or not at all, and this fixture is the case
/// where it cannot: the scope is refused rather than shorted, which is
/// the whole trade this pipeline makes.
#[test]
fn two_nets_that_want_one_coord_with_no_layer_above_are_refused() {
    let source = "\
theme cross:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct thin size=5x4
  floor mat_slot=wall
  door  id=front side=front at=center mat_slot=door
  door  id=back  side=back  at=center mat_slot=door

  pressure_plate id=plate1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=plate2 at=inside.front  offset=0 y=0 -> sig.b

  logic sig.f = sig.a and sig.b

  door[id=front] opened_by=sig.f
  door[id=back]  opened_by=sig.f

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
            .contains("dust already laid for another net"),
        "the refusal says which of the three kinds of obstacle it means: {}",
        refusal.primary,
    );
    assert!(
        refusal
            .primary
            .contains("the coords beside it carry cell #0 and sig.a"),
        "and names the nets in the way, in net order: {}",
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
}

/// AC — `examples/crossbar.crn` with `void=1` never reaches the
/// crossing pass. Four nets share the plane, and with no layer above
/// it the last of them has nowhere to go that the others have not
/// taken. Stage 2 says so, and says which three sinks it could not
/// reach rather than only the first.
#[test]
fn crossbar_void_one_is_refused_before_any_crossing_is_computed() {
    let source = load_example("crossbar.crn");
    let patched = source.replace("void=2", "void=1");
    assert_ne!(
        source, patched,
        "crossbar.crn no longer contains the `void=2` needle — fixture drifted \
         and the void=1 refusal path is not being exercised",
    );
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
        refusal.primary.contains("cannot reach (1,0,0)"),
        "the refusal names the cell nothing can reach: {}",
        refusal.primary,
    );
    assert!(
        refusal
            .primary
            .contains("2 more of this scope's sinks cannot be reached either"),
        "and the count, so a fix-one-recompile loop is not several rounds of \
         one sizing decision: {}",
        refusal.primary,
    );
    assert!(
        routed.scoped.scopes.is_empty(),
        "failed scope must elide from the routed IR",
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

/// The exact-fit boundary the placement pass now allows, carried all the
/// way through legalization.
///
/// Placing a cell in the actuator pad's column is only safe if nothing
/// downstream needs that column to itself. Every segment here is under
/// the dust limit, so the buffer allocator has nothing to place — what
/// this pins is the occupancy sweep in stages 2 and 4, which is where a
/// pad sharing a column with a cell would be caught. A layout legal at
/// stage 1 and refused at stage 4 would have moved the failure rather
/// than removed it.
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
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
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
        "the fixture only tests the boundary while the last cell sits in the \
         last column the row has, one short of the actuator pads'",
    );
}

/// A wire to an actuator attenuates like any other wire.
///
/// Two sensors and one cell inside a 40-wide region: the cell sits at
/// `x=0` and its actuator pad at `x=39`, so the segment out to the door
/// is far past the 15-block dust limit while every segment into the
/// cell is short. Before the output pad was a placed node, that segment
/// was measured (the attenuation cap looked at it) but never charged —
/// zero buffers counted, zero coords materialised — so a driver a
/// hundred blocks from its actuator reported the same delay as one
/// beside it.
#[test]
fn the_wire_out_to_an_actuator_carries_buffer_repeaters() {
    let source = "\
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct reach size=40x6
  floor mat_slot=wall
  door id=front side=front at=center mat_slot=door
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  logic sig.c = sig.a or sig.b
  door[id=front] opened_by=sig.c
  circuit region=floor void=3
";
    let delayed = delayed_from_source(source, Edition::Java);
    let out = compile_crossing(&delayed);
    assert!(
        out.diagnostics.is_empty(),
        "the fixture legalizes: {:?}",
        out.diagnostics,
    );

    let scope = out.scoped.scopes.first().expect("the scope legalizes");
    let cell = scope.ir.cells.first().expect("the one cell");
    let output = scope.ir.outputs.first().expect("the actuator");

    // The premise: the outward segment is long and the inward ones are
    // not, so any buffer here belongs to the wire this test is about.
    assert!(
        output.wire_length().expect("routed") > 15,
        "the fixture only means something while the outward segment is past the dust limit",
    );
    assert_eq!(
        cell.buffer_coords().len(),
        0,
        "the inward segments are short"
    );

    assert!(
        output.delay_ticks().is_some_and(|ticks| ticks >= 1),
        "the outward segment must be charged for its repeaters, got {:?}",
        output.delay_ticks(),
    );
    assert_eq!(
        u32::try_from(output.buffer_coords().len()).expect("buffer count fits")
            * cairn_lang_redstone::BUFFER_REPEATER_TICKS,
        output.delay_ticks().expect("delayed"),
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

/// The symmetry the previous test's asymmetry was: a segment of a given
/// length is charged the same whether it runs into a cell or out to an
/// actuator.
#[test]
fn an_outward_segment_is_charged_like_an_inward_one_of_the_same_length() {
    let source = "\
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct pair size=40x6
  floor mat_slot=wall
  door id=front side=front at=center mat_slot=door
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  logic sig.c = sig.a or sig.b
  door[id=front] opened_by=sig.c
  circuit region=floor void=3
";
    let delayed = delayed_from_source(source, Edition::Java);
    let out = compile_crossing(&delayed);
    let scope = out.scoped.scopes.first().expect("the scope legalizes");
    let output = scope.ir.outputs.first().expect("the actuator");

    // `buffer_count_for_segment` is `(s - 1) / 15`; asserting the pass's
    // figure against that formula rather than against a literal keeps
    // the two from being re-derived from each other.
    let segment = output.wire_length().expect("routed");
    let expected = (segment - 1) / 15;
    assert_eq!(
        output.buffer_coords().len(),
        expected as usize,
        "segment {segment} needs {expected} repeaters",
    );
}

/// One net feeding two actuators shares its routed prefix, so both
/// segments compute the same buffer candidates. One repeater standing
/// on that strand refreshes the signal for both sinks; giving the
/// second segment a repeater of its own would stack two blocks on one
/// strand of dust, and the coords the two actuators name would stop
/// matching.
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
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
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

/// An identity wire reaches stage 4 as well as stages 1–3: dropping it
/// at the top of `legalize_scope` would leave its counted repeaters
/// without coords.
#[test]
fn an_identity_wire_is_legalized_like_any_other_segment() {
    let source = "\
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
    let delayed = delayed_from_source(source, Edition::Java);
    let out = compile_crossing(&delayed);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);

    let scope = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "pass")
        .expect("the identity wire reaches stage 4");
    assert!(scope.ir.cells.is_empty());
    let output = scope.ir.outputs.first().expect("the actuator");
    assert_eq!(
        output.stage(),
        cairn_lang_redstone::PlacementStage::Crossing
    );
    assert_eq!(
        u32::try_from(output.buffer_coords().len()).expect("buffer count fits")
            * cairn_lang_redstone::BUFFER_REPEATER_TICKS,
        output.delay_ticks().expect("delayed"),
        "every tick counted at stage 3 has a coord at stage 4",
    );
}

/// The wire form of a placed actuator, pinned by name. Both `Serialize`
/// impls are hand-written and adjacent, so a key misspelled in one
/// pattern and copied into the other is invisible to a comparison
/// between them.
#[test]
fn the_json_for_a_placed_actuator_carries_its_own_keys() {
    let source = "\
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct reach size=40x6
  floor mat_slot=wall
  door id=front side=front at=center mat_slot=door
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  logic sig.c = sig.a or sig.b
  door[id=front] opened_by=sig.c
  circuit region=floor void=3
";
    let delayed = delayed_from_source(source, Edition::Java);
    let out = compile_crossing(&delayed);
    let json = serde_json::to_string(&out.scoped).expect("serialise");

    for key in [
        "\"stage\":\"crossing\"",
        "\"pad\":",
        "\"wire_length\":",
        "\"delay_ticks\":",
        "\"buffer_coords\":",
        "\"port\":\"out\"",
    ] {
        assert!(json.contains(key), "the dump must carry {key}: {json}");
    }
}
