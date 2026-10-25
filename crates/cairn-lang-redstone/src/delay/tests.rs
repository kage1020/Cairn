use cairn_lang_core::Edition;
use cairn_lang_core::error::Span;
use strum::IntoEnumIterator;

use super::{
    BUFFER_REPEATER_TICKS, DUST_ATTENUATION_LIMIT, Fed, MAX_ATTENUATION_SEGMENT,
    attribute_local_delay_ticks, buffer_count_for_segment, compile_delay,
};
use crate::diagnostic::DiagnosticCode;
use crate::edition_netlist_ir::EditionCell;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
use crate::placement_ir::{CellCoord, PlacementIr, PlacementPhase, ScopedPlacementIrEntry};
use crate::routing::compile_routing;
use crate::test_fixtures::{ROUTED, placed_cell, reservation, routed_output, scoped, staircase};

#[test]
fn attenuation_cap_measures_the_routed_output_segment() {
    let at_cap = MAX_ATTENUATION_SEGMENT / 2 + 1;
    assert_eq!(2 * at_cap - 2, MAX_ATTENUATION_SEGMENT, "the cap is even");
    for (width, refuses) in [(at_cap, false), (at_cap + 1, true)] {
        let delayed = compile_delay(&scoped(
            ScopeKind::Struct,
            "wide",
            trunk_detour(width, FarSink::Pad),
        ));
        let fired = delayed.diagnostics.iter().any(|d| {
            d.code == DiagnosticCode::AttenuationLimit
                && d.primary.contains(&format!(
                    "has a driver segment of {} steps into output pad #0",
                    2 * width - 2,
                ))
        });
        assert_eq!(
            fired,
            refuses,
            "width {width}: routed {}, cap {MAX_ATTENUATION_SEGMENT}; got {:?}",
            2 * width - 2,
            delayed.diagnostics,
        );
        if !refuses {
            assert_eq!(delayed.diagnostics, Vec::new());
        }
    }
}

#[test]
fn a_detour_from_the_source_is_refused_at_the_cap_by_stage_two() {
    let cap = MAX_ATTENUATION_SEGMENT;
    for (width, refuses) in [(cap - 1, false), (cap, true)] {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(width, 4, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(10, 0, 0),
            Vec::new(),
            ROUTED,
        ));
        ir.outputs.push(routed_output(
            NetRef::Input(0),
            CellCoord::new(width - 1, 0, 0),
        ));
        let delayed = compile_delay(&scoped(ScopeKind::Struct, "wide", ir));
        let refusal = format!(
            "has no route from the driver at (0,0,0) to ({},0,0) within the v1 attenuation \
             limit of {cap} steps",
            width - 1,
        );
        let fired = delayed
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::AttenuationLimit && d.primary.contains(&refusal));
        assert_eq!(
            fired,
            refuses,
            "width {width}: straight line {}, routed {}; got {:?}",
            width - 1,
            width + 1,
            delayed.diagnostics,
        );
        if refuses {
            assert_eq!(delayed.diagnostics.len(), 1, "{:?}", delayed.diagnostics);
            assert!(delayed.scoped.scopes.is_empty(), "the scope is elided");
        } else {
            assert_eq!(delayed.diagnostics, Vec::new());
            assert_eq!(delayed.scoped.scopes.len(), 1);
        }
    }
}

#[test]
fn a_cell_is_charged_once_per_net_that_drives_it() {
    for (label, port_b_net, expected) in [
        (
            "both ports on one net",
            NetRef::Input(0),
            2 + BUFFER_REPEATER_TICKS,
        ),
        (
            "one port each on two nets",
            NetRef::Input(1),
            2 + 2 * BUFFER_REPEATER_TICKS,
        ),
    ] {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, 2));
        for name in ["a", "b"] {
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
                span: Span::default(),
            });
        }
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(16, 0, 1),
            vec![
                CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                },
                CellPortDriver {
                    port: PortName::B,
                    net: port_b_net,
                },
            ],
            ROUTED,
        ));
        let delayed = compile_delay(&scoped(ScopeKind::Struct, "charged", ir));
        assert!(
            delayed.diagnostics.is_empty(),
            "{label}: {:?}",
            delayed.diagnostics,
        );
        assert_eq!(
            delayed.scoped.scopes[0].ir.cells[0].local_delay_ticks(),
            Some(expected),
            "{label}: the side port's 2 plus one charge per driving net",
        );
    }
}

