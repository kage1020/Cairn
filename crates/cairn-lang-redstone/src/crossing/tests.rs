use cairn_lang_core::Edition;
use cairn_lang_core::error::Span;

use std::collections::{HashMap, HashSet};

use super::compile_crossing;
use crate::delay::{BUFFER_REPEATER_TICKS, compile_delay, slowest_port_ticks};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::edition_netlist_ir::EditionCell;
use crate::grid::{PadColumn, input_pad};
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
use crate::placement_ir::{BufferSegment, PlacedOutputNode};
use crate::placement_ir::{
    CellCoord, PlacedCellNode, PlacementIr, PlacementPhase, RouteLayer, ScopedPlacementIr,
    ScopedPlacementIrEntry,
};
use crate::routing::compile_routing;
use crate::routing_geometry::{Router, block_sites, collect_nets, net_trees};
use crate::test_fixtures::{DELAYED, placed_cell, reservation, scoped, staircase};

#[test]
fn no_scopes_input_yields_no_scopes_output() {
    let legalized = compile_crossing(&ScopedPlacementIr::new());
    assert_eq!(legalized.diagnostics, [] as [Diagnostic; 0]);
    assert_eq!(legalized.scoped.scopes, [] as [ScopedPlacementIrEntry; 0]);
}

#[test]
fn empty_scope_passes_through_untouched() {
    // Dropping an empty scope is `ScopedPlacementIr::push`'s job, not this pass's.
    let ir = PlacementIr::new(Edition::Java);
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "empty", ir));
    assert_eq!(legalized.diagnostics, [] as [Diagnostic; 0]);
    assert_eq!(legalized.scoped.scopes.len(), 1);
    assert_eq!(
        legalized.scoped.scopes[0].ir.cells,
        [] as [PlacedCellNode; 0]
    );
}

#[test]
fn single_net_no_buffer_is_untouched() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(5, 3, 2));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    // Off the origin, where the cell would stand on input pad #0 and need no buffer at all.
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(1, 0, 1),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        DELAYED,
    ));
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "single", ir));
    assert!(
        legalized.diagnostics.is_empty(),
        "a two-block segment needs nothing: {:?}",
        legalized.diagnostics,
    );
    let cell = &legalized.scoped.scopes[0].ir.cells[0];
    assert!(
        cell.buffer_coords().is_empty(),
        "a segment of at most 16 steps needs no buffer, got {:?}",
        cell.buffer_coords(),
    );
    assert_eq!(
        cell.coord.layer,
        RouteLayer::Plane,
        "cell coord stays on plane",
    );
}

#[test]
fn long_segment_places_buffer_on_plane() {
    // 17 steps carry 16 blocks of dust, one past the attenuation limit.
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(20, 3, 2));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(17, 0, 0),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        DELAYED,
    ));
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "long", ir));
    assert!(
        legalized.diagnostics.is_empty(),
        "clean fixture: {:?}",
        legalized.diagnostics,
    );
    let cell = &legalized.scoped.scopes[0].ir.cells[0];
    assert_eq!(
        cell.buffer_coords().len(),
        1,
        "17-step segment needs exactly one buffer, got {:?}",
        cell.buffer_coords(),
    );
    assert_eq!(
        cell.buffer_coords()[0].coord.layer,
        RouteLayer::Plane,
        "no collision → buffer stays on plane",
    );
    assert_eq!(
        cell.buffer_coords()[0].port,
        BufferSegment::Port(PortName::A),
        "buffer preserves its driver port on the plane placement path",
    );
    let json = serde_json::to_string(&legalized.scoped)
        .expect("legalized scoped IR must serialise cleanly");
    assert!(
        json.contains("\"buffer_coords\":[{\"port\":\"a\",\"coord\":{\"x\":16,\"y\":0,\"z\":0}}]"),
        "expected buffer_coords entry to appear in JSON verbatim, got {json}",
    );
    assert!(
        json.contains("\"stage\":\"crossing\""),
        "expected the crossing stage tag alongside buffer_coords, got {json}",
    );
}

/// A sink 15 steps from its pad, with a block at `(7,0,0)` whose detour makes 17 steps.
fn walled_scope(void: u32) -> ScopedPlacementIr {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(20, 3, void));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(7, 0, 0),
        Vec::new(),
        DELAYED,
    ));
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(15, 0, 0),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        DELAYED,
    ));
    scoped(ScopeKind::Struct, "walled", ir)
}

const WALLED_SINK: usize = 1;

/// `(14,0,1)`, not the turn into the sink at `(15,0,1)`, where a repeater would face away from it.
#[test]
fn buffer_lands_on_the_routed_path_not_the_straight_line() {
    let legalized = compile_crossing(&walled_scope(2));
    assert!(
        legalized.diagnostics.is_empty(),
        "the candidate is free wire: {:?}",
        legalized.diagnostics,
    );
    let cells = &legalized.scoped.scopes[0].ir.cells;
    let buffers = cells[WALLED_SINK].buffer_coords();
    assert_eq!(buffers.len(), 1, "16 blocks of dust need one: {buffers:?}");
    assert_eq!(buffers[0].coord, CellCoord::new(14, 0, 1));
    assert_eq!(buffers[0].coord.layer, RouteLayer::Plane);
}

/// [`walled_scope`] with a second net, `sig.b`, along the row the `sig.a` route comes back down.
fn two_net_walled_scope() -> ScopedPlacementIr {
    let mut scope = walled_scope(2);
    let ir = &mut scope.scopes[0].ir;
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["b".into()]),
        span: Span::default(),
    });
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(13, 0, 0),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(1),
        }],
        DELAYED,
    ));
    scope
}

/// Against `wire_path`: the `route_to` the allocator reads would compare the path to itself.
#[test]
fn every_buffer_stands_on_dust_the_routing_pass_laid() {
    let legalized = compile_crossing(&two_net_walled_scope());
    assert!(
        legalized.diagnostics.is_empty(),
        "the fixture routes: {:?}",
        legalized.diagnostics,
    );
    let ir = &legalized.scoped.scopes[0].ir;
    let region = ir.region.clone().expect("fixture carries a region");
    let nets = collect_nets(ir);
    assert!(
        nets.len() >= 2,
        "the fixture needs a second net for the cross-net half to run: {nets:?}",
    );
    let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();
    let router = Router::new(&region, &block_sites(ir, &region));
    let trees = net_trees(&nets, &router, |net| match net {
        NetRef::Input(i) => input_pad(i as usize, PadColumn::of(ir), &region),
        NetRef::Cell(j) => cell_coords[j as usize],
    });
    let owned: HashMap<NetRef, HashSet<CellCoord>> = trees
        .iter()
        .map(|(net, tree)| (*net, tree.wire_path().into_iter().collect()))
        .collect();

    let mut checked = 0;
    let mut cross_checks = 0;
    for (index, cell) in ir.cells.iter().enumerate() {
        for buffer in cell.buffer_coords() {
            checked += 1;
            let BufferSegment::Port(port) = buffer.port else {
                panic!("a cell's buffer must name one of its driver ports");
            };
            let driver = cell
                .drivers
                .iter()
                .find(|d| d.port == port)
                .expect("every buffer names a driver of its own cell");
            assert!(
                owned[&driver.net].iter().any(|dust| {
                    (dust.x, dust.z) == (buffer.coord.x, buffer.coord.z) && dust.y <= buffer.coord.y
                }),
                "buffer {:?} on cell #{index} is not over dust the routing pass laid for {:?}",
                buffer.coord,
                driver.net,
            );
            assert_eq!(
                buffer.coord.layer == RouteLayer::Plane,
                buffer.coord.y == 0,
                "the layer a buffer carries follows its height: {:?}",
                buffer.coord,
            );
            for (other, dust) in &owned {
                if *other == driver.net {
                    continue;
                }
                cross_checks += 1;
                assert!(
                    !dust.contains(&buffer.coord),
                    "buffer {:?} shorts onto {other:?}'s wire",
                    buffer.coord,
                );
            }
        }
    }
    assert!(
        checked >= 1,
        "the fixture has to emit a buffer to mean anything"
    );
    assert!(
        cross_checks >= 1,
        "the cross-net half never ran — the fixture lost its second net",
    );
}

/// Distinct coords, since two segments of one net share the repeater on their prefix.
#[test]
fn the_buffer_count_matches_the_ticks_delay_charged() {
    let mut routed = walled_scope(2);
    for cell in &mut routed.scopes[0].ir.cells {
        cell.phase = PlacementPhase::Routed { wire_length: 0 };
    }
    let delayed = crate::delay::compile_delay(&routed);
    assert!(
        delayed.diagnostics.is_empty(),
        "17 steps is inside the sanity cap: {:?}",
        delayed.diagnostics,
    );
    let legalized = compile_crossing(&delayed.scoped);
    assert!(
        legalized.diagnostics.is_empty(),
        "{:?}",
        legalized.diagnostics
    );

    for (index, cell) in legalized.scoped.scopes[0].ir.cells.iter().enumerate() {
        let charged = cell
            .local_delay_ticks()
            .expect("stage 3 wrote the ticks")
            .saturating_sub(slowest_port_ticks(cell));
        let blocks: HashSet<CellCoord> = cell.buffer_coords().iter().map(|b| b.coord).collect();
        let placed = u32::try_from(blocks.len()).expect("small");
        assert_eq!(
            charged,
            placed.saturating_mul(BUFFER_REPEATER_TICKS),
            "cell #{index} was charged for {charged} ticks of buffer but got {placed} block(s)",
        );
    }
}

/// Two nets whose straight lines share the row `z=1`, with neither sink on the other's line.
fn two_nets_that_want_one_row_scope(void: u32) -> ScopedPlacementIr {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(40, 4, void));
    for name in ["a", "b"] {
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
            span: Span::default(),
        });
    }
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(30, 0, 1),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(1),
        }],
        DELAYED,
    ));
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(20, 0, 0),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        DELAYED,
    ));
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(10, 0, 2),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Cell(0),
        }],
        DELAYED,
    ));
    scoped(ScopeKind::Struct, "shared", ir)
}

