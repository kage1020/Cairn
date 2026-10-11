//! Delay-pass behaviours `tests/redstone/delay.rs` cannot reach through synth
//! fixtures: shapes only a hand-built `PlacementIr` produces.

use std::collections::HashSet;

use cairn_lang_core::Edition;
use cairn_lang_core::error::Span;

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

/// The sanity cap counts the dust, not the distance, on the segment into an actuator pad too.
///
/// [`trunk_detour`] with the far sink a pad routes `2 * width - 2` against a straight line of 2:
/// exactly the cap at `width = MAX_ATTENUATION_SEGMENT / 2 + 1` and two over one wider, so the
/// boundary is pinned from both sides. The fixture builds only even lengths; the odd side is
/// pinned by [`a_detour_from_the_source_is_refused_at_the_cap_by_stage_two`]. The wording is
/// asserted too, since the straight-line gate and the router's bound raise the same code.
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

/// A detour laid from the source in one piece is refused by stage 2, at the cap and not a block
/// past it, before this pass measures it.
///
/// A sensor pad at `(0,0,0)` drives an actuator pad at the far edge with a cell on the straight
/// line between, so the route is `width + 1`, over the top or beside. At
/// `width = MAX_ATTENUATION_SEGMENT - 1` that is the cap; one wider, the router's bound refuses
/// it through [`crate::pass::lay_nets`], so the refusal is stage 2's wording under this code.
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

/// A cell is charged once per net that drives it, not once per port: two ports on one net are fed
/// by one strand and the repeaters on it.
///
/// The second row, two ports on two nets, is the control: two segments, both charged, so a fold
/// that dropped a net or charged a cell once reports a different number. (The segments are 16
/// and 17 blocks, one buffer each; the rows differ in the number of nets, not the lengths.)
#[test]
fn a_cell_is_charged_once_per_net_that_drives_it() {
    for (label, port_b_net, expected) in [
        (
            "both ports on one net",
            NetRef::Input(0),
            1 + BUFFER_REPEATER_TICKS,
        ),
        (
            "one port each on two nets",
            NetRef::Input(1),
            1 + 2 * BUFFER_REPEATER_TICKS,
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
            "{label}: base 1 plus one charge per driving net",
        );
    }
}

/// `local_delay_ticks` is the wire cost of every net feeding a cell, summed, not the tick the
/// cell's output settles on.
///
/// With one buffer on the `a` segment and two on `b`, three blocks stand on the wires that feed
/// the cell, and stage 4 lays all three
/// (`two_distinct_nets_charge_a_cell_for_every_buffer_under_it`). The output settles at
/// `base + 2`, when the last input arrives. Both figures come from the same two lengths, so a
/// fold that became a max would make them equal and trip the assert.
///
/// Calls the fold with a stand-in `buffers_on`, since what is pinned is how per-net charges
/// combine; `a_cell_is_charged_once_per_net_that_drives_it` measures real routed segments.
#[test]
fn local_delay_is_the_wire_cost_not_the_arrival_time() {
    // One buffer on the short segment, two on the long one.
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

    let base = EditionCell::JavaComparatorAnd.base_delay_ticks();
    let recorded = ir.cells[0]
        .local_delay_ticks()
        .expect("delay insertion writes Some(_)");
    let ticks = |segment| buffer_count_for_segment(segment) * BUFFER_REPEATER_TICKS;
    assert_eq!(
        recorded,
        base + ticks(SHORT) + ticks(LONG),
        "the wire cost is every buffer on every net feeding the cell",
    );

    // What the output actually settles on: the longer segment, through the `max` an arrival walk
    // would take.
    let arrival = base + ticks(SHORT).max(ticks(LONG));
    assert_eq!(
        recorded - arrival,
        ticks(SHORT),
        "the wire cost sits above the arrival time by the buffers on the shorter segment",
    );
}

