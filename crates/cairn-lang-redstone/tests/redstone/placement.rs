//! `compile_placement`: cell coordinates inside each scope's `circuit region=` reservation, or
//! `E_ROUTE_CONGESTION` / `E_NO_CIRCUIT_REGION` when the reservation cannot hold them.

use cairn_lang_core::Edition;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    Diagnostic, DiagnosticCode, EditionCell, ScopedEditionNetlistIr, ScopedPlacementIrEntry,
    compile_placement,
};

use crate::common::{edition_netlist_from_source, load_example};

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
    assert_eq!(cell.coord.z, 1);
    assert!(cell.wire_length().is_none(), "routing fills wire_length");
    assert!(
        cell.local_delay_ticks().is_none(),
        "the delay pass fills local_delay_ticks",
    );
}

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

#[test]
fn multi_cell_scope_places_in_topological_order() {
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
    for (i, (cell, expected)) in ir.cells.iter().zip([1u32, 3, 5]).enumerate() {
        assert_eq!(
            cell.coord.x, expected,
            "cell[{i}] should sit at x={expected}",
        );
        assert_eq!(cell.coord.y, 0);
        assert_eq!(cell.coord.z, 1);
    }
}

#[test]
fn congestion_fires_route_congestion_and_elides_scope() {
    // 12 blocks of cells in 9 reserved: about 1.3 times over budget.
    let source = r"
theme t:
  slot wall -> @oak_planks

struct tiny size=3x3
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b

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
    // The fix triple from `spec/redstone` "Place-and-route".
    for phrase in ["increase", "void", "enlarge", "region", "split", "circuit"] {
        assert!(
            footer.message.contains(phrase),
            "footer should carry the `spec/redstone` \"Place-and-route\" triple \
             (missing {phrase:?}), got {:?}",
            footer.message,
        );
    }
    assert!(
        !footer.message.contains("`circuit` blocks"),
        "footer must not advise a second `circuit` line, which `check` refuses, got {:?}",
        footer.message,
    );

    assert!(
        out.scoped.scopes.iter().all(|e| e.name != "tiny"),
        "failed scope must be elided from the output",
    );
}

#[test]
fn missing_region_fires_no_circuit_region() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct nomarker size=7x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b

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

#[test]
fn empty_edition_netlist_produces_no_placement_entry() {
    let module = parse("").expect("empty parses");
    let intent = lower(&module);
    let scoped = ScopedEditionNetlistIr::new();
    let out = compile_placement(&scoped, &intent);
    assert_eq!(out.diagnostics, [] as [Diagnostic; 0]);
    assert!(out.scoped.is_empty());
    assert_eq!(out.scoped.scopes, [] as [ScopedPlacementIrEntry; 0]);
}

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
        !json.contains("\"local_delay_ticks\""),
        "local_delay_ticks must be elided today: {json}",
    );
}

#[test]
fn missing_size_falls_through_to_no_circuit_region() {
    let source = r"
theme t:
  slot wall -> @oak_planks

def gadget
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b
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
    assert_eq!(
        &source[missing[0].span.clone()],
        "circuit region=floor void=2",
        "the finding stands on the `circuit` line",
    );
    assert!(
        missing[0]
            .primary
            .ends_with("the enclosing scope has no `size=WxH` header for it to reserve within"),
        "diagnostic must name the missing `size=` cause and no other: {:?}",
        missing[0].primary,
    );
    assert_eq!(
        fix_line(missing[0]),
        "Fix: give the enclosing scope a `size=WxH` header",
    );
}

fn fix_line(diagnostic: &cairn_lang_redstone::Diagnostic) -> &str {
    &diagnostic
        .notes
        .iter()
        .find(|n| n.message.starts_with("Fix:"))
        .unwrap_or_else(|| panic!("no Fix line: {:?}", diagnostic.notes))
        .message
}