#[test]
fn local_delay_is_the_wire_cost_not_the_arrival_time() {
    const SHORT: u32 = DUST_ATTENUATION_LIMIT + 2;
    const LONG: u32 = 2 * (DUST_ATTENUATION_LIMIT + 1) + 1;
    assert_eq!(buffer_count_for_segment(SHORT), 1);
    assert_eq!(buffer_count_for_segment(LONG), 2);

    // No region or inputs: the fold reaches the nets only through the stand-in `buffers_on`.
    let mut ir = PlacementIr::new(Edition::Java);
    ir.cells.push(placed_cell(
        EditionCell::JavaComparatorAnd,
        CellCoord::new(4, 0, 1),
        vec![
            CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            },
            CellPortDriver {
                port: PortName::B,
                net: NetRef::Input(1),
            },
        ],
        ROUTED,
    ));

    let entry = ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "arrival".to_owned(),
        ir: ir.clone(),
    };
    let buffers_on = |net: NetRef, _sink: CellCoord, _fed: Fed| -> u32 {
        match net {
            NetRef::Input(0) => buffer_count_for_segment(SHORT),
            NetRef::Input(1) => buffer_count_for_segment(LONG),
            other => panic!("no other net drives this cell: {other:?}"),
        }
    };
    attribute_local_delay_ticks(&mut ir, &entry, &buffers_on);

    let recorded = ir.cells[0]
        .local_delay_ticks()
        .expect("delay insertion writes Some(_)");
    let ticks = |segment| buffer_count_for_segment(segment) * BUFFER_REPEATER_TICKS;
    assert_eq!(
        recorded,
        2 + ticks(SHORT) + ticks(LONG),
        "the wire cost is the side port's 2 and every buffer on every net feeding the cell",
    );

    // The arrival time takes the max over the ports, each its own net's buffers and its own delay.
    let arrival = (1 + ticks(SHORT)).max(2 + ticks(LONG));
    assert_eq!(
        recorded - arrival,
        ticks(SHORT),
        "the wire cost sits above the arrival time by the buffers on the shorter segment",
    );
}

#[test]
fn a_cell_is_charged_its_slowest_port_whichever_net_carries_the_buffers() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.cells.push(placed_cell(
        EditionCell::JavaComparatorAnd,
        CellCoord::new(4, 0, 1),
        vec![
            CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            },
            CellPortDriver {
                port: PortName::B,
                net: NetRef::Input(1),
            },
        ],
        ROUTED,
    ));
    let entry = ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "ports".to_owned(),
        ir: ir.clone(),
    };
    let charged = |buffered: Option<NetRef>| {
        let mut ir = ir.clone();
        let buffers_on =
            |net: NetRef, _sink: CellCoord, _fed: Fed| u32::from(Some(net) == buffered);
        attribute_local_delay_ticks(&mut ir, &entry, &buffers_on);
        ir.cells[0].local_delay_ticks()
    };
    assert_eq!(
        [
            charged(None),
            charged(Some(NetRef::Input(0))),
            charged(Some(NetRef::Input(1))),
        ],
        [
            Some(2),
            Some(2 + BUFFER_REPEATER_TICKS),
            Some(2 + BUFFER_REPEATER_TICKS),
        ],
        "no buffer, one on the rear's net, one on the side's",
    );
}

#[test]
fn a_cell_is_charged_the_slowest_of_its_driven_ports_in_any_order() {
    let driver = |port, input| CellPortDriver {
        port,
        net: NetRef::Input(input),
    };
    let charged = |drivers: Vec<CellPortDriver>| {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(4, 0, 1),
            drivers,
            ROUTED,
        ));
        let entry = ScopedPlacementIrEntry {
            kind: ScopeKind::Struct,
            name: "drivers".to_owned(),
            ir: ir.clone(),
        };
        attribute_local_delay_ticks(&mut ir, &entry, &|_net, _sink, _fed| 0);
        ir.cells[0].local_delay_ticks()
    };
    assert_eq!(
        [
            charged(vec![]),
            charged(vec![driver(PortName::A, 0)]),
            charged(vec![driver(PortName::B, 1)]),
            charged(vec![driver(PortName::B, 1), driver(PortName::A, 0)]),
        ],
        [Some(0), Some(1), Some(2), Some(2)],
        "no driver, the rear alone, the side alone, the side listed before the rear",
    );
}

