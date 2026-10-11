//! The refusals this module owns, asked of every stage that calls
//! [`lay_nets`].

use cairn_lang_core::Edition;
use cairn_lang_core::error::Span;

use super::MAX_ATTENUATION_SEGMENT;
use crate::diagnostic::DiagnosticCode;
use crate::edition_netlist_ir::EditionCell;
use crate::grid::{PadColumn, output_pad};
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, PortName};
use crate::placement_ir::{
    CellCoord, PlacedCellNode, PlacedOutputNode, PlacementIr, PlacementPhase, ScopedPlacementIr,
};
use crate::test_fixtures::{
    CollapsedRow, DanglingNet, FarSink, collapsed_pad_row, dangling_net, far_sink, placed_cell,
    regionless_scope, reservation, scoped,
};

/// A reservation too shallow to hold its pad row is refused by stages 2, 3 and 4 alike, and the
/// scope is dropped rather than measured against a collapsed layout, where an actuator pad and a
/// cell body share a voxel and the numbers look plausible.
///
/// Asked of every stage at once, since what is held is that the three answer one geometry the
/// same way. Both ends of the pad column are rowed, because they saturate onto different things
/// (the actuator column onto a cell, the sensor column onto its own previous pad) and the message
/// names which. Hand-built, because the placement pass refuses this region first; stages 3 and 4
/// meet it from a library caller of `compile_delay` or `compile_crossing`.
#[test]
fn every_stage_refuses_a_reservation_too_shallow_for_its_pad_row() {
    let stages = stages();
    let mut said: Vec<(CollapsedRow, Vec<String>)> = Vec::new();
    for row in [CollapsedRow::OutputOntoCell, CollapsedRow::InputOntoInput] {
        let (kind, index, coord) = row.names();
        let mut primaries = Vec::new();
        for (stage, netlist, phase, run) in &stages {
            let (diagnostics, kept) = run(&collapsed_pad_row(phase, row));
            let refusal = diagnostics
                .iter()
                .find(|d| d.code == DiagnosticCode::RouteCongestion)
                .unwrap_or_else(|| {
                    panic!("the {stage} pass must refuse a collapsed pad row: {diagnostics:?}")
                });
            assert!(
                refusal
                    .primary
                    .contains(&format!("{kind} pad #{index} at {coord}"))
                    && refusal.primary.contains("collapses I/O pads"),
                "the {stage} pass names which pad could not fit, where, and why: {}",
                refusal.primary,
            );
            // The netlist noun is meant to differ between stages, so it is asserted and stripped
            // before the sentences are compared.
            let prefix = format!("{netlist} netlist for ");
            let rest = refusal.primary.strip_prefix(&prefix).unwrap_or_else(|| {
                panic!(
                    "the {stage} pass names the netlist it read: expected {prefix:?}, got {}",
                    refusal.primary,
                )
            });
            primaries.push(rest.to_owned());
            assert_eq!(
                kept,
                vec!["roomy".to_owned()],
                "the {stage} pass elides the refused scope and only that one",
            );
            // The fix line and the span are what an author acts on; the span is the
            // reservation's, because `size=` is the edit.
            let footer = refusal
                .notes
                .iter()
                .find(|n| n.message.starts_with("Fix:"))
                .unwrap_or_else(|| panic!("the {stage} refusal carries a fix line"));
            assert!(
                footer.message.contains("depth >= max(inputs, outputs) + 1"),
                "the fix line gives the same arithmetic as the placement pass: {}",
                footer.message,
            );
            assert_eq!(
                refusal.span,
                row.span(),
                "the {stage} refusal anchors on the `circuit region=` line, not on a cell",
            );
        }
        said.push((row, primaries));
    }

    // One geometry, one sentence, so an author never compares two messages to see they are one
    // finding.
    for (row, primaries) in said {
        assert!(
            primaries.windows(2).all(|w| w[0] == w[1]),
            "{row:?}: the three stages word the refusal identically past the netlist \
             noun, got {primaries:?}",
        );
    }
}