#[test]
fn void_zero_is_refused_on_its_circuit_line_for_that_reason() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct simple size=5x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b
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
    assert_eq!(
        &source[d.span.clone()],
        "circuit region=floor void=0",
        "the finding stands on the `circuit` line, not the first `logic` line",
    );
    assert_eq!(
        d.primary,
        "this `circuit` line reserves no room for the scope's redstone cells or actuator pads: its `void=0` reserves no service layer",
    );
    assert_eq!(fix_line(d), "Fix: set `void=` to an integer >= 1");
}

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

#[test]
fn an_identity_wire_in_a_one_column_region_is_refused_for_its_width() {
    for depth in [5, 50] {
        let source = format!(
            "
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct s size=1x{depth}
  floor mat_slot=wall
  door id=d side=front at=center mat_slot=door
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  door[id=d] opened_by=sig.a
  circuit region=floor void=2
"
        );
        let out = placement_of(&source);

        assert!(
            out.scoped.scopes.is_empty(),
            "one column cannot hold both pad columns: {:?}",
            out.scoped.scopes,
        );
        let diagnostic = out
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::RouteCongestion)
            .expect("the shared column must surface as E_ROUTE_CONGESTION");
        assert_eq!(diagnostic.severity(), Severity::Error);
        assert_eq!(
            &source[diagnostic.span.clone()],
            "circuit region=floor void=2",
        );
        assert!(
            diagnostic.primary.contains("needs 2 columns")
                && diagnostic.primary.contains(&format!(
                    "only 1 wide, so the two are one column (region 1x{depth}, void=2)"
                )),
            "the refusal names the width: {}",
            diagnostic.primary,
        );
        let fix = fix_line(diagnostic);
        assert!(
            fix.starts_with("Fix: widen the enclosing `size=WxH` to at least two columns.")
                && !fix.contains("depth >="),
            "the fix asks for width, not depth: {fix}",
        );
    }
    let wide = placement_of(
        "
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct s size=2x5
  floor mat_slot=wall
  door id=d side=front at=center mat_slot=door
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  door[id=d] opened_by=sig.a
  circuit region=floor void=2
",
    );
    assert!(
        wide.diagnostics.is_empty(),
        "two columns hold both pad columns: {:?}",
        wide.diagnostics,
    );
}

/// One `circuit` line that reserves nothing, and the finding the pass makes of it.
struct RejectedLine {
    /// `size=` header of the scope, or empty for none.
    size: &'static str,
    /// How many `level` blocks the line is nested under.
    levels: usize,
    line: &'static str,
    /// The primary after `this ``circuit`` line reserves no room ...: `.
    reason: &'static str,
    fix: &'static str,
}

/// One case per `CircuitRegionDefect` variant, plus a second `level` for the nesting one.
fn rejected_lines() -> [RejectedLine; 10] {
    let sized = " size=5x5";
    let nested = RejectedLine {
        size: sized,
        levels: 1,
        line: "circuit region=floor void=2",
        reason: "it is written under a `level`, and only a `circuit` line at the scope's top level is read as a reservation",
        fix: "Fix: move the `circuit` line out of the `level` to the scope's top level. That changes nothing else about it: a reservation spans the scope's `size=WxH`, and no pass reads a `level`'s `y=` for a `circuit` line",
    };
    [
        RejectedLine {
            levels: 2,
            ..nested
        },
        nested,
        RejectedLine {
            size: "",
            levels: 0,
            line: "circuit region=floor void=2",
            reason: "the enclosing scope has no `size=WxH` header for it to reserve within",
            fix: "Fix: give the enclosing scope a `size=WxH` header",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit void=2",
            reason: "it has no `region=`",
            fix: "Fix: add `region=<label>` naming the reservation (`region=floor`, `region=basement`)",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=3 void=2",
            reason: "its `region=` must be an identifier or string label, got integer",
            fix: "Fix: write `region=` as an identifier or a string (`region=floor`, `region=basement`)",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=\"\" void=2",
            reason: "its `region=` is an empty label",
            fix: "Fix: give `region=` a non-empty label (`region=floor`, `region=basement`)",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor",
            reason: "it has no `void=`",
            fix: "Fix: add `void=<N>`, the height of the service layer, an integer >= 1",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor void=deep",
            reason: "its `void=` must be an integer, got identifier",
            fix: "Fix: write `void=` as an integer >= 1",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor void=0",
            reason: "its `void=0` reserves no service layer",
            fix: "Fix: set `void=` to an integer >= 1",
        },
        RejectedLine {
            size: sized,
            levels: 0,
            line: "circuit region=floor void=4294967296",
            reason: "its `void=4294967296` is over the limit of 4294967295",
            fix: "Fix: set `void=` to an integer from 1 to 4294967295",
        },
    ]
}