/// Routed alone, two of the nets would share a coord, which the test asserts first.
#[test]
fn two_nets_that_want_one_row_are_laid_apart() {
    let scope = two_nets_that_want_one_row_scope(3);
    let legalized = compile_crossing(&scope);
    assert!(
        legalized.diagnostics.is_empty(),
        "there is room for both, so neither is refused: {:?}",
        legalized.diagnostics,
    );

    let ir = &legalized.scoped.scopes[0].ir;
    let region = ir.region.clone().expect("the fixture carries a region");
    let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();
    let router = Router::new(&region, &block_sites(ir, &region));
    let nets = collect_nets(ir);
    let trees = net_trees(&nets, &router, |net| match net {
        NetRef::Input(i) => input_pad(i as usize, PadColumn::of(ir), &region),
        NetRef::Cell(j) => cell_coords[j as usize],
    });

    let alone: Vec<HashSet<CellCoord>> = nets
        .iter()
        .map(|(net, sinks)| {
            let source = match net {
                NetRef::Input(i) => input_pad(*i as usize, PadColumn::of(ir), &region),
                NetRef::Cell(j) => cell_coords[*j as usize],
            };
            router
                .dust(&router.tree(source, sinks, &HashSet::new()))
                .into_iter()
                .collect()
        })
        .collect();
    assert!(
        alone
            .iter()
            .enumerate()
            .any(|(i, one)| alone[i + 1..].iter().any(|two| !one.is_disjoint(two))),
        "the fixture only means something while two of these nets want one \
         coord when each is routed against the blocks alone",
    );

    // Not through `beside`, so a mutant that empties `beside` fails here.
    let dust: Vec<(NetRef, Vec<CellCoord>)> = trees
        .iter()
        .map(|(net, tree)| (*net, router.dust(tree)))
        .collect();
    for (index, (net, mine)) in dust.iter().enumerate() {
        for (other, theirs) in &dust[index + 1..] {
            for a in mine {
                for b in theirs {
                    assert!(
                        a.y != b.y || a.x.abs_diff(b.x) + a.z.abs_diff(b.z) > 1,
                        "{net:?} lays dust on {a:?} and {other:?} on {b:?}, \
                         which is one strand carrying two signals",
                    );
                }
            }
        }
    }
}

/// The repeater stands on `(14,0,0)`, before the fork at `(15,0,0)` where it would starve a branch.
#[test]
fn a_shared_prefix_carries_one_repeater_however_many_sinks_hang_off_it() {
    for (sinks, void) in [(2u32, 3u32), (3, 2), (2, 1), (3, 1)] {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, void));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        for x in 16..16 + sinks {
            ir.cells.push(placed_cell(
                EditionCell::JavaRepeaterOr,
                CellCoord::new(x, 0, 0),
                vec![CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                }],
                DELAYED,
            ));
        }
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "fanout", ir));
        assert!(
            legalized.diagnostics.is_empty(),
            "{sinks} sinks at void={void} need one repeater and no escape: {:?}",
            legalized.diagnostics,
        );
        for (index, cell) in legalized.scoped.scopes[0].ir.cells.iter().enumerate() {
            let buffers = cell.buffer_coords();
            assert_eq!(
                buffers.len(),
                1,
                "cell #{index} of {sinks} at void={void}: {buffers:?}",
            );
            assert_eq!(
                buffers[0].coord,
                CellCoord::new(14, 0, 0),
                "cell #{index} of {sinks} at void={void} must name the repeater standing on the shared prefix, before the fork",
            );
        }
    }
}

#[test]
fn a_buffer_candidate_is_never_a_cell_body() {
    let legalized = compile_crossing(&cell_on_the_fifteen_step_point());
    assert!(
        legalized.diagnostics.is_empty(),
        "the route goes round the body, so nothing collides: {:?}",
        legalized.diagnostics,
    );
    let cells = &legalized.scoped.scopes[0].ir.cells;
    let buffers = cells[1].buffer_coords();
    assert_eq!(
        buffers.len(),
        1,
        "the segment is over 15 blocks: {buffers:?}"
    );
    assert_eq!(
        buffers[0].coord.layer,
        RouteLayer::Plane,
        "and it stands on the plane, with no layer needed: {buffers:?}",
    );
    assert_ne!(
        buffers[0].coord,
        CellCoord::new(15, 0, 1),
        "the straight-line 15-step point is the cell body",
    );
}

/// `sig.b`'s cell standing on `sig.a`'s straight-line 15-step point.
fn cell_on_the_fifteen_step_point() -> ScopedPlacementIr {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(20, 3, 1));
    for name in ["a", "b"] {
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
            span: Span::default(),
        });
    }
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(15, 0, 1),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(1),
        }],
        DELAYED,
    ));
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(16, 0, 1),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        DELAYED,
    ));
    scoped(ScopeKind::Struct, "packed", ir)
}

/// Two buffers on `sig.a`, one on the cell's net: three ticks of buffer, where a max over the nets
/// would charge two.
#[test]
fn two_distinct_nets_charge_a_cell_for_every_buffer_under_it() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(48, 3, 2));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: Vec::new(),
        coord: CellCoord::new(20, 0, 2),
        phase: PlacementPhase::Unrouted,
        span: Span::default(),
    });
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaComparatorAnd,
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
        coord: CellCoord::new(40, 0, 2),
        phase: PlacementPhase::Unrouted,
        span: Span::default(),
    });
    let routed = compile_routing(&scoped(ScopeKind::Struct, "two-nets", ir));
    assert!(routed.diagnostics.is_empty(), "{:?}", routed.diagnostics);
    let delayed = compile_delay(&routed.scoped);
    assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
    let legalized = compile_crossing(&delayed.scoped);
    assert!(
        legalized.diagnostics.is_empty(),
        "three repeaters on free wire need no escape: {:?}",
        legalized.diagnostics,
    );

    let cell = &legalized.scoped.scopes[0].ir.cells[1];
    let buffers = cell.buffer_coords();
    let per_port = |port| {
        buffers
            .iter()
            .filter(|b| b.port == BufferSegment::Port(port))
            .count()
    };
    assert_eq!(
        (per_port(PortName::A), per_port(PortName::B)),
        (1, 2),
        "one buffer on the cell segment, two on the sensor's: {buffers:?}",
    );
    let distinct: HashSet<CellCoord> = buffers.iter().map(|b| b.coord).collect();
    assert_eq!(
        distinct.len(),
        3,
        "two distinct nets share no repeater: {buffers:?}",
    );

    assert_eq!(
        cell.local_delay_ticks(),
        Some(2 + 3 * BUFFER_REPEATER_TICKS),
        "the side port's 2 plus a charge for each of the three blocks",
    );
    let slowest = slowest_port_ticks(cell);
    let ticks = cell
        .local_delay_ticks()
        .expect("legalized cells carry Some(local_delay_ticks)");
    let blocks = u32::try_from(distinct.len()).expect("three blocks fit in u32");
    assert_eq!(
        ticks - slowest,
        blocks * BUFFER_REPEATER_TICKS,
        "every block stage 4 laid is a tick stage 3 charged",
    );
    let widest = u32::try_from(per_port(PortName::A).max(per_port(PortName::B)))
        .expect("two buffers fit in u32");
    assert_ne!(
        ticks - slowest,
        widest * BUFFER_REPEATER_TICKS,
        "a max over the driving nets would drop the shorter segment's block",
    );
}

/// Run through all three passes, each of which writes one figure and must count the segment once.
#[test]
fn ports_sharing_a_net_share_the_repeater_the_charge_and_the_dust() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(20, 3, 2));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaComparatorAnd,
        drivers: vec![
            CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            },
            CellPortDriver {
                port: PortName::B,
                net: NetRef::Input(0),
            },
        ],
        coord: CellCoord::new(17, 0, 0),
        phase: PlacementPhase::Unrouted,
        span: Span::default(),
    });
    let routed = compile_routing(&scoped(ScopeKind::Struct, "shared", ir));
    assert!(routed.diagnostics.is_empty(), "{:?}", routed.diagnostics);
    let delayed = compile_delay(&routed.scoped);
    assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
    let legalized = compile_crossing(&delayed.scoped);
    assert!(
        legalized.diagnostics.is_empty(),
        "one repeater on free wire needs no escape: {:?}",
        legalized.diagnostics,
    );

    let cell = &legalized.scoped.scopes[0].ir.cells[0];
    assert_eq!(
        cell.wire_length(),
        Some(17),
        "one strand of dust, measured once",
    );
    assert_eq!(
        cell.local_delay_ticks(),
        Some(2 + BUFFER_REPEATER_TICKS),
        "the side port's 2 plus the one repeater the signal passes through",
    );
    let buffers = cell.buffer_coords();
    assert_eq!(
        buffers.iter().map(|b| b.port).collect::<Vec<_>>(),
        vec![
            BufferSegment::Port(PortName::A),
            BufferSegment::Port(PortName::B),
        ],
        "one attribution per driver, in driver order",
    );
    let distinct: HashSet<CellCoord> = buffers.iter().map(|b| b.coord).collect();
    assert_eq!(
        distinct,
        [CellCoord::new(16, 0, 0)].into_iter().collect(),
        "both attributions name the one block: {buffers:?}",
    );
}

/// The route forks at `(16,0,0)`, into the cell and round it, so the shared repeater is before it.
#[test]
fn an_actuator_segment_records_the_repeater_a_cell_segment_placed() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(31, 3, 2));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        coord: CellCoord::new(17, 0, 0),
        phase: PlacementPhase::Unrouted,
        span: Span::default(),
    });
    ir.outputs.push(PlacedOutputNode::new(
        cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        NetRef::Input(0),
        CellCoord::new(30, 0, 0),
        Span::default(),
    ));
    let routed = compile_routing(&scoped(ScopeKind::Struct, "both", ir));
    assert!(routed.diagnostics.is_empty(), "{:?}", routed.diagnostics);
    let delayed = compile_delay(&routed.scoped);
    assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
    let legalized = compile_crossing(&delayed.scoped);
    assert!(
        legalized.diagnostics.is_empty(),
        "the pad's segment reuses the cell's repeater: {:?}",
        legalized.diagnostics,
    );

    let ir = &legalized.scoped.scopes[0].ir;
    let shared = CellCoord::new(15, 0, 0);
    assert_eq!(
        ir.cells[0]
            .buffer_coords()
            .iter()
            .map(|b| (b.port, b.coord))
            .collect::<Vec<_>>(),
        vec![(BufferSegment::Port(PortName::A), shared)],
    );
    assert_eq!(
        ir.outputs[0]
            .buffer_coords()
            .iter()
            .map(|b| (b.port, b.coord))
            .collect::<Vec<_>>(),
        vec![
            (BufferSegment::Out, shared),
            (BufferSegment::Out, CellCoord::new(29, 0, 1)),
        ],
        "the wire to the pad names the block the cell segment put there",
    );
}