/// The ports the netlist drives on `cell`; the match names every variant, so adding one stops
/// this compiling.
const fn ports(cell: EditionCell) -> &'static [PortName] {
    match cell {
        EditionCell::JavaComparatorAnd
        | EditionCell::BedrockTorchAnd
        | EditionCell::JavaRepeaterOr
        | EditionCell::BedrockTorchOr
        | EditionCell::JavaXorUnpinned
        | EditionCell::BedrockXorUnpinned
        | EditionCell::JavaNandUnpinned
        | EditionCell::BedrockNandUnpinned
        | EditionCell::JavaNorUnpinned
        | EditionCell::BedrockNorUnpinned => &[PortName::A, PortName::B],
        EditionCell::JavaInverterTorch | EditionCell::BedrockInverterTorch => &[PortName::A],
        EditionCell::JavaMuxUnpinned | EditionCell::BedrockMuxUnpinned => {
            &[PortName::Sel, PortName::A, PortName::B]
        }
    }
}

#[test]
fn each_port_counts_the_stages_between_it_and_the_output() {
    let table = [
        (EditionCell::JavaComparatorAnd, PortName::A, 1),
        (EditionCell::JavaComparatorAnd, PortName::B, 2),
        (EditionCell::BedrockTorchAnd, PortName::A, 2),
        (EditionCell::BedrockTorchAnd, PortName::B, 2),
        (EditionCell::JavaRepeaterOr, PortName::A, 1),
        (EditionCell::JavaRepeaterOr, PortName::B, 1),
        (EditionCell::BedrockTorchOr, PortName::A, 0),
        (EditionCell::BedrockTorchOr, PortName::B, 0),
        (EditionCell::JavaInverterTorch, PortName::A, 1),
        (EditionCell::BedrockInverterTorch, PortName::A, 1),
        (EditionCell::JavaXorUnpinned, PortName::A, 3),
        (EditionCell::JavaXorUnpinned, PortName::B, 3),
        (EditionCell::BedrockXorUnpinned, PortName::A, 3),
        (EditionCell::BedrockXorUnpinned, PortName::B, 3),
        (EditionCell::JavaNandUnpinned, PortName::A, 3),
        (EditionCell::JavaNandUnpinned, PortName::B, 3),
        (EditionCell::BedrockNandUnpinned, PortName::A, 3),
        (EditionCell::BedrockNandUnpinned, PortName::B, 3),
        (EditionCell::JavaNorUnpinned, PortName::A, 3),
        (EditionCell::JavaNorUnpinned, PortName::B, 3),
        (EditionCell::BedrockNorUnpinned, PortName::A, 3),
        (EditionCell::BedrockNorUnpinned, PortName::B, 3),
        (EditionCell::JavaMuxUnpinned, PortName::Sel, 3),
        (EditionCell::JavaMuxUnpinned, PortName::A, 3),
        (EditionCell::JavaMuxUnpinned, PortName::B, 3),
        (EditionCell::BedrockMuxUnpinned, PortName::Sel, 3),
        (EditionCell::BedrockMuxUnpinned, PortName::A, 3),
        (EditionCell::BedrockMuxUnpinned, PortName::B, 3),
    ];
    let rows: Vec<_> = table.iter().map(|&(cell, port, _)| (cell, port)).collect();
    let every: Vec<_> = EditionCell::iter()
        .flat_map(|cell| ports(cell).iter().map(move |&port| (cell, port)))
        .collect();
    assert_eq!(rows, every, "one row per port of every variant");
    let counted: Vec<_> = table
        .iter()
        .map(|&(cell, port, _)| (cell, port, cell.port_delay_ticks(port)))
        .collect();
    assert_eq!(counted, table);
}

#[test]
fn only_the_merge_and_the_comparator_pass_strength_on() {
    let table = [
        (EditionCell::JavaComparatorAnd, false),
        (EditionCell::BedrockTorchAnd, true),
        (EditionCell::JavaRepeaterOr, true),
        (EditionCell::BedrockTorchOr, false),
        (EditionCell::JavaInverterTorch, true),
        (EditionCell::BedrockInverterTorch, true),
        (EditionCell::JavaXorUnpinned, false),
        (EditionCell::BedrockXorUnpinned, false),
        (EditionCell::JavaNandUnpinned, false),
        (EditionCell::BedrockNandUnpinned, false),
        (EditionCell::JavaNorUnpinned, false),
        (EditionCell::BedrockNorUnpinned, false),
        (EditionCell::JavaMuxUnpinned, false),
        (EditionCell::BedrockMuxUnpinned, false),
    ];
    let every: Vec<_> = EditionCell::iter()
        .map(|cell| (cell, cell.regenerates()))
        .collect();
    assert_eq!(
        every, table,
        "one row per variant, in declaration order, each with its `regenerates`",
    );
}