/// A two-sensor `or` gate on a door in `keyword s{size}`, then `lines` at the body's top level.
fn gate_with_lines(keyword: &str, size: &str, lines: &str) -> String {
    format!(
        "
theme t:
  slot wall -> @oak_planks

{keyword} s{size}
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b
  logic sig.open = sig.a or sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.open
{lines}"
    )
}

/// Each case runs under a `struct` and a `def`, since the walk reads the two in separate loops.
#[test]
fn each_circuit_line_that_reserves_nothing_is_refused_on_that_line_with_its_reason() {
    for case in &rejected_lines() {
        for keyword in ["struct", "def"] {
            let mut lines = String::new();
            let mut indent = String::from("  ");
            for y in 0..case.levels {
                let _ = writeln!(lines, "{indent}level y={y}");
                indent.push_str("  ");
            }
            let _ = writeln!(lines, "{indent}{}", case.line);
            let source = gate_with_lines(keyword, case.size, &lines);
            let out = placement_of(&source);

            assert!(out.scoped.scopes.is_empty(), "{source}");
            let [diagnostic] = out.diagnostics.as_slice() else {
                panic!("one finding:\n{source}{:?}", out.diagnostics);
            };
            assert_eq!(diagnostic.code, DiagnosticCode::NoCircuitRegion, "{source}");
            assert_eq!(diagnostic.severity(), Severity::Error, "{source}");
            assert_eq!(&source[diagnostic.span.clone()], case.line, "{source}");
            assert_eq!(
                diagnostic.primary,
                format!(
                    "this `circuit` line reserves no room for the scope's redstone cells or actuator pads: {}",
                    case.reason,
                ),
                "{source}",
            );
            assert_eq!(fix_line(diagnostic), case.fix, "{source}");
        }
    }
}

#[test]
fn the_first_of_two_rejected_lines_is_the_one_reported() {
    let source = gate_with_lines(
        "struct",
        " size=5x5",
        "  circuit void=2\n  circuit region=floor void=0\n",
    );
    let out = placement_of(&source);

    let [diagnostic] = out.diagnostics.as_slice() else {
        panic!("one finding: {:?}", out.diagnostics);
    };
    assert_eq!(diagnostic.code, DiagnosticCode::NoCircuitRegion);
    assert_eq!(&source[diagnostic.span.clone()], "circuit void=2");
    assert!(
        diagnostic.primary.ends_with(": it has no `region=`"),
        "the first line's reason: {}",
        diagnostic.primary,
    );
}

/// Refusing the second line is `check`'s `E_DUPLICATE_CIRCUIT`.
#[test]
fn a_usable_circuit_line_beside_a_rejected_one_places_with_no_finding() {
    for lines in [
        "  circuit region=floor void=0\n  circuit region=basement void=2\n",
        "  circuit region=basement void=2\n  circuit region=3 void=0\n",
    ] {
        let out = placement_of(&gate_with_lines("struct", " size=5x5", lines));

        assert!(out.diagnostics.is_empty(), "{lines}{:?}", out.diagnostics);
        let [scope] = out.scoped.scopes.as_slice() else {
            panic!("the scope places: {lines}{:?}", out.scoped.scopes);
        };
        let region = scope
            .ir
            .region
            .as_ref()
            .expect("a placed scope has a region");
        assert_eq!(
            (region.label.as_str(), region.void),
            ("basement", 2),
            "{lines}"
        );
    }
}