/// `sig.sel` turns off `z=0` at `(14,0,0)`, so its repeater stands on `(15,0,1)`; `sig.port_a` has
/// no row left and climbs at its pad, so its repeater is on a bridge layer.
#[test]
fn mux_ports_each_keep_the_coord_their_own_segment_reaches() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(22, 5, 3));
    for name in ["sel", "blocker", "port_a", "port_b"] {
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
            span: Span::default(),
        });
    }
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(15, 0, 0),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(1),
        }],
        DELAYED,
    ));
    ir.cells.push(placed_cell(
        EditionCell::JavaMuxUnpinned,
        CellCoord::new(18, 0, 1),
        vec![
            CellPortDriver {
                port: PortName::Sel,
                net: NetRef::Input(0),
            },
            CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(2),
            },
            CellPortDriver {
                port: PortName::B,
                net: NetRef::Input(3),
            },
        ],
        DELAYED,
    ));
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "mux", ir));
    assert!(
        legalized.diagnostics.is_empty(),
        "the fixture routes: {:?}",
        legalized.diagnostics,
    );
    let bufs = legalized.scoped.scopes[0].ir.cells[1].buffer_coords();
    assert_eq!(
        bufs.iter().map(|b| (b.port, b.coord)).collect::<Vec<_>>(),
        vec![
            (BufferSegment::Port(PortName::Sel), CellCoord::new(15, 0, 1),),
            (BufferSegment::Port(PortName::A), CellCoord::new(11, 1, 2),),
            (BufferSegment::Port(PortName::B), CellCoord::new(16, 0, 4),),
        ],
        "each port keeps its own segment's coord across both push sites",
    );
    let json = serde_json::to_string(&legalized.scoped)
        .expect("legalized scoped IR must serialise cleanly");
    for fragment in ["\"port\":\"sel\"", "\"port\":\"a\"", "\"port\":\"b\""] {
        assert!(
            json.contains(fragment),
            "JSON wire form must carry {fragment}, got {json}",
        );
    }
}

#[test]
#[should_panic(
    expected = "for cell #0 at (16,0,1) in struct `twice` — crossing legalization must run exactly once per delayed IR"
)]
fn re_running_crossing_pass_panics_loudly() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(20, 3, 2));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(16, 0, 1),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        DELAYED,
    ));
    let first = compile_crossing(&scoped(ScopeKind::Struct, "twice", ir));
    let _second = compile_crossing(&first.scoped);
}

/// No cells, so only the loop over the actuator pads can refuse the second run.
#[test]
#[should_panic(
    expected = "for output #0 at (5,0,1) in struct `twice` — crossing legalization must run exactly once per delayed IR"
)]
fn re_running_crossing_pass_over_a_pad_names_the_output() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(6, 3, 2));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    let mut output = PlacedOutputNode::new(
        cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["out".into()]),
        NetRef::Input(0),
        CellCoord::new(5, 0, 1),
        Span::default(),
    );
    output.phase = DELAYED;
    ir.outputs.push(output);
    let first = compile_crossing(&scoped(ScopeKind::Struct, "twice", ir));
    let _second = compile_crossing(&first.scoped);
}

#[test]
fn missing_region_with_cells_fires_no_circuit_region() {
    // Only an IR that skipped placement has cells and no region.
    let mut ir = PlacementIr::new(Edition::Java);
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(0, 0, 0),
        vec![],
        DELAYED,
    ));
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "roomless", ir));
    let diag = legalized
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
        legalized.scoped.scopes.is_empty(),
        "failed scope must elide",
    );
}

#[test]
fn missing_region_empty_scope_passes_through() {
    let ir = PlacementIr::new(Edition::Java);
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "harmless", ir));
    assert_eq!(legalized.diagnostics, [] as [Diagnostic; 0]);
    assert_eq!(legalized.scoped.scopes.len(), 1);
    assert!(legalized.scoped.scopes[0].ir.region.is_none());
}

#[test]
fn output_only_scope_with_missing_region_refuses() {
    // `PlacementIr::is_empty` is false while outputs exist, so this is not the empty pass-through.
    let mut ir = PlacementIr::new(Edition::Java);
    ir.outputs.push(PlacedOutputNode::new(
        cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["x".into()]),
        NetRef::Input(0),
        CellCoord::new(3, 0, 1),
        Span::default(),
    ));
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "outputless", ir));
    assert!(
        legalized
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::NoCircuitRegion),
        "missing region with outputs must refuse: {:?}",
        legalized.diagnostics,
    );
}

/// Stage 3 refuses the staircase, so the routed IR is promoted to `Delayed` by hand.
#[test]
fn a_stretch_with_nowhere_for_a_repeater_is_refused() {
    let routed = compile_routing(&staircase(&PlacementPhase::Unrouted));
    assert_eq!(routed.diagnostics, Vec::new(), "stage 2 lays the staircase");
    let delayed = compile_delay(&routed.scoped);
    let [stage_3] = delayed.diagnostics.as_slice() else {
        panic!("stage 3 refuses once: {:?}", delayed.diagnostics);
    };

    let mut promoted = routed.scoped.clone();
    for entry in &mut promoted.scopes {
        let phases = entry
            .ir
            .cells
            .iter_mut()
            .map(|cell| &mut cell.phase)
            .chain(entry.ir.outputs.iter_mut().map(|output| &mut output.phase));
        for phase in phases {
            let PlacementPhase::Routed { wire_length } = *phase else {
                panic!("stage 2 leaves every node routed: {phase:?}");
            };
            *phase = PlacementPhase::Delayed {
                wire_length,
                local_delay_ticks: 0,
            };
        }
    }
    let legalized = compile_crossing(&promoted);
    let codes: Vec<_> = legalized.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, vec![DiagnosticCode::AttenuationLimit]);
    let primary = &legalized.diagnostics[0].primary;
    assert!(
        primary.contains("delayed netlist for struct `stairs` routes sig.a so that the signal leaving (0,0,0) runs out before (8,0,8)"),
        "the refusal names the net and the stretch, got {primary:?}",
    );
    assert_eq!(
        primary.replacen("delayed netlist", "routed netlist", 1),
        stage_3.primary,
        "stage 4 refuses the stretch stage 3 did",
    );
    assert_eq!(legalized.diagnostics[0].notes, stage_3.notes);
    let survivors: Vec<_> = legalized.scoped.scopes.iter().map(|e| &e.name).collect();
    assert_eq!(survivors, vec!["roomy"]);
}

/// The first repeater steps back off the corner at `(16,0,0)`, leaving one block too many ahead.
#[test]
fn a_turn_at_the_limit_costs_the_route_a_second_repeater() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(20, 18, 1));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        coord: CellCoord::new(16, 0, 16),
        phase: PlacementPhase::Unrouted,
        span: Span::default(),
    });
    let routed = compile_routing(&scoped(ScopeKind::Struct, "corner", ir));
    assert!(routed.diagnostics.is_empty(), "{:?}", routed.diagnostics);
    let delayed = compile_delay(&routed.scoped);
    assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
    let legalized = compile_crossing(&delayed.scoped);
    assert!(
        legalized.diagnostics.is_empty(),
        "{:?}",
        legalized.diagnostics
    );

    let cell = &legalized.scoped.scopes[0].ir.cells[0];
    assert_eq!(cell.wire_length(), Some(32), "the fixture is the 32-step L");
    assert_eq!(
        cell.buffer_coords()
            .iter()
            .map(|b| b.coord)
            .collect::<Vec<_>>(),
        vec![CellCoord::new(15, 0, 0), CellCoord::new(16, 0, 15)],
    );
    assert_eq!(
        cell.local_delay_ticks(),
        Some(EditionCell::JavaRepeaterOr.port_delay_ticks(PortName::A) + 2 * BUFFER_REPEATER_TICKS),
        "stage 3 charges for both blocks stage 4 lays",
    );
}

/// Cell #1's wire crosses over cell #0's bridge at `y=2` and comes straight down two blocks onto
/// its pad. `(29,1,3)`, where its second repeater falls due, runs straight but not at one height,
/// so the repeater steps back over the top of the descent onto `(28,2,3)`.
#[test]
fn a_repeater_due_on_a_straight_descent_steps_back_onto_the_bridge() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(30, 7, 3));
    let walls = (19..=21).flat_map(|x| (1..=5).map(move |z| CellCoord::new(x, 0, z)));
    for coord in [CellCoord::new(20, 0, 0), CellCoord::new(0, 0, 3)]
        .into_iter()
        .chain(walls)
    {
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            coord,
            Vec::new(),
            PlacementPhase::Unrouted,
        ));
    }
    let pad = CellCoord::new(29, 0, 3);
    for (driver, coord) in [
        (NetRef::Cell(0), CellCoord::new(20, 0, 6)),
        (NetRef::Cell(1), pad),
    ] {
        ir.outputs.push(PlacedOutputNode::new(
            cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["out".into()]),
            driver,
            coord,
            Span::default(),
        ));
    }
    let routed = compile_routing(&scoped(ScopeKind::Struct, "descent", ir));
    let codes: Vec<_> = routed.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, vec![DiagnosticCode::RouteCrossLayerClearance]);

    let (_, trees) = buffer_delay_invariant::laid_trees(&routed.scoped.scopes[0]);
    let route = trees[&NetRef::Cell(1)]
        .route_to(pad)
        .expect("the pad is a terminal of the net driving it");
    assert_eq!(
        route[route.len() - 3..],
        [CellCoord::new(29, 2, 3), CellCoord::new(29, 1, 3), pad],
        "the shape no longer comes straight down onto the pad: {route:?}",
    );

    let delayed = compile_delay(&routed.scoped);
    assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
    let legalized = compile_crossing(&delayed.scoped);
    assert!(
        legalized.diagnostics.is_empty(),
        "{:?}",
        legalized.diagnostics
    );
    let output = &legalized.scoped.scopes[0].ir.outputs[1];
    assert_eq!(output.wire_length(), Some(33));
    assert_eq!(
        output
            .buffer_coords()
            .iter()
            .map(|b| b.coord)
            .collect::<Vec<_>>(),
        vec![CellCoord::new(16, 0, 3), CellCoord::new(28, 2, 3)],
    );
    assert_eq!(output.local_delay_ticks(), Some(2 * BUFFER_REPEATER_TICKS));
}