#[test]
fn a_stretch_with_nowhere_for_a_repeater_is_refused() {
    let routed = compile_routing(&staircase(&PlacementPhase::Unrouted));
    assert_eq!(routed.diagnostics, Vec::new(), "stage 2 lays the staircase");
    let stairs = &routed.scoped.scopes[0];
    assert_eq!(stairs.name, "stairs");
    assert_eq!(stairs.ir.cells.len(), 18, "one sink and 17 walls");
    assert_eq!(
        stairs.ir.cells[0].phase.wire_length(),
        Some(18),
        "the one 18-block way through",
    );

    let delayed = compile_delay(&routed.scoped);
    let codes: Vec<_> = delayed.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, vec![DiagnosticCode::AttenuationLimit]);
    let primary = &delayed.diagnostics[0].primary;
    assert!(
        primary.contains("routed netlist for struct `stairs` routes sig.a so that the signal leaving (0,0,0) runs out before (8,0,8), past the attenuation limit of 15 blocks of dust, and never reaches cell #0:"),
        "the refusal names the net, the stretch and the node that goes dark, got {primary:?}",
    );
    let footer = &delayed.diagnostics[0].notes[0].message;
    assert!(
        footer.contains("run straight at least once in every 16 blocks"),
        "the fix is room to run straight, got {footer:?}",
    );
    let survivors: Vec<_> = delayed.scoped.scopes.iter().map(|e| &e.name).collect();
    assert_eq!(survivors, vec!["roomy"]);
}

/// What the trunk in [`trunk_detour`] leads round the wall to.
#[derive(Clone, Copy)]
enum FarSink {
    /// A cell, as cell #2.
    Cell,
    /// An actuator pad, as output #0.
    Pad,
}

/// A segment over the cap only the routed-length check sees: `far` hangs off the trunk, round a
/// wall, `2 * width - 2` routed against 2 in a straight line.
fn trunk_detour(width: u32, far: FarSink) -> PlacementIr {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(width, 3, 1));
    let driven = || {
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Cell(0),
        }]
    };
    let far_coord = CellCoord::new(0, 0, 2);
    ir.cells.push(placed_cell(
        EditionCell::JavaComparatorAnd,
        CellCoord::new(0, 0, 0),
        vec![],
        ROUTED,
    ));
    ir.cells.push(placed_cell(
        EditionCell::JavaComparatorAnd,
        CellCoord::new(width - 1, 0, 0),
        driven(),
        ROUTED,
    ));
    match far {
        FarSink::Cell => ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            far_coord,
            driven(),
            ROUTED,
        )),
        FarSink::Pad => ir.outputs.push(routed_output(NetRef::Cell(0), far_coord)),
    }
    for x in 0..width - 2 {
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(x, 0, 1),
            vec![],
            ROUTED,
        ));
    }
    ir
}

/// A width whose branch the router lays within the cap but whose routed total is over it.
const OVER_CAP_TRUNK: u32 = 201;
const _: () = assert!(OVER_CAP_TRUNK <= MAX_ATTENUATION_SEGMENT);
const _: () = assert!(2 * OVER_CAP_TRUNK - 2 > MAX_ATTENUATION_SEGMENT);

#[test]
fn cell_driver_attenuation_primary_names_cell_and_port() {
    let delayed = compile_delay(&scoped(
        ScopeKind::Struct,
        "wide",
        trunk_detour(OVER_CAP_TRUNK, FarSink::Cell),
    ));
    let attenuation = delayed
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::AttenuationLimit)
        .expect("cell-driver segment past cap must fire E_ATTENUATION_LIMIT");
    assert!(
        attenuation.primary.contains(&format!(
            "has a driver segment of {} steps into cell #2",
            2 * OVER_CAP_TRUNK - 2,
        )),
        "primary must name the failing cell index and the trunk-plus-branch \
         length, got {:?}",
        attenuation.primary,
    );
    assert!(
        attenuation.primary.contains("port #0"),
        "primary must name the failing driver port, got {:?}",
        attenuation.primary,
    );
    assert!(
        delayed.scoped.scopes.is_empty(),
        "failed scope must be elided",
    );
}

