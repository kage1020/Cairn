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
            // Only the netlist noun differs between stages; strip it before comparing.
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

    for (row, primaries) in said {
        assert!(
            primaries.windows(2).all(|w| w[0] == w[1]),
            "{row:?}: the three stages word the refusal identically past the netlist \
             noun, got {primaries:?}",
        );
    }
}

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

#[test]
fn a_scope_that_earns_both_refusals_is_refused_over_the_pad_row() {
    for (inputs, names, why) in [
        // Not `cannot be reached`, which only the plural suffix carries.
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
        assert_eq!(
            routed.diagnostics[0].code,
            DiagnosticCode::RouteCongestion,
            "{inputs} input(s): both refusals are E_ROUTE_CONGESTION",
        );
    }
}

/// A stage that lays nets: its name, its refusals' netlist noun, its expected phase and a runner.
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

#[test]
fn every_stage_panics_on_a_net_index_the_ir_cannot_answer() {
    for which in [DanglingNet::Cell, DanglingNet::Input] {
        for (stage, _, phase, run) in &stages() {
            let fixture = dangling_net(phase, which);
            let message = panic_message(
                || {
                    run(&fixture);
                },
                &format!("the {stage} pass must not answer a dangling {which:?} net with a coord"),
            );
            assert!(
                message.contains(which.expected()),
                "the {stage} pass names the index and the list it ran past, got {message:?}",
            );
        }
    }
}

#[test]
fn every_stage_names_the_actuator_pad_it_refuses_not_the_first_one() {
    // No stage takes `Legalized`, so each one commits pad #0 and refuses pad #1.
    let legalized = PlacementPhase::Legalized {
        wire_length: 0,
        local_delay_ticks: 0,
        buffer_coords: Vec::new(),
    };
    let region = reservation(6, 4, 2);
    let column = PadColumn::for_cell_count(1);
    let [routing, delay, crossing] = stages();
    for ((stage, _, phase, run), method, clause) in [
        (
            routing,
            "route",
            "routing must run exactly once per placement",
        ),
        (
            delay,
            "delay",
            "delay insertion must run exactly once per routed IR",
        ),
        (
            crossing,
            "legalize",
            "crossing legalization must run exactly once per delayed IR",
        ),
    ] {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(region.clone());
        ir.inputs.push(NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(2, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            phase.clone(),
        ));
        for (k, pad_phase) in [phase, legalized.clone()].into_iter().enumerate() {
            let mut output = PlacedOutputNode::new(
                cairn_lang_core::ast::DottedRef::new("sig".into(), vec![format!("out{k}")]),
                NetRef::Cell(0),
                output_pad(k, column, &region),
                Span::default(),
            );
            output.phase = pad_phase;
            ir.outputs.push(output);
        }
        let fixture = scoped(ScopeKind::Struct, "pads", ir);

        let message = panic_message(
            || {
                run(&fixture);
            },
            &format!("the {stage} pass must refuse pad #1"),
        );
        assert_eq!(
            message,
            format!(
                "PlacementPhase::{method} called on {legalized:?} for output #1 at (5,0,2) in \
                 struct `pads` — {clause}"
            ),
            "the {stage} pass names the pad it refused",
        );
    }
}

/// What `body` panics with, failing with `refusal` if it returns instead.
fn panic_message(body: impl FnOnce(), refusal: &str) -> String {
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).expect_err(refusal);
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "<non-string panic>".to_owned())
}

#[test]
fn every_stage_refuses_a_sink_further_than_the_cap_can_reach() {
    // One block past the cap, so `>` cannot pass for `>=`.
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

#[test]
fn an_over_cap_sink_outranks_a_stranded_one() {
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
    // Cell #0 is within the cap of both the pad and cell #1.
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
    // Port #0 is fed by the near cell, so only port #1 is over the cap.
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
    // Equally far, on a coord of its own so the pad-overlap check stays quiet.
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