#[test]
fn buffer_coord_index_at_kth_boundary() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(52, 3, 3));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(48, 0, 1),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        DELAYED,
    ));
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "boundary", ir));
    assert!(
        legalized.diagnostics.is_empty(),
        "clean fixture: {:?}",
        legalized.diagnostics,
    );
    let bufs = legalized.scoped.scopes[0].ir.cells[0].buffer_coords();
    assert_eq!(
        bufs.iter().map(|b| b.coord).collect::<Vec<_>>(),
        vec![
            CellCoord::new(16, 0, 0),
            CellCoord::new(32, 0, 0),
            CellCoord::new(47, 0, 0),
        ],
        "k=1, 2 at path[k * 16]; k=3 one short of the turn at path[48]",
    );
    for b in bufs {
        assert_eq!(
            b.coord.layer,
            RouteLayer::Plane,
            "no collision → every k-th buffer stays on plane; got {b:?}",
        );
    }
}

#[test]
fn buffer_coord_index_at_max_segment_boundary() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(256, 3, 3));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(255, 0, 1),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        DELAYED,
    ));
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "max_boundary", ir));
    assert!(
        legalized.diagnostics.is_empty(),
        "segment == MAX_ATTENUATION_SEGMENT must legalize cleanly: {:?}",
        legalized.diagnostics,
    );
    let bufs = legalized.scoped.scopes[0].ir.cells[0].buffer_coords();
    let expected: Vec<CellCoord> = (1..=15u32).map(|k| CellCoord::new(k * 16, 0, 0)).collect();
    assert_eq!(
        bufs.iter().map(|b| b.coord).collect::<Vec<_>>(),
        expected,
        "segment 256 → 15 buffers",
    );
}

#[test]
#[should_panic(expected = "topological invariant broken")]
fn out_of_range_net_ref_cell_panics_loudly() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(5, 3, 2));
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(0, 0, 0),
        vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Cell(u32::MAX),
        }],
        DELAYED,
    ));
    let _ = compile_crossing(&scoped(ScopeKind::Struct, "broken", ir));
}

#[test]
#[should_panic(
    expected = "for cell #1 at (4,0,1) in struct `mixed` — crossing legalization must run exactly once per delayed IR"
)]
fn legalize_panic_names_the_offending_cell_not_the_first_one() {
    // A `Delayed` first cell, so the panic cannot name `cells[0]` by accident.
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(8, 3, 2));
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(0, 0, 0),
        vec![],
        DELAYED,
    ));
    let mut already_legalized = placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(4, 0, 1),
        vec![],
        DELAYED,
    );
    already_legalized.phase = PlacementPhase::Legalized {
        wire_length: 0,
        local_delay_ticks: 0,
        buffer_coords: Vec::new(),
    };
    ir.cells.push(already_legalized);
    let _ = compile_crossing(&scoped(ScopeKind::Struct, "mixed", ir));
}

mod strand_invariant {
    //! The attenuation limit along whole strands: the dust counted from the last block that
    //! restored strength, through the cells that pass it on.

    use std::collections::HashMap;
    use std::fmt::Write as _;

    use cairn_lang_core::{Edition, lower, parse};

    use super::{
        CellCoord, CellPortDriver, EditionCell, HashSet, NetRef, PadColumn, PlacedCellNode,
        PlacedOutputNode, PlacementIr, PlacementPhase, PortName, Router, ScopeKind, Span,
        block_sites, collect_nets, compile_crossing, input_pad, net_trees, reservation, scoped,
    };
    use crate::delay::{DUST_ATTENUATION_LIMIT, compile_delay};
    use crate::routing::compile_routing;
    use crate::{compile_edition_netlist, compile_netlist, compile_placement, synthesize};

    /// `source` through every stage to the legalized IR of its one scope.
    fn legalized(source: &str, edition: Edition) -> PlacementIr {
        let intent = lower(&parse(source).expect("the fixture parses"));
        let synth = synthesize(&intent);
        let netlist = compile_netlist(&synth.scoped);
        let placed = compile_placement(&compile_edition_netlist(&netlist, edition), &intent);
        assert!(placed.diagnostics.is_empty(), "{:?}", placed.diagnostics);
        let routed = compile_routing(&placed.scoped);
        assert!(
            routed
                .diagnostics
                .iter()
                .all(|d| d.code == crate::DiagnosticCode::RouteCrossLayerClearance),
            "{:?}",
            routed.diagnostics,
        );
        let delayed = compile_delay(&routed.scoped);
        assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
        let legalized = compile_crossing(&delayed.scoped);
        assert!(
            legalized.diagnostics.is_empty(),
            "{:?}",
            legalized.diagnostics
        );
        legalized.scoped.scopes[0].ir.clone()
    }

    /// Asserts no block of dust is past the limit along any strand, counting the IR's repeaters,
    /// and returns how many it met. It is one-sided: a repeater too many passes.
    fn assert_no_strand_runs_past_the_limit(ir: &PlacementIr, label: &str) -> usize {
        let region = ir.region.clone().expect("the fixture carries a region");
        let nets = collect_nets(ir);
        let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();
        let router = Router::new(&region, &block_sites(ir, &region));
        let trees = net_trees(&nets, &router, |net| match net {
            NetRef::Input(i) => input_pad(i as usize, PadColumn::of(ir), &region),
            NetRef::Cell(j) => cell_coords[j as usize],
        });
        let mut repeaters: HashMap<NetRef, HashSet<CellCoord>> = HashMap::new();
        for cell in &ir.cells {
            for buffer in cell.buffer_coords() {
                let crate::placement_ir::BufferSegment::Port(port) = buffer.port else {
                    panic!("a cell's buffer names one of its ports");
                };
                let net = cell
                    .drivers
                    .iter()
                    .find(|d| d.port == port)
                    .expect("a driver of its own cell")
                    .net;
                repeaters.entry(net).or_default().insert(buffer.coord);
            }
        }
        for output in &ir.outputs {
            for buffer in output.buffer_coords() {
                repeaters
                    .entry(output.driver)
                    .or_default()
                    .insert(buffer.coord);
            }
        }

        let mut order: Vec<NetRef> = trees.keys().copied().collect();
        order.sort_by_key(|net| match net {
            NetRef::Input(i) => (0, *i),
            NetRef::Cell(j) => (1, *j),
        });
        let mut arrived: HashMap<(NetRef, CellCoord), u32> = HashMap::new();
        let none = HashSet::new();
        for net in order {
            let at_source = match net {
                NetRef::Input(_) => 0,
                NetRef::Cell(j) => {
                    let cell = &ir.cells[j as usize];
                    if cell.cell.regenerates() {
                        0
                    } else {
                        cell.drivers
                            .iter()
                            .map(|d| arrived[&(d.net, cell.coord)])
                            .max()
                            .unwrap_or(0)
                    }
                }
            };
            let tree = &trees[&net];
            let reps = repeaters.get(&net).unwrap_or(&none);
            let mut spent: HashMap<CellCoord, u32> = HashMap::new();
            let path = tree.wire_path();
            spent.insert(path[0], at_source);
            let parents: HashSet<CellCoord> =
                path[1..].iter().filter_map(|c| tree.parent(*c)).collect();
            for coord in &path[1..] {
                let parent = tree.parent(*coord).expect("attached");
                let behind = if reps.contains(&parent) {
                    0
                } else {
                    spent[&parent]
                };
                // A sink or a repeater reads the dust before it, so it is allowed one block more.
                let here = behind + 1;
                let is_dust = parents.contains(coord) && !reps.contains(coord);
                if is_dust {
                    assert!(
                        here <= DUST_ATTENUATION_LIMIT,
                        "{label}: {net:?} lays dust on {coord:?} as block {here} since the last block that restores strength",
                    );
                } else {
                    assert!(
                        here <= DUST_ATTENUATION_LIMIT + 1,
                        "{label}: {net:?} reaches {coord:?} over {} blocks of dust since the last block that restores strength",
                        here - 1,
                    );
                }
                spent.insert(*coord, here);
                arrived.insert((net, *coord), here);
            }
        }
        repeaters.values().map(HashSet::len).sum()
    }