#[test]
fn a_detour_refusal_leaves_its_sibling_alone() {
    let detour = trunk_detour(OVER_CAP_TRUNK, FarSink::Cell);

    let mut roomy = PlacementIr::new(Edition::Java);
    roomy.region = Some(reservation(16, 3, 3));
    roomy.cells.push(placed_cell(
        EditionCell::JavaComparatorAnd,
        CellCoord::new(1, 0, 0),
        vec![],
        ROUTED,
    ));
    roomy.cells.push(placed_cell(
        EditionCell::JavaComparatorAnd,
        CellCoord::new(3, 0, 0),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Cell(0),
        }],
        ROUTED,
    ));

    let mut input = scoped(ScopeKind::Struct, "detour", detour);
    input.scopes.push(ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "roomy".to_owned(),
        ir: roomy,
    });

    let delayed = compile_delay(&input);

    let codes: Vec<_> = delayed.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        vec![DiagnosticCode::AttenuationLimit],
        "only the detour scope is refused, and only once: {:?}",
        delayed.diagnostics,
    );
    let refusal = &delayed.diagnostics[0];
    assert!(
        refusal.primary.contains("struct `detour`"),
        "the refusal must name the scope it belongs to, got {:?}",
        refusal.primary,
    );
    assert!(
        refusal.primary.contains("has a driver segment of")
            && !refusal.primary.contains("in a straight line"),
        "this is the routed-length check, not the straight-line gate, got {:?}",
        refusal.primary,
    );

    let survivors: Vec<_> = delayed.scoped.scopes.iter().map(|e| &e.name).collect();
    assert_eq!(
        survivors,
        vec!["roomy"],
        "the sibling survives the detour scope's refusal",
    );
    assert!(
        delayed.scoped.scopes[0]
            .ir
            .cells
            .iter()
            .all(|c| c.local_delay_ticks().is_some()),
        "and is delayed in full rather than half-written",
    );
}

#[test]
fn missing_region_with_cells_fires_no_circuit_region() {
    // Only an IR that skipped placement has cells and no region.
    let mut ir = PlacementIr::new(Edition::Java);
    ir.cells.push(placed_cell(
        EditionCell::JavaComparatorAnd,
        CellCoord::new(0, 0, 0),
        vec![],
        ROUTED,
    ));
    let delayed = compile_delay(&scoped(ScopeKind::Struct, "roomless", ir));
    let diag = delayed
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::NoCircuitRegion)
        .expect("missing region with cells must fire E_NO_CIRCUIT_REGION");
    assert!(
        diag.primary.contains("struct `roomless`"),
        "primary must name the scope, got {:?}",
        diag.primary,
    );
    assert!(
        delayed.scoped.scopes.is_empty(),
        "failed scope must elide even though it carried cells",
    );
}

#[test]
fn buffer_count_boundary_table() {
    for (steps, expected_buffers) in [
        (0_u32, 0_u32),
        (1, 0),
        (15, 0),
        (16, 0),
        (17, 1),
        (31, 1),
        (32, 1),
        (33, 2),
        (48, 2),
        (49, 3),
        (MAX_ATTENUATION_SEGMENT, 15),
    ] {
        assert_eq!(
            buffer_count_for_segment(steps),
            expected_buffers,
            "segment of {steps} steps",
        );
    }
}

#[test]
#[should_panic(expected = "topological invariant broken")]
fn out_of_range_net_ref_cell_panics_loudly() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(5, 3, 2));
    ir.cells.push(placed_cell(
        EditionCell::JavaComparatorAnd,
        CellCoord::new(0, 0, 0),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Cell(u32::MAX),
        }],
        ROUTED,
    ));
    let _ = compile_delay(&scoped(ScopeKind::Struct, "broken", ir));
}

#[test]
#[should_panic(
    expected = "for cell #1 at (4,0,1) in struct `mixed` — delay insertion must run exactly once per routed IR"
)]
fn delay_panic_names_the_offending_cell_not_the_first_one() {
    // A `Delayed` second cell, so the panic cannot name `cells[0]` by accident.
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(8, 3, 2));
    ir.cells.push(placed_cell(
        EditionCell::JavaComparatorAnd,
        CellCoord::new(0, 0, 0),
        vec![],
        ROUTED,
    ));
    let mut already_delayed = placed_cell(
        EditionCell::JavaComparatorAnd,
        CellCoord::new(4, 0, 1),
        vec![],
        ROUTED,
    );
    already_delayed.phase = PlacementPhase::Delayed {
        wire_length: 0,
        local_delay_ticks: 0,
    };
    ir.cells.push(already_delayed);
    let _ = compile_delay(&scoped(ScopeKind::Struct, "mixed", ir));
}