/// A cell-less scope one column wide collapses its sensor pad and its actuator pad onto `(0,0,0)`
/// at any depth, and every stage asks for the width rather than for rows the region already has:
/// the region is four rows deep, and one pad on each edge wants one.
#[test]
fn every_stage_asks_a_one_column_identity_wire_for_a_second_column() {
    let region = reservation(1, 4, 2);
    for (stage, _, phase, run) in &stages() {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.inputs.push(NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        let pad = output_pad(0, PadColumn::of(&ir), &region);
        let mut output = PlacedOutputNode::new(
            cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["d".into()]),
            NetRef::Input(0),
            pad,
            Span::default(),
        );
        output.phase = phase.clone();
        ir.outputs.push(output);
        ir.region = Some(region.clone());

        let (diagnostics, kept) = run(&scoped(ScopeKind::Struct, "wire", ir));
        assert!(kept.is_empty(), "the {stage} pass elides the scope");
        let refusal = diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::RouteCongestion)
            .unwrap_or_else(|| panic!("the {stage} pass refuses: {diagnostics:?}"));
        assert!(
            refusal.primary.contains("output pad #0 at (0,0,0)"),
            "the {stage} pass names the pad that landed second: {}",
            refusal.primary,
        );
        let footer = refusal
            .notes
            .iter()
            .find(|n| n.message.starts_with("Fix:"))
            .unwrap_or_else(|| panic!("the {stage} refusal carries a fix line"));
        assert!(
            footer.message.contains("at least two columns")
                && footer.message.contains("`depth >= max(inputs, outputs)` ")
                && !footer.message.contains("+ 1"),
            "the {stage} fix line asks for the width, and for no cell row: {}",
            footer.message,
        );
    }
}

/// A scope that collapses its pad row *and* strands a sink is refused over the pad row.
///
/// Both are `E_ROUTE_CONGESTION` with one cause, a reserved area too small; the pad-row message
/// says that and names the `size=` line, while the stranded-sink message describes a symptom and
/// sends the author to their cells. Only the first refusal comes out, so the pad row is asked
/// before the trees are grown. The one-input row, with a pad column that fits, is the control
/// and still reports the stranded sink.
#[test]
fn a_scope_that_earns_both_refusals_is_refused_over_the_pad_row() {
    for (inputs, names, why) in [
        // `cannot reach (x,y,z)` rather than `cannot be reached`, which is only in the plural
        // suffix and would key the row on the count of stranded sinks.
        (1, "cannot reach (2,0,0)", "a pad column that fits"),
        (3, "input pad #2", "input #2 saturating onto input #1"),
    ] {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(4, 2, 1));
        for i in 0..inputs {
            ir.inputs.push(NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![format!("s{i}")]),
                span: Span::default(),
            });
        }
        // Input pad #0 lands at (0,0,0). Walling the row at
        // (1,0,0) and the course above the two sinks leaves them
        // nothing to be fed from, and `void=1` reserves no layer
        // to come in over.
        for coord in [
            CellCoord::new(1, 0, 0),
            CellCoord::new(2, 0, 1),
            CellCoord::new(3, 0, 1),
        ] {
            ir.cells.push(placed_cell(
                EditionCell::JavaRepeaterOr,
                coord,
                Vec::new(),
                PlacementPhase::Unrouted,
            ));
        }
        for coord in [CellCoord::new(2, 0, 0), CellCoord::new(3, 0, 0)] {
            ir.cells.push(placed_cell(
                EditionCell::JavaRepeaterOr,
                coord,
                vec![CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                }],
                PlacementPhase::Unrouted,
            ));
        }

        let routed = crate::routing::compile_routing(&scoped(ScopeKind::Struct, "boxed", ir));
        let primaries: Vec<&str> = routed
            .diagnostics
            .iter()
            .map(|d| d.primary.as_str())
            .collect();
        assert!(
            primaries.len() == 1 && primaries[0].contains(names),
            "{inputs} input(s), {why}: expected the one refusal naming `{names}`, got {primaries:?}",
        );
        // One code for both, so the message is all that tells them apart.
        assert_eq!(
            routed.diagnostics[0].code,
            DiagnosticCode::RouteCongestion,
            "{inputs} input(s): both refusals are E_ROUTE_CONGESTION",
        );
    }
}