    /// Four plates into one door through three cells that pass strength on: no single segment is
    /// past the limit, but the strand is.
    fn four_plates(op: &str) -> String {
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

    /// Sixteen Java comparators in a chain, each also reading the shared sensor `sig.b`.
    fn comparator_chain() -> String {
        let mut source = String::from(
            r"
theme t:
  slot wall -> @oak_planks

struct chain size=60x5
  floor mat_slot=wall
  pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=pb at=inside.front offset=1 y=0 -> sig.b
  logic sig.c0 = sig.a and sig.b
",
        );
        for i in 1..16 {
            writeln!(source, "  logic sig.c{i} = sig.c{} and sig.b", i - 1)
                .expect("writing to a String cannot fail");
        }
        source.push_str(
            "  door id=d side=front at=center mat_slot=wall opened_by=sig.c15\n  circuit region=floor void=2\n",
        );
        source
    }

    /// The sensor pad at `(0, 0, 0)` drives a `kind` cell on `cell`, which drives a pad on each of
    /// `pads`.
    fn one_cell_by_hand(
        edition: Edition,
        kind: EditionCell,
        cell: CellCoord,
        pads: &[CellCoord],
    ) -> PlacementIr {
        let mut ir = PlacementIr::new(edition);
        ir.region = Some(reservation(20, 4, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(PlacedCellNode {
            cell: kind,
            drivers: vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            coord: cell,
            phase: PlacementPhase::Unrouted,
            span: Span::default(),
        });
        for (index, pad) in pads.iter().enumerate() {
            ir.outputs.push(PlacedOutputNode::new(
                cairn_lang_core::ast::DottedRef::new("sig".into(), vec![format!("out{index}")]),
                NetRef::Cell(0),
                *pad,
                Span::default(),
            ));
        }
        let routed = compile_routing(&scoped(ScopeKind::Struct, "by_hand", ir));
        assert!(routed.diagnostics.is_empty(), "{:?}", routed.diagnostics);
        let delayed = compile_delay(&routed.scoped);
        assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
        let legalized = compile_crossing(&delayed.scoped);
        assert!(
            legalized.diagnostics.is_empty(),
            "{:?}",
            legalized.diagnostics
        );
        legalized.scoped.scopes[0].ir.clone()
    }

    /// Every cell's buffer coords, then every actuator pad's, in IR order.
    fn every_buffer(ir: &PlacementIr) -> (Vec<Vec<CellCoord>>, Vec<Vec<CellCoord>>) {
        let coords = |buffers: &[crate::placement_ir::BufferCoord]| {
            buffers.iter().map(|b| b.coord).collect::<Vec<_>>()
        };
        (
            ir.cells.iter().map(|c| coords(c.buffer_coords())).collect(),
            ir.outputs
                .iter()
                .map(|o| coords(o.buffer_coords()))
                .collect(),
        )
    }

    /// The strand runs through the merge as its 6th block of dust, so its 16th is the corner at
    /// `(16, 0, 0)` and the repeater steps back to `(15, 0, 0)`.
    #[test]
    fn a_strand_through_a_dust_merge_steps_back_off_a_turn_once() {
        let ir = one_cell_by_hand(
            Edition::Bedrock,
            EditionCell::BedrockTorchOr,
            CellCoord::new(6, 0, 0),
            &[CellCoord::new(16, 0, 2)],
        );
        assert_eq!(assert_no_strand_runs_past_the_limit(&ir, "turn"), 1);
        assert_eq!(
            every_buffer(&ir),
            (vec![Vec::new()], vec![vec![CellCoord::new(15, 0, 0)]]),
            "nothing on the wire into the merge, and one repeater off the corner",
        );
    }

    /// The strand reaches the comparator with 3 blocks spent, so the branch's 16th block is the
    /// turn at `(15, 0, 1)`, and the repeater steps back over the fork to `(14, 0, 0)`.
    #[test]
    fn a_strand_through_a_comparator_steps_back_off_a_fork_once() {
        let ir = one_cell_by_hand(
            Edition::Java,
            EditionCell::JavaComparatorAnd,
            CellCoord::new(3, 0, 0),
            &[CellCoord::new(16, 0, 0), CellCoord::new(16, 0, 1)],
        );
        assert_eq!(assert_no_strand_runs_past_the_limit(&ir, "fork"), 1);
        let trunk = vec![CellCoord::new(14, 0, 0)];
        assert_eq!(
            every_buffer(&ir),
            (vec![Vec::new()], vec![trunk.clone(), trunk]),
            "nothing on the wire into the comparator, and one repeater on the trunk for both pads",
        );
    }

    #[test]
    fn a_strand_through_cells_that_pass_strength_on_is_buffered_as_one() {
        for (label, source, edition) in [
            ("bedrock or", four_plates("or"), Edition::Bedrock),
            ("java and", four_plates("and"), Edition::Java),
            ("comparator chain", comparator_chain(), Edition::Java),
        ] {
            let ir = legalized(&source, edition);
            assert!(
                ir.cells.iter().all(|c| !c.cell.regenerates()),
                "{label}: the fixture is a chain of cells that pass strength on",
            );
            let met = assert_no_strand_runs_past_the_limit(&ir, label);
            assert!(
                met >= 1,
                "{label}: the strand is past the limit, so it needs a repeater"
            );
        }
    }
}

mod buffer_delay_invariant {
    //! Property tests for the crossing and delay agreement, here because their fixtures hand-build
    //! `Unrouted` nodes through `pub(crate)` items.
    use proptest::prelude::*;

    use super::{
        BufferSegment, CellCoord, CellPortDriver, DiagnosticCode, Edition, EditionCell, HashMap,
        HashSet, NetRef, PadColumn, PlacedCellNode, PlacedOutputNode, PlacementIr, PlacementPhase,
        PortName, RouteLayer, Router, ScopeKind, ScopedPlacementIr, Span, block_sites,
        collect_nets, compile_crossing, input_pad, net_trees, reservation, scoped,
    };
    use crate::delay::{BUFFER_REPEATER_TICKS, compile_delay, slowest_port_ticks};
    use crate::placement_ir::{BufferCoord, CellIdentity, ScopedPlacementIrEntry};
    use crate::routing::compile_routing;
    use crate::routing_geometry::{NetTree, Unreached};

    /// One `(x, z, second port on the same net)` per cell, deduplicated in a `prop_map` over this
    /// same tuple, since a new shape would replay the seeds in `proptest-regressions/` as others.
    fn scope_strategy() -> impl Strategy<Value = Vec<(u32, u32, bool)>> {
        prop::collection::vec((1u32..=99u32, 0u32..8u32, prop::bool::ANY), 1..=3).prop_map(
            |mut cells| {
                let mut seen: HashSet<(u32, u32)> = HashSet::new();
                cells.retain(|(x, z, _)| seen.insert((*x, *z)));
                cells
            },
        )
    }

    /// One cell at `(x, 0, z)` per entry, each driven from `sig.a`.
    fn scope_ir(xs: &[(u32, u32, bool)]) -> PlacementIr {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(200, 10, 3));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        for &(x, z, shared_port) in xs {
            let mut drivers = vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }];
            if shared_port {
                drivers.push(CellPortDriver {
                    port: PortName::B,
                    net: NetRef::Input(0),
                });
            }
            ir.cells.push(PlacedCellNode {
                cell: EditionCell::JavaRepeaterOr,
                drivers,
                coord: CellCoord::new(x, 0, z),
                phase: PlacementPhase::Unrouted,
                span: Span::default(),
            });
        }
        ir
    }

    /// A cell or actuator pad of [`pad_scope_ir`]: `(x, z, the net driving it)`.
    type Node = (u32, u32, NetRef);

    /// Up to two cells, each on `sig.a` or `sig.b`, and one or two actuator pads, each on a sensor
    /// or a cell. A cell that draws a pad's coord is dropped, so every draw keeps a pad.
    fn pad_scope_strategy() -> impl Strategy<Value = (Vec<Node>, Vec<Node>)> {
        let sensor = prop_oneof![Just(NetRef::Input(0)), Just(NetRef::Input(1))];
        let cells = prop::collection::vec((1u32..=99u32, 0u32..8u32, sensor), 0..=2);
        let pads = prop::collection::vec(
            (1u32..=99u32, 0u32..8u32, any::<prop::sample::Index>()),
            1..=2,
        );
        (cells, pads).prop_map(|(cells, pads)| {
            let mut seen: HashSet<(u32, u32)> = HashSet::new();
            let pads: Vec<_> = pads
                .into_iter()
                .filter(|(x, z, _)| seen.insert((*x, *z)))
                .collect();
            let cells: Vec<Node> = cells
                .into_iter()
                .filter(|(x, z, _)| seen.insert((*x, *z)))
                .collect();
            let drivers: Vec<NetRef> = [NetRef::Input(0), NetRef::Input(1)]
                .into_iter()
                .chain((0..cells.len()).map(|j| {
                    NetRef::Cell(u32::try_from(j).expect("two cells fit the u32 a NetRef carries"))
                }))
                .collect();
            let pads = pads
                .into_iter()
                .map(|(x, z, driver)| (x, z, *driver.get(&drivers)))
                .collect();
            (cells, pads)
        })
    }