/// Every cell, and whether its output is at full strength whatever reached it: only the merge and
/// the comparator pass strength on, and every `*Unpinned` placeholder, having no realisation,
/// answers `false`.
///
/// `listed` fails to compile when a variant is added and the length check fails when one is left
/// out, because `strand_invariant` reads this same method and a wrong entry would fool it too.
#[test]
fn only_the_merge_and_the_comparator_pass_strength_on() {
    const fn listed(cell: EditionCell) {
        match cell {
            EditionCell::JavaComparatorAnd
            | EditionCell::BedrockTorchAnd
            | EditionCell::JavaRepeaterOr
            | EditionCell::BedrockTorchOr
            | EditionCell::JavaInverterTorch
            | EditionCell::BedrockInverterTorch
            | EditionCell::JavaXorUnpinned
            | EditionCell::BedrockXorUnpinned
            | EditionCell::JavaNandUnpinned
            | EditionCell::BedrockNandUnpinned
            | EditionCell::JavaNorUnpinned
            | EditionCell::BedrockNorUnpinned
            | EditionCell::JavaMuxUnpinned
            | EditionCell::BedrockMuxUnpinned => {}
        }
    }
    let table = [
        (EditionCell::JavaComparatorAnd, false),
        (EditionCell::BedrockTorchOr, false),
        (EditionCell::JavaRepeaterOr, true),
        (EditionCell::BedrockTorchAnd, true),
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
    let distinct: HashSet<EditionCell> = table.iter().map(|(cell, _)| *cell).collect();
    assert_eq!(distinct.len(), 14, "every variant, once");
    for (cell, regenerates) in table {
        listed(cell);
        assert_eq!(cell.regenerates(), regenerates, "{cell:?}");
    }
}

/// A net with a stretch past the limit on which every coord turns
/// is refused, rather than charged for a repeater stage 4 could
/// only put on a turn.
///
/// Through stage 2 first: the router lays the staircase itself, and
/// raises nothing, so this is a tree stage 2 hands on and stage 3
/// has to answer.
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

/// A segment over the cap that only the routed-length check can see.
///
/// Cell #0 drives cell #1 at the far end of a row `width - 1` long, and `far` two blocks away
/// across a wall of cells ending two columns short of the row's end. The tree reaches `far` round
/// the wall off the trunk laid for cell #1, so its route is `2 * width - 2` against a straight
/// line of 2. The straight-line gate and the router's bound, which sees only the branch, both pass
/// it; a detour laid from the source in one piece would be refused by the bound first. Hand-built,
/// since placement puts every cell on one row.
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

/// The width [`trunk_detour`] is built at for the over-cap side: its branch, `width` blocks, is 55
/// under the cap, so the router lays it, and its total, `2 * width - 2`, is 144 over, so only this
/// pass refuses it.
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

/// The delay pass's own attenuation refusal, for a route over the cap because it goes round
/// something, drops its scope without disturbing a sibling. No `.crn` can write this shape (see
/// [`trunk_detour`]), so the integration fixture covers the routing pass instead.
#[test]
fn a_detour_refusal_leaves_its_sibling_alone() {
    // Scope one: the detour fixture, which clears the straight-line
    // gate and the router's bound and fails the routed length.
    let detour = trunk_detour(OVER_CAP_TRUNK, FarSink::Cell);

    // Scope two: roomy, and nothing in it is near the cap.
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
    // Which check answered: both say "routed netlist" under one code, so only the wording of the
    // measurement tells them apart. The gate's phrase is asserted absent so a fixture that drifted
    // into tripping the straight-line gate fails.
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

/// Boundary values of `buffer_count_for_segment`, in steps from a fresh source to its sink: 16
/// steps is 15 blocks of dust and needs no repeater, 17 needs one, and each repeater carries the
/// signal 16 steps further. Row by row, so counting the sink as dust (`(s - 1) / 15`) or dropping
/// the `- 1` (`s / 16`) trips the rows it moves.
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
    // Re-running the whole pass trips on `cells[0]`, which cannot tell a hard-coded index from the
    // real one; a `Routed` first cell and a `Delayed` second one put the panic on `cells[1]`.
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