/// The three stages that lay nets, each with the phase it expects, the netlist noun its refusals
/// name, and a reader for what it produced. A scope's geometry does not vary with the phase, so
/// one fixture is put to all three; the noun is carried so a test can assert it as part of the
/// message.
type Stage = (
    &'static str,
    &'static str,
    PlacementPhase,
    fn(&ScopedPlacementIr) -> (Vec<crate::diagnostic::Diagnostic>, Vec<String>),
);

fn stages() -> [Stage; 3] {
    [
        ("routing", "placed", PlacementPhase::Unrouted, |scoped| {
            let out = crate::routing::compile_routing(scoped);
            (
                out.diagnostics,
                out.scoped.scopes.iter().map(|e| e.name.clone()).collect(),
            )
        }),
        (
            "delay",
            "routed",
            PlacementPhase::Routed { wire_length: 0 },
            |scoped| {
                let out = crate::delay::compile_delay(scoped);
                (
                    out.diagnostics,
                    out.scoped.scopes.iter().map(|e| e.name.clone()).collect(),
                )
            },
        ),
        (
            "crossing",
            "delayed",
            PlacementPhase::Delayed {
                wire_length: 0,
                local_delay_ticks: 0,
            },
            |scoped| {
                let out = crate::crossing::compile_crossing(scoped);
                (
                    out.diagnostics,
                    out.scoped.scopes.iter().map(|e| e.name.clone()).collect(),
                )
            },
        ),
    ]
}

/// A scope with cells and no reservation is `E_NO_CIRCUIT_REGION` at whichever stage meets it,
/// and the scope is dropped: each pass writes a phase field `PlacementPhase` promises after its
/// stage, so the pass that cannot write it says why.
#[test]
fn every_stage_refuses_a_scope_with_cells_and_no_region() {
    for (stage, _, phase, run) in &stages() {
        let (diagnostics, kept) = run(&regionless_scope(phase));
        let refusal = diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::NoCircuitRegion)
            .unwrap_or_else(|| {
                panic!("the {stage} pass must refuse a regionless scope: {diagnostics:?}")
            });
        assert!(
            refusal.primary.contains("struct `roomless`") && refusal.primary.contains(*stage),
            "the {stage} pass names the scope and its own stage: {}",
            refusal.primary,
        );
        assert_eq!(
            kept,
            vec!["roomy".to_owned()],
            "the {stage} pass elides the refused scope and only that one",
        );
    }
}

/// A scope with no reservation and nothing to lay out is handed back at every stage: the refusal
/// is keyed on what the scope carries, not on what it lacks.
#[test]
fn every_stage_passes_an_empty_regionless_scope_through() {
    for (stage, _, _, run) in &stages() {
        let (diagnostics, kept) = run(&scoped(
            ScopeKind::Struct,
            "harmless",
            PlacementIr::new(Edition::Java),
        ));
        assert!(
            diagnostics.is_empty(),
            "the {stage} pass has nothing to say about an empty scope: {diagnostics:?}",
        );
        assert_eq!(kept, vec!["harmless".to_owned()]);
    }
}

/// A driver naming a net no list can answer panics at every stage, naming the variant, the index
/// and the length it ran past. Not a diagnostic: no source produces it, only a hand-built IR, and
/// the crate asserts on those. Both variants, because clamping either would root the net
/// somewhere plausible.
#[test]
fn every_stage_panics_on_a_net_index_the_ir_cannot_answer() {
    for which in [DanglingNet::Cell, DanglingNet::Input] {
        for (stage, _, phase, run) in &stages() {
            let fixture = dangling_net(phase, which);
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(&fixture);
            }))
            .expect_err(&format!(
                "the {stage} pass must not answer a dangling {which:?} net with a coord",
            ));
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("<non-string panic>");
            assert!(
                message.contains(which.expected()),
                "the {stage} pass names the index and the list it ran past, got {message:?}",
            );
        }
    }
}