    /// [`scope_ir`]'s reservation with a second sensor, `sig.b`, a cell at `(x, 0, z)` per entry of
    /// `cells` and an actuator pad at `(x, 0, z)` per entry of `pads`.
    fn pad_scope_ir(cells: &[Node], pads: &[Node]) -> PlacementIr {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(200, 10, 3));
        for name in ["a", "b"] {
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
                span: Span::default(),
            });
        }
        for &(x, z, net) in cells {
            ir.cells.push(PlacedCellNode {
                cell: EditionCell::JavaRepeaterOr,
                drivers: vec![CellPortDriver {
                    port: PortName::A,
                    net,
                }],
                coord: CellCoord::new(x, 0, z),
                phase: PlacementPhase::Unrouted,
                span: Span::default(),
            });
        }
        for (index, &(x, z, driver)) in pads.iter().enumerate() {
            ir.outputs.push(PlacedOutputNode::new(
                cairn_lang_core::ast::DottedRef::new("sig".into(), vec![format!("out{index}")]),
                driver,
                CellCoord::new(x, 0, z),
                Span::default(),
            ));
        }
        ir
    }

    /// [`check_legalized`] over a draw of [`scope_strategy`].
    fn check_scope(xs: &[(u32, u32, bool)]) -> Result<(), TestCaseError> {
        check_legalized(scope_ir(xs), &format!("xs={xs:?}"), &[])
    }

    /// [`check_legalized`] over a draw of [`pad_scope_strategy`].
    fn check_pad_scope(cells: &[Node], pads: &[Node]) -> Result<(), TestCaseError> {
        check_legalized(
            pad_scope_ir(cells, pads),
            &format!("cells={cells:?} pads={pads:?}"),
            &[DiagnosticCode::RouteCongestion],
        )
    }

    /// The assertions over one draw, as a function so a named test can replay it.
    fn check_legalized(
        ir: PlacementIr,
        case: &str,
        refusals: &[DiagnosticCode],
    ) -> Result<(), TestCaseError> {
        let legalized = legalize_scope(ir, case, refusals)?;
        for entry in &legalized.scopes {
            check_buffer_sites(entry, case)?;
            check_charges(entry, case)?;
        }
        for entry in &legalized.scopes {
            check_scope_total(entry, case)?;
        }
        Ok(())
    }

    /// The draw routed, delayed and legalized; a draw that routing refuses with one of `refusals`
    /// over a walled-in sink is rejected.
    fn legalize_scope(
        ir: PlacementIr,
        case: &str,
        refusals: &[DiagnosticCode],
    ) -> Result<ScopedPlacementIr, TestCaseError> {
        let placed = scoped(ScopeKind::Struct, "prop", ir);
        let routing = compile_routing(&placed);
        let refused = routing.scoped.scopes.is_empty();
        prop_assert!(
            routing.diagnostics.iter().all(|d| if refused {
                refusals.contains(&d.code)
            } else {
                d.code == DiagnosticCode::RouteCrossLayerClearance
            }),
            "routing diagnostics for {}: {:?}",
            case,
            routing.diagnostics,
        );
        if refused {
            prop_assert!(
                walls_a_sink_in(&placed.scopes[0]),
                "routing refused {} with no sink walled in: {:?}",
                case,
                routing.diagnostics,
            );
        }
        // A sink other nets' wires wall in has no route, so refusing it is the right answer.
        prop_assume!(!refused);
        let delayed = compile_delay(&routing.scoped);
        prop_assert!(
            delayed.diagnostics.is_empty(),
            "delay diagnostics for {}: {:?}",
            case,
            delayed.diagnostics,
        );
        let legalized = compile_crossing(&delayed.scoped);
        prop_assert!(
            legalized.diagnostics.is_empty(),
            "crossing diagnostics for {}: {:?}",
            case,
            legalized.diagnostics,
        );
        Ok(legalized.scoped)
    }

    /// Whether some sink of `entry` has no route of any length from its net's source.
    fn walls_a_sink_in(entry: &ScopedPlacementIrEntry) -> bool {
        let (_, trees) = laid_trees(entry);
        trees.values().any(|tree| {
            tree.unreachable()
                .iter()
                .any(|sink| tree.stranded_by(*sink) == Some(Unreached::WalledIn))
        })
    }

    fn check_buffer_sites(entry: &ScopedPlacementIrEntry, case: &str) -> Result<(), TestCaseError> {
        let (router, trees) = laid_trees(entry);
        for (index, cell) in entry.ir.cells.iter().enumerate() {
            for buffer in cell.buffer_coords() {
                let BufferSegment::Port(port) = buffer.port else {
                    panic!("a cell's buffer must name one of its driver ports");
                };
                let driver = cell
                    .drivers
                    .iter()
                    .find(|d| d.port == port)
                    .expect("every buffer names a driver of its own cell");
                let on = Buffer {
                    coord: buffer.coord,
                    net: driver.net,
                    sink: cell.coord,
                    into: CellIdentity::new(index, cell.coord, entry),
                    case,
                };
                check_buffer_site(&on, &router, &trees[&driver.net])?;
            }
        }
        for (index, output) in entry.ir.outputs.iter().enumerate() {
            for buffer in output.buffer_coords() {
                assert_eq!(
                    buffer.port,
                    BufferSegment::Out,
                    "a pad's buffer must name the outward segment",
                );
                let on = Buffer {
                    coord: buffer.coord,
                    net: output.driver,
                    sink: output.pad,
                    into: CellIdentity::output(index, output.pad, entry),
                    case,
                };
                check_buffer_site(&on, &router, &trees[&output.driver])?;
            }
        }
        Ok(())
    }

    /// Every check on one buffer, against its own net's tree: another net's block may stand beside
    /// it.
    fn check_buffer_site(
        on: &Buffer<'_>,
        router: &Router,
        tree: &NetTree,
    ) -> Result<(), TestCaseError> {
        // Not `wire_path`, which holds the terminals and would accept a cell body or pad.
        let dust: HashSet<CellCoord> = router.dust(tree).into_iter().collect();
        // Terminals included: a repeater beside a cell body or pad cuts it off too.
        let net: HashSet<CellCoord> = tree.wire_path().into_iter().collect();
        check_on_route(on, tree, &dust)?;
        let (before, after) = check_straight_through(on, tree)?;
        check_nothing_of_its_net_beside(on, &net, before, &after)
    }

    /// The router and the trees it lays for one scope, as the delay and crossing passes do.
    pub(super) fn laid_trees(entry: &ScopedPlacementIrEntry) -> (Router, HashMap<NetRef, NetTree>) {
        let region = entry.ir.region.clone().expect("fixture carries a region");
        let nets = collect_nets(&entry.ir);
        let cell_coords: Vec<CellCoord> = entry.ir.cells.iter().map(|c| c.coord).collect();
        let router = Router::new(&region, &block_sites(&entry.ir, &region));
        let trees = net_trees(&nets, &router, |net| match net {
            NetRef::Input(i) => input_pad(i as usize, PadColumn::of(&entry.ir), &region),
            NetRef::Cell(j) => cell_coords[j as usize],
        });
        (router, trees)
    }

    struct Buffer<'a> {
        coord: CellCoord,
        net: NetRef,
        /// The cell body or pad the segment the buffer is on feeds.
        sink: CellCoord,
        /// That sink, as a failure names it.
        into: CellIdentity<'a>,
        case: &'a str,
    }

    fn check_on_route(
        on: &Buffer<'_>,
        tree: &NetTree,
        dust: &HashSet<CellCoord>,
    ) -> Result<(), TestCaseError> {
        // The whole coord: footprints would accept a repeater on the plane under climbed wire.
        prop_assert!(
            dust.contains(&on.coord),
            "buffer {:?} on the wire into {} is not over dust the routing pass laid for {:?} ({})",
            on.coord,
            on.into,
            on.net,
            on.case,
        );
        // `wire_path` would accept a coord that refreshes a sibling sink instead.
        let route: HashSet<CellCoord> = tree
            .route_to(on.sink)
            .expect("every cell and pad is a terminal of the net driving it")
            .into_iter()
            .collect();
        prop_assert!(
            route.contains(&on.coord),
            "buffer {:?} is not over the route into {} for {:?} ({})",
            on.coord,
            on.into,
            on.net,
            on.case,
        );
        if on.coord.layer == RouteLayer::Plane {
            prop_assert_eq!(on.coord.y, 0);
        }
        Ok(())
    }

    /// Not through the placer's own check, so a placer that stopped asking it fails.
    fn check_straight_through(
        on: &Buffer<'_>,
        tree: &NetTree,
    ) -> Result<(Option<CellCoord>, Vec<CellCoord>), TestCaseError> {
        let c = on.coord;
        let before = tree.parent(c);
        let after: Vec<CellCoord> = tree
            .wire_path()
            .into_iter()
            .filter(|n| tree.parent(*n) == Some(c))
            .collect();
        let straight = match (before, after.as_slice()) {
            (Some(b), [a]) => {
                b.y == c.y
                    && a.y == c.y
                    && i64::from(c.x) - i64::from(b.x) == i64::from(a.x) - i64::from(c.x)
                    && i64::from(c.z) - i64::from(b.z) == i64::from(a.z) - i64::from(c.z)
            }
            _ => false,
        };
        prop_assert!(
            straight,
            "buffer {:?} on the wire into {} does not have the wire straight through it: in from \
             {:?}, out to {:?} ({})",
            c,
            on.into,
            before,
            after,
            on.case,
        );
        Ok((before, after))
    }

    /// Each layer spelled with `with_layer`, not derived by the rule production uses.
    fn faces_of(c: CellCoord) -> Vec<CellCoord> {
        let below = if c.y == 1 {
            RouteLayer::Plane
        } else {
            RouteLayer::Bridge
        };
        [
            c.x.checked_sub(1)
                .map(|x| CellCoord::with_layer(x, c.y, c.z, c.layer)),
            c.x.checked_add(1)
                .map(|x| CellCoord::with_layer(x, c.y, c.z, c.layer)),
            c.y.checked_sub(1)
                .map(|y| CellCoord::with_layer(c.x, y, c.z, below)),
            c.y.checked_add(1)
                .map(|y| CellCoord::with_layer(c.x, y, c.z, RouteLayer::Bridge)),
            c.z.checked_sub(1)
                .map(|z| CellCoord::with_layer(c.x, c.y, z, c.layer)),
            c.z.checked_add(1)
                .map(|z| CellCoord::with_layer(c.x, c.y, z, c.layer)),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// A repeater severs every face but its front and back, so this asks the grid, not the tree.
    fn check_nothing_of_its_net_beside(
        on: &Buffer<'_>,
        net: &HashSet<CellCoord>,
        before: Option<CellCoord>,
        after: &[CellCoord],
    ) -> Result<(), TestCaseError> {
        let c = on.coord;
        let beside: HashSet<CellCoord> = faces_of(c)
            .into_iter()
            .filter(|n| net.contains(n))
            .collect();
        let joined: HashSet<CellCoord> = before.into_iter().chain(after.iter().copied()).collect();
        prop_assert_eq!(
            &beside,
            &joined,
            "buffer {:?} on the wire into {} has a block of its own net beside it that it does not \
             join ({})",
            c,
            on.into,
            on.case,
        );
        Ok(())
    }

    /// What stage 3 charged one cell or pad, and the blocks stage 4 laid on the wire into it.
    struct Charge<'a> {
        /// The cell or pad, as a failure names it.
        node: CellIdentity<'a>,
        local_delay_ticks: u32,
        /// The delay of the cell's slowest driven port; zero for a pad, which has no delay
        /// of its own.
        slowest_port_ticks: u32,
        /// Distinct coords, since a cell's two ports on one net name the same repeaters.
        blocks: u32,
    }

    /// Every cell's [`Charge`], then every actuator pad's.
    fn charges(entry: &ScopedPlacementIrEntry) -> Vec<Charge<'_>> {
        let blocks = |buffers: &[BufferCoord]| {
            let coords: HashSet<CellCoord> = buffers.iter().map(|b| b.coord).collect();
            u32::try_from(coords.len()).expect("buffer block count fits in u32")
        };
        let cells = entry
            .ir
            .cells
            .iter()
            .enumerate()
            .map(|(index, cell)| Charge {
                node: CellIdentity::new(index, cell.coord, entry),
                local_delay_ticks: cell
                    .local_delay_ticks()
                    .expect("legalized cells carry Some(local_delay_ticks)"),
                slowest_port_ticks: slowest_port_ticks(cell),
                blocks: blocks(cell.buffer_coords()),
            });
        let pads = entry
            .ir
            .outputs
            .iter()
            .enumerate()
            .map(|(index, output)| Charge {
                node: CellIdentity::output(index, output.pad, entry),
                local_delay_ticks: output
                    .local_delay_ticks()
                    .expect("legalized pads carry Some(local_delay_ticks)"),
                slowest_port_ticks: 0,
                blocks: blocks(output.buffer_coords()),
            });
        cells.chain(pads).collect()
    }

    /// Per cell and pad, since an over-charged one and an under-charged one cancel in a sum.
    fn check_charges(entry: &ScopedPlacementIrEntry, case: &str) -> Result<(), TestCaseError> {
        for charge in charges(entry) {
            // A cell below its slowest port would clamp to zero and match a cell with no buffers.
            let delta = charge
                .local_delay_ticks
                .checked_sub(charge.slowest_port_ticks);
            prop_assert!(
                delta.is_some(),
                "local_delay_ticks {} < slowest_port_ticks {} for {} ({})",
                charge.local_delay_ticks,
                charge.slowest_port_ticks,
                charge.node,
                case,
            );
            let delta = delta.unwrap_or_default();
            prop_assert_eq!(
                delta,
                charge
                    .blocks
                    .checked_mul(BUFFER_REPEATER_TICKS)
                    .expect("block count × BUFFER_REPEATER_TICKS fits in u32"),
                "{} charged {} ticks of buffer but its wire carries {} block(s) ({})",
                charge.node,
                delta,
                charge.blocks,
                case,
            );
        }
        Ok(())
    }

    /// `Σ blocks × BUFFER_REPEATER_TICKS = Σ (local − slowest port)` over a scope's cells and pads.
    fn check_scope_total(entry: &ScopedPlacementIrEntry, case: &str) -> Result<(), TestCaseError> {
        let charges = charges(entry);
        let buffer_total: u32 = charges.iter().map(|charge| charge.blocks).sum();
        let mut delta_total: u32 = 0;
        for charge in &charges {
            let delta = charge
                .local_delay_ticks
                .checked_sub(charge.slowest_port_ticks);
            prop_assert!(
                delta.is_some(),
                "local_delay_ticks {} < slowest_port_ticks {} for {} — the delay pass charged less than the slowest port",
                charge.local_delay_ticks,
                charge.slowest_port_ticks,
                charge.node,
            );
            delta_total = delta_total
                .checked_add(delta.unwrap())
                .expect("delta_total sum overflowed u32");
        }
        let lhs = buffer_total
            .checked_mul(BUFFER_REPEATER_TICKS)
            .expect("buffer_total × BUFFER_REPEATER_TICKS overflowed u32");
        prop_assert_eq!(
            lhs,
            delta_total,
            "buffer block count × BUFFER_REPEATER_TICKS ({}) must equal Σ(local_delay_ticks − slowest_port_ticks) ({}) for scope `{}` with {}",
            lhs,
            delta_total,
            entry.name,
            case,
        );
        Ok(())
    }

    /// A rejected case never reached the assertions, so it panics as a shape gone stale.
    fn replay(result: Result<(), TestCaseError>, case: &str) {
        match result {
            Ok(()) => {}
            Err(TestCaseError::Reject(why)) => {
                panic!("{case}: this shape no longer exercises the bug ({why})")
            }
            Err(TestCaseError::Fail(why)) => panic!("{case}: {why}"),
        }
    }

    /// The branch to the far sink steps back to `x = 61`, so `(61, 0, 1)` is dust beside the
    /// straight coord `(61, 0, 0)` that the tree does not join to it.
    #[test]
    fn a_buffer_walked_back_over_a_turn_stops_short_of_a_branch_beside_it() {
        let xs = [(62, 4, false), (62, 2, false)];
        let case = format!("xs={xs:?}");
        replay(check_scope(&xs), &case);
        let legalized =
            legalize_scope(scope_ir(&xs), &case, &[]).expect("the replay above legalized it");
        let entry = &legalized.scopes[0];
        let (router, trees) = laid_trees(entry);
        let plane = |x, z| CellCoord::with_layer(x, 0, z, RouteLayer::Plane);
        assert_a_branch_beside(
            &router,
            &trees[&NetRef::Input(0)],
            plane(61, 0),
            plane(61, 1),
            &case,
        );
        assert_eq!(
            sites_of(entry, plane(62, 4)),
            vec![plane(16, 0), plane(32, 0), plane(48, 0), plane(60, 0)],
            "{case}: the fourth repeater must walk back off (61, 0, 1), over the fork and \
             the turn and past (61, 0, 0), onto (60, 0, 0)",
        );
    }

    /// The `x = 62` shape moved to `x = 97`, where the repeater falls due beside the branch itself.
    #[test]
    fn a_buffer_due_beside_a_branch_steps_back_off_it() {
        let xs = [(97, 2, false), (97, 3, false)];
        let case = format!("xs={xs:?}");
        replay(check_scope(&xs), &case);
        let legalized =
            legalize_scope(scope_ir(&xs), &case, &[]).expect("the replay above legalized it");
        let entry = &legalized.scopes[0];
        let (router, trees) = laid_trees(entry);
        let plane = |x, z| CellCoord::with_layer(x, 0, z, RouteLayer::Plane);
        assert_a_branch_beside(
            &router,
            &trees[&NetRef::Input(0)],
            plane(96, 0),
            plane(96, 1),
            &case,
        );
        assert_eq!(
            sites_of(entry, plane(97, 3)),
            vec![
                plane(16, 0),
                plane(32, 0),
                plane(48, 0),
                plane(64, 0),
                plane(80, 0),
                plane(95, 0),
            ],
            "{case}: the sixth repeater must step back off (96, 0, 0), beside the branch, \
             onto (95, 0, 0)",
        );
    }

    /// The `x = 62` shape on a trunk fed through the fork `(1, 0, 4)`, off the pad's row.
    #[test]
    fn a_buffer_on_a_trunk_fed_through_a_fork_stops_short_of_a_branch_beside_it() {
        let xs = [(27, 6, false), (27, 7, false), (1, 5, false)];
        let case = format!("xs={xs:?}");
        replay(check_scope(&xs), &case);
        let legalized =
            legalize_scope(scope_ir(&xs), &case, &[]).expect("the replay above legalized it");
        let entry = &legalized.scopes[0];
        let (router, trees) = laid_trees(entry);
        let plane = |x, z| CellCoord::with_layer(x, 0, z, RouteLayer::Plane);
        assert_a_branch_beside(
            &router,
            &trees[&NetRef::Input(0)],
            plane(26, 4),
            plane(26, 5),
            &case,
        );
        assert_eq!(
            sites_of(entry, plane(27, 7)),
            vec![plane(12, 4), plane(25, 4)],
            "{case}: the second repeater must walk back off the fork (27, 0, 5), over the \
             turn and past (26, 0, 4), onto (25, 0, 4)",
        );
    }

    /// The coords the buffers of the cell on `sink` stand on, each once, in first-named order.
    fn sites_of(entry: &ScopedPlacementIrEntry, sink: CellCoord) -> Vec<CellCoord> {
        let cell = entry
            .ir
            .cells
            .iter()
            .find(|c| c.coord == sink)
            .expect("the sink is a cell of the scope");
        let mut sites: Vec<CellCoord> = Vec::new();
        for buffer in cell.buffer_coords() {
            if !sites.contains(&buffer.coord) {
                sites.push(buffer.coord);
            }
        }
        sites
    }

    /// `c` spelled `(x, y, z)` for a failure message.
    fn spelled(c: CellCoord) -> String {
        format!("({}, {}, {})", c.x, c.y, c.z)
    }

    /// Asserts `branch` and `beside` are both dust of the net that the tree does not join, so a
    /// repeater on `beside` would cut `branch` off.
    fn assert_a_branch_beside(
        router: &Router,
        tree: &NetTree,
        beside: CellCoord,
        branch: CellCoord,
        case: &str,
    ) {
        let gone = format!("{case}: this shape no longer exercises the bug");
        assert!(
            faces_of(beside).contains(&branch),
            "{gone}: {} is not on a face of {}",
            spelled(branch),
            spelled(beside),
        );
        let dust: HashSet<CellCoord> = router.dust(tree).into_iter().collect();
        for coord in [beside, branch] {
            assert!(
                dust.contains(&coord),
                "{gone}: {} is not dust of the net",
                spelled(coord),
            );
        }
        assert!(
            tree.parent(branch) != Some(beside) && tree.parent(beside) != Some(branch),
            "{gone}: the tree joins {} and {}",
            spelled(beside),
            spelled(branch),
        );
    }

    /// The route to the far sink climbs over the cell at `(47, 0, 0)`, so the only block of its net
    /// beside `(47, 1, 0)`, where a repeater falls due, is that sink below it.
    #[test]
    fn a_buffer_never_stands_over_a_sink_of_its_own_net() {
        let xs = [(74, 0, false), (47, 0, false), (47, 1, false)];
        let case = format!("xs={xs:?}");
        replay(check_scope(&xs), &case);
        let legalized =
            legalize_scope(scope_ir(&xs), &case, &[]).expect("the replay above legalized it");
        let entry = &legalized.scopes[0];
        let (router, trees) = laid_trees(entry);
        let tree = &trees[&NetRef::Input(0)];
        let plane = |x, z| CellCoord::with_layer(x, 0, z, RouteLayer::Plane);
        let bridge = |x, z| CellCoord::with_layer(x, 1, z, RouteLayer::Bridge);
        assert_only_a_terminal_beside(
            &router,
            tree,
            plane(74, 0),
            [bridge(46, 0), bridge(47, 0), bridge(48, 0)],
            plane(47, 0),
            &case,
        );
        assert_eq!(
            sites_of(entry, plane(74, 0)),
            vec![plane(16, 0), plane(32, 0), plane(45, 0), bridge(60, 0)],
            "{case}: the third repeater must step back off (47, 1, 0), over the sink at \
             (47, 0, 0), onto (45, 0, 0)",
        );
    }

    /// The near cell's body at `(31, 0, 0)` is the only block of the net beside `(31, 0, 1)`.
    #[test]
    fn a_buffer_never_stands_beside_a_sink_of_its_own_net() {
        let xs = [(31, 0, false), (32, 0, false)];
        let case = format!("xs={xs:?}");
        replay(check_scope(&xs), &case);
        let legalized =
            legalize_scope(scope_ir(&xs), &case, &[]).expect("the replay above legalized it");
        let entry = &legalized.scopes[0];
        let (router, trees) = laid_trees(entry);
        let tree = &trees[&NetRef::Input(0)];
        let plane = |x, z| CellCoord::with_layer(x, 0, z, RouteLayer::Plane);
        assert_only_a_terminal_beside(
            &router,
            tree,
            plane(32, 0),
            [plane(30, 1), plane(31, 1), plane(32, 1)],
            plane(31, 0),
            &case,
        );
        assert_eq!(
            sites_of(entry, plane(32, 0)),
            vec![plane(16, 0), plane(29, 0)],
            "{case}: the second repeater must step back off (31, 0, 1), beside the sink at \
             (31, 0, 0), onto (29, 0, 0)",
        );
    }

    /// Asserts `terminal` is the only block of the net beside `due` but `before` and `after`, and
    /// not dust, which would keep the repeater off `due` for another reason.
    fn assert_only_a_terminal_beside(
        router: &Router,
        tree: &NetTree,
        sink: CellCoord,
        [before, due, after]: [CellCoord; 3],
        terminal: CellCoord,
        case: &str,
    ) {
        let gone = format!("{case}: this shape no longer exercises the bug");
        let route = tree
            .route_to(sink)
            .expect("every cell and pad is a terminal of the net driving it");
        assert!(
            route.contains(&due),
            "{gone}: the route to {} no longer passes {}: {route:?}",
            spelled(sink),
            spelled(due),
        );
        assert_eq!(
            tree.parent(due),
            Some(before),
            "{gone}: {} is no longer reached from {}",
            spelled(due),
            spelled(before),
        );
        assert_eq!(
            tree.parent(after),
            Some(due),
            "{gone}: {} no longer runs on to {}",
            spelled(due),
            spelled(after),
        );
        let net: HashSet<CellCoord> = tree.wire_path().into_iter().collect();
        let beside: HashSet<CellCoord> = faces_of(due)
            .into_iter()
            .filter(|n| net.contains(n) && *n != before && *n != after)
            .collect();
        assert_eq!(
            beside,
            HashSet::from([terminal]),
            "{gone}: the blocks of the net on the faces of {} other than {} and {} are no \
             longer {} alone",
            spelled(due),
            spelled(before),
            spelled(after),
            spelled(terminal),
        );
        assert!(
            !router.dust(tree).contains(&terminal),
            "{gone}: {} is dust of the net, not one of its terminals",
            spelled(terminal),
        );
    }

    /// [`a_buffer_never_stands_beside_a_sink_of_its_own_net`] with actuator pads for the cells.
    #[test]
    fn a_buffer_never_stands_beside_a_pad_of_its_own_net() {
        let cells: [Node; 0] = [];
        let pads = [(31, 0, NetRef::Input(0)), (32, 0, NetRef::Input(0))];
        let case = format!("cells={cells:?} pads={pads:?}");
        replay(check_pad_scope(&cells, &pads), &case);
        let legalized = legalize_scope(pad_scope_ir(&cells, &pads), &case, &[])
            .expect("the replay above legalized it");
        let entry = &legalized.scopes[0];
        let (router, trees) = laid_trees(entry);
        let plane = |x, z| CellCoord::with_layer(x, 0, z, RouteLayer::Plane);
        assert_only_a_terminal_beside(
            &router,
            &trees[&NetRef::Input(0)],
            plane(32, 0),
            [plane(30, 1), plane(31, 1), plane(32, 1)],
            plane(31, 0),
            &case,
        );
        assert_eq!(
            entry.ir.outputs[1]
                .buffer_coords()
                .iter()
                .map(|b| b.coord)
                .collect::<Vec<_>>(),
            vec![plane(16, 0), plane(29, 0)],
            "{case}: the second repeater must step back off (31, 0, 1), beside the pad at \
             (31, 0, 0), onto (29, 0, 0)",
        );
    }

    /// `sig.a` runs straight to its pad past the cell `sig.b` feeds at `(16, 0, 1)`, so the first
    /// repeater on the pad's wire stands beside that cell's body.
    #[test]
    fn a_buffer_stands_beside_a_cell_body_of_another_net() {
        let cells = [(16, 1, NetRef::Input(1))];
        let pads = [(40, 0, NetRef::Input(0))];
        let case = format!("cells={cells:?} pads={pads:?}");
        replay(check_pad_scope(&cells, &pads), &case);
        let legalized = legalize_scope(pad_scope_ir(&cells, &pads), &case, &[])
            .expect("the replay above legalized it");
        let entry = &legalized.scopes[0];
        let (router, trees) = laid_trees(entry);
        let plane = |x, z| CellCoord::with_layer(x, 0, z, RouteLayer::Plane);
        assert_a_block_of_another_net_beside(
            &router,
            &trees,
            NetRef::Input(0),
            plane(40, 0),
            plane(16, 0),
            plane(16, 1),
            &case,
        );
        assert_eq!(
            entry.ir.outputs[0]
                .buffer_coords()
                .iter()
                .map(|b| b.coord)
                .collect::<Vec<_>>(),
            vec![plane(16, 0), plane(32, 0)],
            "{case}: the first repeater must stand on (16, 0, 0), beside the cell `sig.b` feeds",
        );
    }

    /// Asserts `due` is dust on `net`'s route into `pad`, and `block` is on a face of it and on
    /// another net's tree, not on `net`'s, so a check that took every net's blocks for `net`'s
    /// would refuse a repeater on `due`.
    fn assert_a_block_of_another_net_beside(
        router: &Router,
        trees: &HashMap<NetRef, NetTree>,
        net: NetRef,
        pad: CellCoord,
        due: CellCoord,
        block: CellCoord,
        case: &str,
    ) {
        let gone = format!("{case}: this shape no longer puts another net beside a repeater");
        let tree = &trees[&net];
        assert!(
            router.dust(tree).contains(&due),
            "{gone}: {} is not dust of {net:?}",
            spelled(due),
        );
        let route = tree
            .route_to(pad)
            .expect("every cell and pad is a terminal of the net driving it");
        assert!(
            route.contains(&due),
            "{gone}: the route to {} no longer passes {}: {route:?}",
            spelled(pad),
            spelled(due),
        );
        assert!(
            faces_of(due).contains(&block),
            "{gone}: {} is not on a face of {}",
            spelled(block),
            spelled(due),
        );
        let holds = |net: &NetRef| trees[net].wire_path().contains(&block);
        assert!(
            !holds(&net),
            "{gone}: {} is a block of {net:?}",
            spelled(block),
        );
        assert!(
            trees.keys().any(|other| *other != net && holds(other)),
            "{gone}: {} is a block of no other net",
            spelled(block),
        );
    }

    /// `sig.a` forks at `(28, 0, 0)`, so the pad's wire shares the repeater on `(16, 0, 0)` with
    /// the cell's and runs on past the fork through two the cell's wire does not.
    #[test]
    fn a_pad_counts_the_repeaters_past_the_fork_its_wire_shares_with_a_cell() {
        let cells = [(28, 7, NetRef::Input(0))];
        let pads = [(60, 0, NetRef::Input(0))];
        let case = format!("cells={cells:?} pads={pads:?}");
        replay(check_pad_scope(&cells, &pads), &case);
        let legalized = legalize_scope(pad_scope_ir(&cells, &pads), &case, &[])
            .expect("the replay above legalized it");
        let entry = &legalized.scopes[0];
        let (router, trees) = laid_trees(entry);
        let plane = |x, z| CellCoord::with_layer(x, 0, z, RouteLayer::Plane);
        assert_a_fork_before(
            &router,
            &trees[&NetRef::Input(0)],
            [plane(28, 7), plane(60, 0)],
            plane(28, 0),
            plane(32, 0),
            &case,
        );
        let coords = |buffers: &[BufferCoord]| buffers.iter().map(|b| b.coord).collect::<Vec<_>>();
        assert_eq!(
            coords(entry.ir.cells[0].buffer_coords()),
            vec![plane(16, 0), plane(28, 4)],
            "{case}: the cell's wire must keep only the shared repeater and its own past the fork",
        );
        assert_eq!(
            coords(entry.ir.outputs[0].buffer_coords()),
            vec![plane(16, 0), plane(32, 0), plane(48, 0)],
            "{case}: the pad's wire must name the shared repeater, then its own two past the fork",
        );
    }

    /// Asserts the routes into `cell` and `pad` part at `fork`, a block of dust, and `due` is on
    /// the pad's past it, so a repeater there is the pad's alone.
    fn assert_a_fork_before(
        router: &Router,
        tree: &NetTree,
        [cell, pad]: [CellCoord; 2],
        fork: CellCoord,
        due: CellCoord,
        case: &str,
    ) {
        let gone = format!("{case}: this shape no longer puts a repeater past a fork");
        let route = |sink: CellCoord| {
            tree.route_to(sink)
                .expect("every cell and pad is a terminal of the net driving it")
        };
        let (to_cell, to_pad) = (route(cell), route(pad));
        let shared = to_cell
            .iter()
            .zip(&to_pad)
            .take_while(|(a, b)| a == b)
            .last()
            .map(|(a, _)| *a);
        assert_eq!(
            shared,
            Some(fork),
            "{gone}: the routes to {} and {} no longer part at {}",
            spelled(cell),
            spelled(pad),
            spelled(fork),
        );
        assert!(
            router.dust(tree).contains(&fork),
            "{gone}: {} is not dust of the net",
            spelled(fork),
        );
        assert!(
            to_pad.contains(&due) && !to_cell.contains(&due),
            "{gone}: {} is not on the route to {} alone",
            spelled(due),
            spelled(pad),
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

        /// Every cell here is driven by one net, so a sum and a max over the driving nets agree.
        #[test]
        fn a_cell_waits_its_slowest_port_plus_its_buffer_ticks(xs in scope_strategy()) {
            check_scope(&xs)?;
        }

        /// Up to four nets, so a buffer can stand beside a block of a net other than its own.
        #[test]
        fn a_pad_waits_the_buffer_ticks_of_the_repeaters_on_its_wire(
            (cells, pads) in pad_scope_strategy(),
        ) {
            check_pad_scope(&cells, &pads)?;
        }
    }
}