#[test]
fn a_scope_with_no_circuit_line_is_told_what_its_kind_can_do() {
    let structure = gate_with_lines("struct", " size=5x5", "");
    let out = placement_of(&structure);
    let [diagnostic] = out.diagnostics.as_slice() else {
        panic!("one finding: {:?}", out.diagnostics);
    };
    assert_eq!(diagnostic.code, DiagnosticCode::NoCircuitRegion);
    assert_eq!(
        &structure[diagnostic.span.clone()],
        "logic sig.open = sig.a or sig.b"
    );
    assert_eq!(
        diagnostic.primary,
        "this scope has redstone cells or actuator pads to place but no `circuit` line at its top level to reserve room for them",
    );
    assert_eq!(
        fix_line(diagnostic),
        "Fix: add a `circuit region=<label> void=<N>` line at the top level of the scope, with a non-empty label naming the reservation (`region=floor`, `region=basement`) and an integer `void=` >= 1. It reserves within the footprint the scope's `size=WxH` header declares",
    );

    // `check` refuses these rows in a `site`; a library caller reaches placement without it.
    let site = "
theme t:
  slot wall -> @oak_planks

site s
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b
  logic sig.open = sig.a or sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.open
  circuit region=floor void=2
";
    let out = placement_of(site);
    let [diagnostic] = out.diagnostics.as_slice() else {
        panic!("one finding: {:?}", out.diagnostics);
    };
    assert_eq!(diagnostic.code, DiagnosticCode::NoCircuitRegion);
    assert_eq!(
        &site[diagnostic.span.clone()],
        "logic sig.open = sig.a or sig.b"
    );
    assert_eq!(
        diagnostic.primary,
        "this `site` has redstone cells or actuator pads to place, and a `site` has no `size=WxH` header to reserve room for them within",
    );
    assert_eq!(
        fix_line(diagnostic),
        "Fix: move the redstone into a `struct` or `def` with a `size=WxH` header and a `circuit region=<label> void=<N>` line at its top level",
    );
}

/// `check` refuses the second line; placement, run without it, keeps the first.
#[test]
fn duplicate_circuit_region_placement_keeps_the_first() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct dup size=7x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b
  logic sig.open = sig.a or sig.b
  door id=d side=front at=center mat_slot=wall opened_by=sig.open
  circuit region=floor void=2
  circuit region=basement void=3
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Java);
    let out = compile_placement(&edition_netlist, &intent);

    assert!(
        out.diagnostics.is_empty(),
        "placement leaves duplicate `circuit region=` lines to `check`, got {:?}",
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
  pressure_plate id=r at=inside.front  offset=1 y=0 -> sig.b2
  logic sig.both = sig.b1 and sig.b2
  door id=e side=front at=center mat_slot=wall opened_by=sig.both
";
    let (edition_netlist, intent) = edition_netlist_from_source(source, Edition::Bedrock);
    let out = compile_placement(&edition_netlist, &intent);

    let alpha = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "alpha")
        .expect("alpha scope survives");
    assert_eq!(alpha.ir.cells[0].cell, EditionCell::BedrockInverterTorch);
    assert_eq!(alpha.ir.cells[0].coord.x, 1);

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

// ---- the row has to hold the cells (`spec/redstone` "Place-and-route") ----
// Cells stand at `x = 1 + 2i`, so the row wants `2n + 1` columns, which volume cannot see.

use std::fmt::Write as _;