/// A sink the cap already puts out of reach is refused before a route to it is laid, at every
/// stage that lays one: the straight line is a floor on every route, and laying the wire first
/// costs a coord of search per block, which a `region=` taken from a wide `size=` makes millions.
/// The check sits in the routine all three call, because all three would pay that.
#[test]
fn every_stage_refuses_a_sink_further_than_the_cap_can_reach() {
    // One block past the cap, not comfortably past: the bound is
    // `>`, and a distance far past the end cannot tell that from
    // `>=`. `width - 1` is the pad's distance from the sensor pad.
    let width = MAX_ATTENUATION_SEGMENT + 2;
    for which in [FarSink::Cell, FarSink::OutputPad] {
        for (stage, netlist, phase, run) in &stages() {
            let (diagnostics, kept) = run(&far_sink(phase, width, which));
            let refusal = diagnostics
                .iter()
                .find(|d| d.code == DiagnosticCode::AttenuationLimit)
                .unwrap_or_else(|| {
                    panic!("the {stage} pass must refuse a {which:?} past the cap: {diagnostics:?}")
                });
            assert!(
                refusal.primary.contains(which.label())
                    && refusal.primary.contains(&format!("{} steps", width - 1))
                    && refusal
                        .primary
                        .contains(&format!("limit of {MAX_ATTENUATION_SEGMENT} steps")),
                "the {stage} pass names the sink, the distance and the cap: {}",
                refusal.primary,
            );
            // A constant noun would name the delay pass on the other two paths, `--stage route`
            // over any `.crn` among them.
            assert!(
                refusal
                    .primary
                    .starts_with(&format!("{netlist} netlist for ")),
                "the {stage} pass names the netlist it read: {}",
                refusal.primary,
            );
            assert!(
                refusal
                    .notes
                    .iter()
                    .any(|note| note.message.contains("a larger reservation cannot help")),
                "the fix may not send the author the way the routed-length refusal does, \
                 since nothing shortens a straight line: {:?}",
                refusal.notes,
            );
            assert!(kept.is_empty(), "the {stage} pass elides the refused scope");
        }
    }
}

/// A sink exactly at the cap is routed rather than refused, at every stage: a cheap gate in front
/// of the exact check must not refuse a layout the language allows.
#[test]
fn a_sink_exactly_at_the_cap_is_routed_rather_than_refused() {
    let width = MAX_ATTENUATION_SEGMENT + 1;
    for which in [FarSink::Cell, FarSink::OutputPad] {
        for (stage, _, phase, run) in &stages() {
            let (diagnostics, kept) = run(&far_sink(phase, width, which));
            assert!(
                diagnostics.is_empty(),
                "the {stage} pass accepts a {which:?} at exactly \
                 {MAX_ATTENUATION_SEGMENT}: {diagnostics:?}",
            );
            assert_eq!(kept, vec!["wide".to_owned()]);
        }
    }
}