/// A chain of exactly `cells` cells in a `size={width}x{depth}` footprint reserving `void`.
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
         pressure_plate id=p2 at=inside.front offset=1 y=0 -> sig.b\n",
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
    assert_eq!(
        diagnostic.primary,
        "synthesized netlist needs 7 columns for a row of 3 cells, a clear column beside each \
         and one past the end of the row, but the reserved region is only 2 wide (region 2x8, \
         void=3)",
        "the refusal names the row, not an area ratio",
    );
    assert_eq!(diagnostic.severity(), Severity::Error);
}

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
    assert_eq!(
        diagnostic.primary,
        "synthesized netlist needs 7 columns for a row of 3 cells, a clear column beside each \
         and one past the end of the row, but the reserved region is only 4 wide (region 4x8, \
         void=3)",
        "the refusal names the columns the row wants, not the cells it holds",
    );
}

/// A row of `n` cells wants `2n + 1` columns; at `2n` the last cell stands in the pad column.
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

#[test]
fn one_cell_more_than_the_row_holds_is_refused() {
    let out = placement_of(&source_with_cells(5, 4, 8, 3));

    assert_eq!(out.scoped.scopes, [] as [ScopedPlacementIrEntry; 0]);
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

#[test]
fn a_scope_with_cells_in_one_column_is_refused_on_its_row() {
    let out = placement_of(&source_with_cells(1, 1, 5, 2));

    assert_eq!(out.scoped.scopes, [] as [ScopedPlacementIrEntry; 0]);
    let [diagnostic] = out.diagnostics.as_slice() else {
        panic!("one finding: {:?}", out.diagnostics);
    };
    assert_eq!(diagnostic.code, DiagnosticCode::RouteCongestion);
    assert!(
        diagnostic
            .primary
            .contains("needs 3 columns for a row of 1 cells")
            && !diagnostic.primary.contains("sensor pads"),
        "refused on the row, not on the pad columns: {}",
        diagnostic.primary,
    );
}

#[test]
fn a_netlist_short_of_volume_but_not_of_row_still_reports_the_area() {
    // 11 cells fit a 40-wide row, but 40 blocks of volume is short of the 44 asked.
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

#[test]
fn a_region_too_shallow_for_a_lane_either_side_of_the_row_is_refused() {
    let refused = placement_of(&source_with_cells(1, 9, 2, 4));

    assert_eq!(refused.scoped.scopes, [] as [ScopedPlacementIrEntry; 0]);
    let diagnostic = refused
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .expect("the shortfall must surface");
    assert!(
        diagnostic.primary.contains("only 2 deep")
            && diagnostic
                .primary
                .contains("clear row on either side of it"),
        "the refusal must name the depth and what the rows are for: {}",
        diagnostic.primary,
    );

    let placed = placement_of(&source_with_cells(1, 9, 3, 4));
    assert!(
        placed.diagnostics.is_empty() && !placed.scoped.scopes.is_empty(),
        "one row more is the row with its lanes: {:?}",
        placed.diagnostics,
    );
}

#[test]
fn a_scope_with_no_cell_row_wants_no_lanes_beside_it() {
    let refused = placement_of(&source_with_cells(1, 9, 2, 4));
    assert!(
        refused.scoped.scopes.is_empty(),
        "one cell at two rows deep has no lane on one side of it",
    );

    let placed = placement_of(
        r"
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct wire size=9x2
  floor mat_slot=wall
  door id=d side=front at=center mat_slot=door
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.x
  door[id=d] opened_by=sig.x
  circuit region=floor void=4
",
    );
    assert!(
        placed.diagnostics.is_empty(),
        "an identity wire wants no lanes: {:?}",
        placed.diagnostics,
    );
    let ir = &placed
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "wire")
        .expect("wire scope")
        .ir;
    assert!(
        ir.cells.is_empty(),
        "the fixture only means something while it has no cell row",
    );
    assert_eq!(ir.outputs.len(), 1, "and an actuator pad to route out to");
}

/// `n` pads beside a cell row fit a region `n + 1` deep. The primary is pinned because the other
/// footprint refusals share `E_ROUTE_CONGESTION`.
#[test]
fn more_actuators_than_rows_is_refused_before_their_pads_collide() {
    let source = |depth: u32| {
        format!(
            r"
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct four size=8x{depth}
  floor mat_slot=wall
  door id=d1 side=front at=center mat_slot=door
  door id=d2 side=back  at=center mat_slot=door
  door id=d3 side=left  at=center mat_slot=door
  door id=d4 side=right at=center mat_slot=door
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front  offset=1 y=0 -> sig.b
  logic sig.f = sig.a and sig.b
  door[id=d1] opened_by=sig.f
  door[id=d2] opened_by=sig.f
  door[id=d3] opened_by=sig.f
  door[id=d4] opened_by=sig.f
  circuit region=floor void=2
"
        )
    };

    let refused = placement_of(&source(4));
    assert!(
        refused.scoped.scopes.is_empty(),
        "four actuators and the cell row do not fit four rows",
    );
    let diagnostic = refused
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .expect("the shortfall must surface");
    assert!(
        diagnostic.primary.contains("rows for its I/O pads")
            && diagnostic.primary.contains("needs 5 rows")
            && diagnostic.primary.contains("only 4 deep"),
        "the refusal must name the resource that ran out, so it cannot be \
         confused with the others that share its code: {}",
        diagnostic.primary,
    );

    let placed = placement_of(&source(5));
    assert!(
        placed.diagnostics.is_empty() && !placed.scoped.scopes.is_empty(),
        "one row per pad and the cell row is enough, because the pad column \
         starts at z=0: {:?}",
        placed.diagnostics,
    );
}

/// With no cell row to skip, `n` pads on an edge fit a region exactly `n` deep, at `z = 0..n`.
#[test]
fn a_scope_with_no_cell_row_gives_its_pads_every_row() {
    let source = |doors: usize, depth: u32| {
        let mut source = format!(
            "
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct wire size=9x{depth}
  floor mat_slot=wall
  pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=pb at=inside.front offset=1 y=0 -> sig.b
"
        );
        for (d, side) in ["front", "back", "left"].iter().take(doors).enumerate() {
            let _ = writeln!(
                source,
                "  door id=d{d} side={side} at=center mat_slot=door\n  door[id=d{d}] opened_by=sig.{}",
                ["a", "b"][d % 2],
            );
        }
        source.push_str("  circuit region=floor void=2\n");
        source
    };

    for (doors, depth) in [(2, 2), (3, 3)] {
        let placed = placement_of(&source(doors, depth));
        assert!(
            placed.diagnostics.is_empty(),
            "{doors} doors fit {depth} rows with no cell row to step over: {:?}",
            placed.diagnostics,
        );
        let ir = &placed.scoped.scopes[0].ir;
        assert!(
            ir.cells.is_empty(),
            "the fixture only means something while it has no cell row",
        );
        let pads: Vec<(u32, u32, u32)> = ir
            .outputs
            .iter()
            .map(|o| (o.pad.x, o.pad.y, o.pad.z))
            .collect();
        let expected: Vec<(u32, u32, u32)> = (0..depth).map(|z| (8, 0, z)).collect();
        assert_eq!(
            pads, expected,
            "{doors} doors at {depth} rows: one row each, from z=0"
        );
    }

    let refused = placement_of(&source(3, 2));
    assert!(
        refused.scoped.scopes.is_empty(),
        "three doors do not fit two rows"
    );
    let diagnostic = refused
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::RouteCongestion)
        .expect("the shortfall must surface");
    assert!(
        diagnostic.primary.contains("needs 3 rows for its I/O pads")
            && diagnostic.primary.contains("only 2 deep"),
        "the refusal counts one row per pad and no cell row: {}",
        diagnostic.primary,
    );
    let fix = diagnostic
        .notes
        .iter()
        .find(|n| n.message.starts_with("Fix:"))
        .expect("the refusal carries a fix line");
    assert!(
        fix.message.contains("`depth >= max(inputs, outputs)` ")
            && !fix.message.contains("+ 1")
            && !fix.message.contains("step over the cell row"),
        "the fix line asks for no cell row: {}",
        fix.message,
    );
}

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