/// A scope carrying both an over-cap sink and a stranded one is answered by the cap: congestion
/// says to enlarge the reservation, which cannot shorten a straight line already over the cap.
/// The same scope minus its far sink earns congestion, so both faults are present and the order
/// in [`lay_nets`] is what picks.
#[test]
fn an_over_cap_sink_outranks_a_stranded_one() {
    // `void=1` reserves no layer to come in over the top, so
    // walling (1,0,0) and (2,0,1) strands the sink at (2,0,0) — the
    // shape `routing::tests` uses. The far cell at `width - 1` is
    // the over-cap one: nothing stands between it and the pad, so
    // its straight line is the full width.
    let width = MAX_ATTENUATION_SEGMENT + 44;
    let build = |with_far_sink: bool| {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(width, 2, 1));
        ir.inputs.push(NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        let mut push = |coord: CellCoord, drivers: Vec<CellPortDriver>| {
            ir.cells.push(PlacedCellNode {
                cell: EditionCell::JavaRepeaterOr,
                drivers,
                coord,
                phase: PlacementPhase::Unrouted,
                span: Span::default(),
            });
        };
        push(CellCoord::new(1, 0, 0), vec![]);
        push(CellCoord::new(2, 0, 1), vec![]);
        push(
            CellCoord::new(2, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        );
        if with_far_sink {
            push(
                CellCoord::new(width - 1, 0, 0),
                vec![CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                }],
            );
        }
        scoped(ScopeKind::Struct, "both", ir)
    };

    // Without the far sink the scope is a plain congestion refusal,
    // which is what proves the stranded sink is really stranded.
    let stranded_only = crate::routing::compile_routing(&build(false));
    assert_eq!(
        stranded_only
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        vec![DiagnosticCode::RouteCongestion],
        "the stranded sink alone earns congestion: {:?}",
        stranded_only.diagnostics,
    );

    // With both faults present, the cap answers first.
    let both = crate::routing::compile_routing(&build(true));
    assert_eq!(
        both.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::AttenuationLimit],
        "the cap outranks congestion when a scope carries both: {:?}",
        both.diagnostics,
    );
    let footer = both.diagnostics[0]
        .notes
        .iter()
        .find(|n| n.span.is_none())
        .expect("the cap refusal carries a fix footer");
    assert!(
        footer.message.contains("cannot help") && !footer.message.contains("enlarge"),
        "and the author gets the repair that is true for a distance, got {:?}",
        footer.message,
    );
}

/// The walk names a cell before an actuator pad, and the cell and port it actually reached. Two
/// cells and an output pad, all over the cap, so only one ordering and one pair of indices
/// produces the sentence asserted.
#[test]
fn the_walk_names_the_cell_it_reached_before_any_pad() {
    let width = MAX_ATTENUATION_SEGMENT + 44;
    let far = CellCoord::new(width - 1, 0, 0);
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(width, 3, 2));
    ir.inputs.push(NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    // Cell #0 is under the cap from the pad (200 < 256), so the walk
    // has to carry on past it to reach cell #1 — and it is close
    // enough to cell #1 that driving it stays under the cap too.
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        coord: CellCoord::new(200, 0, 0),
        phase: PlacementPhase::Unrouted,
        span: Span::default(),
    });
    // Cell #1's port #1 is the first sink over the cap. Port #0 is
    // driven by the near cell, so the port index has to be the
    // reached one rather than the first.
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: vec![
            CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(0),
            },
            CellPortDriver {
                port: PortName::B,
                net: NetRef::Input(0),
            },
        ],
        coord: far,
        phase: PlacementPhase::Unrouted,
        span: Span::default(),
    });
    // An output pad equally far, on its own coord so the pad-overlap
    // check — which outranks this one — has nothing to say, and is
    // passed over only because cells are walked first.
    ir.outputs.push({
        let mut output = crate::placement_ir::PlacedOutputNode::new(
            cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["out".into()]),
            NetRef::Input(0),
            CellCoord::new(width - 1, 0, 2),
            Span::default(),
        );
        output.phase = PlacementPhase::Unrouted;
        output
    });

    let out = crate::routing::compile_routing(&scoped(ScopeKind::Struct, "walk", ir));
    assert_eq!(
        out.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::AttenuationLimit],
        "one refusal, from the cap: {:?}",
        out.diagnostics,
    );
    let primary = &out.diagnostics[0].primary;
    assert!(
        primary.contains("cell #1 port #1"),
        "the walk must name the cell and port it reached, not the first of each, \
         got {primary:?}",
    );
    assert!(
        !primary.contains("output pad"),
        "cells are walked before pads, so the pad must not be what is named: {primary:?}",
    );
}
