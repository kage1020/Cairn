//! Crossing-legalization behaviours `tests/redstone/crossing.rs` cannot reach
//! through synth fixtures: shapes only a hand-built `PlacementIr`
//! produces, such as a segment long enough to need a buffer.

use cairn_lang_core::Edition;
use cairn_lang_core::error::Span;

use std::collections::{HashMap, HashSet};

use super::compile_crossing;
use crate::delay::{BUFFER_REPEATER_TICKS, compile_delay};
use crate::diagnostic::DiagnosticCode;
use crate::edition_netlist_ir::EditionCell;
use crate::grid::{PadColumn, input_pad};
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
use crate::placement_ir::{BufferSegment, PlacedOutputNode};
use crate::placement_ir::{
    CellCoord, PlacedCellNode, PlacementIr, PlacementPhase, RouteLayer, ScopedPlacementIr,
};
use crate::routing::compile_routing;
use crate::routing_geometry::{Router, block_sites, collect_nets, net_trees};
use crate::test_fixtures::{DELAYED, placed_cell, reservation, scoped, staircase};

#[test]
fn no_scopes_input_yields_no_scopes_output() {
    let legalized = compile_crossing(&ScopedPlacementIr::new());
    assert!(legalized.diagnostics.is_empty());
    assert!(legalized.scoped.scopes.is_empty());
}

#[test]
fn empty_scope_passes_through_untouched() {
    // Dropping an empty scope is `ScopedPlacementIr::push`'s job, not this pass's.
    let ir = PlacementIr::new(Edition::Java);
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "empty", ir));
    assert!(legalized.diagnostics.is_empty());
    assert_eq!(legalized.scoped.scopes.len(), 1);
    assert!(legalized.scoped.scopes[0].ir.cells.is_empty());
}

#[test]
fn single_net_no_buffer_is_untouched() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(5, 3, 2));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    // `(1, 0, 1)` is where placement stamps the first cell. At the origin it would stand on input
    // pad #0, a zero-length net that needs no buffer whatever the rule.
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
    // 17 steps carry 16 blocks of dust, one past the attenuation limit, so one buffer stands in
    // place of the 16th block.
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
    // The widest `Serialize` path, `Legalized` with a non-empty `buffer_coords`, which no
    // byte-identity test reaches.
    let json = serde_json::to_string(&legalized.scoped)
        .expect("legalized scoped IR must serialise cleanly");
    assert!(
        json.contains("\"buffer_coords\":[{\"port\":\"a\",\"coord\":{\"x\":16,\"y\":0,\"z\":0}}]"),
        "expected buffer_coords entry to appear in JSON verbatim, got {json}",
    );
    // No `.crn` example has a segment past the attenuation limit, so only this fixture shows the
    // stage tag beside a populated `buffer_coords`.
    assert!(
        json.contains("\"stage\":\"crossing\""),
        "expected the crossing stage tag alongside buffer_coords, got {json}",
    );
}

/// A sink fifteen blocks from its driver with a block halfway.
///
/// The pad is at `(0,0,0)` and the sink at `(15,0,0)`: 15 steps in a straight line, 14 blocks of
/// dust, no buffer. The block at `(7,0,0)` sends the wire round it, and the two steps that costs
/// make 17 steps, 16 blocks of dust and one repeater. The blocker drives nothing, so no wire of
/// its own runs through the coords under test.
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

/// The index of the sink in [`walled_scope`]'s cell list.
const WALLED_SINK: usize = 1;

/// The straight line from the pad would ask for no buffer, leaving 16 blocks of dust unrefreshed.
/// The coord is pinned so a change to the axis order or tie-break shows here. It is 15 steps along
/// rather than 16 because the route comes back along `z=1` and turns into the sink at `(15,0,1)`,
/// and a repeater on the turn would face away from it.
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

/// [`walled_scope`] with a second net, `sig.b`, along the row the `sig.a` route comes back down,
/// so a buffer on a foreign net's dust has something to stand on.
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

/// Every buffer stands on dust stage 2 laid (a coord of its own net's [`NetTree::wire_path`], or a
/// layer above one in the same column, since the routed wire itself climbs) and never on a coord
/// another net's wire runs through.
///
/// Checked against `wire_path` rather than the `route_to` the allocator reads, since comparing
/// that path to itself would assert nothing.
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

/// Stage 3 charges ticks for the repeaters a cell's signals pass through and stage 4 lays them, so
/// the two counts must agree.
///
/// Distinct coords rather than `buffer_coords().len()`, because two segments of one net share the
/// repeater on their prefix; every cell here has one driver, and
/// `ports_sharing_a_net_share_the_repeater_the_charge_and_the_dust` is where the two differ. The
/// proptest seeds unobstructed layouts, where the straight line and the route mostly coincide;
/// here Manhattan says 15 steps and no buffer, and the route says 17 and one.
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
            .saturating_sub(cell.cell.base_delay_ticks());
        let blocks: HashSet<CellCoord> = cell.buffer_coords().iter().map(|b| b.coord).collect();
        let placed = u32::try_from(blocks.len()).expect("small");
        assert_eq!(
            charged,
            placed.saturating_mul(BUFFER_REPEATER_TICKS),
            "cell #{index} was charged for {charged} ticks of buffer but got {placed} block(s)",
        );
    }
}

/// Two nets whose straight lines share one row, `z=1`: `sig.a` runs left to right from its pad and
/// cell #0 drives right to left. Both sinks sit one row off it, so neither cell body stands on the
/// other net's line and sends the router round for an unrelated reason.
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

/// Two nets that want one row are laid apart, the row going to one of them. Stage 2 answers this,
/// so the assertion is about the wires and there is no finding.
///
/// Routing each net against the blocks alone puts both on one coord, which the test asserts
/// first, so the disjointness below is the rule at work rather than the geometry. The region is
/// roomy, so the second net goes round; `a_net_with_no_way_round_climbs_over_the_dust_in_its_way`
/// in `routing_geometry` pins the climb.
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

    // The adjacency rule written out rather than through `beside`, so a mutant that empties
    // `beside` cannot pass here too.
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

/// A fan-out's shared prefix carries one repeater, and every sink past it is fed by that one.
///
/// The first cell ends the straight run at `(16,0,0)` and the route to every later one leaves the
/// row at `(15,0,0)` to go round it, so that coord is a fork, where a repeater would starve one
/// branch. The one on the trunk at `(14,0,0)` refreshes the signal for all of them. Reuse needs no
/// layer above the plane, which is why `void=1` is among the cases. The coord is compared whole,
/// `layer` included, so a buffer that escaped upward fails.
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

/// A buffer candidate is never a component: a candidate sits strictly between the ends of a
/// route, and the router keeps those coords off the blocks. With each net owning its dust alone,
/// this pass has no coord to contest.
///
/// Here `sig.a`'s straight-line 15-step point is `sig.b`'s cell body, with `void=1` leaving no
/// layer above it.
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
        "the coord it used to want is the cell body",
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

/// A cell fed by two *distinct* nets is charged for every buffer standing on both.
///
/// The proptest drives every cell from one net, where a sum and a max over the driving nets agree.
/// Here `sig.a` reaches the comparator over 42 blocks of dust and two buffers and the upstream
/// cell over 20 blocks and one, so the figure is `base + 3` where a max would write `base + 2`.
/// Threaded through all three passes so the ticks and the blocks come from different passes.
/// `cells[0]` drives the comparator without being driven, which keeps its segment short.
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
    // Per port, not just the total, so the sum and the max differ by an amount attributable to
    // one net.
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

    let base = EditionCell::JavaComparatorAnd.base_delay_ticks();
    let ticks = cell
        .local_delay_ticks()
        .expect("legalized cells carry Some(local_delay_ticks)");
    let blocks = u32::try_from(distinct.len()).expect("three blocks fit in u32");
    assert_eq!(
        ticks - base,
        blocks * BUFFER_REPEATER_TICKS,
        "every block stage 4 laid is a tick stage 3 charged",
    );
    // And not the figure a max over the driving nets would write.
    let widest = u32::try_from(per_port(PortName::A).max(per_port(PortName::B)))
        .expect("two buffers fit in u32");
    assert_ne!(
        ticks - base,
        widest * BUFFER_REPEATER_TICKS,
        "a max over the driving nets would drop the shorter segment's block",
    );
}

/// Two ports of one cell on one net are fed by one strand of dust: two attributions, one block,
/// one charge.
///
/// Threaded through routing, delay and crossing because `wire_length`, `local_delay_ticks` and
/// `buffer_coords` are written by three passes, each of which must count the segment once.
/// `JavaComparatorAnd` so the base delay is a pinned 1; `sig.s0 = sig.a and sig.a` is how a `.crn`
/// reaches this shape (`ports_sharing_a_net_are_measured_once` in `tests/redstone/routing.rs`).
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
    // The pad sits at (0,0,0) and the cell at (17,0,0): 17 steps,
    // 16 blocks of dust, laid once.
    assert_eq!(
        cell.wire_length(),
        Some(17),
        "one strand of dust, measured once",
    );
    assert_eq!(
        cell.local_delay_ticks(),
        Some(1 + BUFFER_REPEATER_TICKS),
        "base 1 plus the one repeater the signal passes through",
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

/// The wire out to an actuator records a repeater a *cell* segment placed.
///
/// Cells are placed before actuator pads, so this is the only direction the two can meet. The
/// cell is a block on the row, so the pad's wire steps round it and needs a second repeater past
/// the shared one. The route forks at `(16,0,0)`, into the cell and round it, so the shared
/// repeater stands one coord before the fork; the second stands at `(29,0,1)`, the last straight
/// coord before the turn at `(30,0,1)` into the pad at `(30,0,0)`.
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

/// Every port spelling reaches the push site and the wire form. Every other buffer test drives a
/// single `A`; here the three drivers are on three different nets, so each port carries its own
/// coord out.
///
/// The four nets contend for the rows. Their pads stand at `z=0`, `2`, `3` and `4`, stepping over
/// the cell row. `sig.sel` is laid first (same fanout, lowest [`crate::grid::net_ref_key`]) along
/// `z=0`; `sig.blocker` takes `z=2`; `sig.port_a`, whose pad stands beside that row, has no row
/// left and climbs at its own pad; `sig.port_b` runs along `z=4`. The [`RouteLayer::Bridge`]
/// repeater is the same rule holding through the climb. `sig.sel` turns off `z=0` at `(14,0,0)`
/// and back east at `(14,0,1)`, so its repeater, due 16 steps along, stands on `(15,0,1)`.
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
    // The expected text pins the cell identity too, which tells an operator which cell tripped
    // the guard.
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
    // Prior stages never produce this, and the refusal is only for a scope with cells or outputs.
    let ir = PlacementIr::new(Edition::Java);
    let legalized = compile_crossing(&scoped(ScopeKind::Struct, "harmless", ir));
    assert!(legalized.diagnostics.is_empty());
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

/// Stage 4 refuses a stretch no repeater can stand on as stage 3 does, word for word but for the
/// netlist named. Stage 3 refuses the staircase, so the routed one is promoted by hand: what
/// `--stage crossing` would get without stage 3.
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

/// A route that turns at the 16-step point takes a second repeater its length alone does not ask
/// for, and stage 3 charges for it.
///
/// 32 steps from `(0,0,0)` to `(16,0,16)`, east then south. The first repeater falls due on the
/// corner, so it stands one short of it; the 16 blocks of dust it leaves ahead are one too many,
/// and the second stands on `(16,0,15)`. `buffer_count_for_segment(32)` is 1, so counting from
/// the length would charge one tick for two blocks.
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
        Some(EditionCell::JavaRepeaterOr.base_delay_ticks() + 2 * BUFFER_REPEATER_TICKS),
        "stage 3 charges for both blocks stage 4 lays",
    );
}

#[test]
fn buffer_coord_index_at_kth_boundary() {
    // 49 steps trip the attenuation limit three times; the delay tests' tick-side table has the
    // same row. The route walks x 48 steps then z 1 step. The first two repeaters stand at
    // `path[16]` and `path[32]`; `path[48]` is the turn, where a repeater would drive off the
    // wire, so the third stands one short of it.
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
    // The far end of the range `MAX_ATTENUATION_SEGMENT` permits: 256 steps take 15 buffers, as
    // in the tick-side table, all at `k * 16` on the straight run.
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
    // Re-running the whole pass trips on `cells[0]`, which cannot tell a hard-coded index from the
    // real one; a `Delayed` first cell and a `Legalized` second one put the panic on `cells[1]`.
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
    //! The attenuation limit holds along whole strands, across the cells that pass strength on,
    //! checked by a walk written out here rather than through the placer's budgets. "Strand" here
    //! is the dust counted from the last block that restored strength, through such cells, not
    //! the contiguous dust the rest of this module keeps two signals off.

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

    /// `source` through every stage to the legalized IR of its one
    /// scope, each stage clean but for the cross-layer advisory.
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

    /// Walks every net in topological order, carrying the dust spent since the last block that
    /// restores strength across the cells that pass it on, and asserts that no block of dust is
    /// past the limit and no sink or repeater is reached over more than the limit's worth.
    /// Returns how many buffer repeaters it met.
    ///
    /// It reads the repeaters out of the IR and never calls the placer. It does not catch:
    ///
    /// - an off-by-one in which coords are dust, since it tells dust from a sink or a repeater by
    ///   the placer's own rule;
    /// - a repeater too many, since it is one-sided.
    ///   [`a_strand_through_a_dust_merge_steps_back_off_a_turn_once`] and
    ///   [`a_strand_through_a_comparator_steps_back_off_a_fork_once`] pin that side exactly.
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
                // `here` counts `coord` as a block of dust. Dust
                // there is at strength `16 - here`; a sink or a
                // repeater only reads the dust before it, at
                // `17 - here`.
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

    /// Four plates combined into one door through three two-input
    /// cells in a row: dust merges for `or` on Bedrock, comparators
    /// for `and` on Java. Either way the dust from a plate to the
    /// door is one strand. No single segment is past the limit —
    /// the door's own is 15 — and the strand is.
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

    /// Sixteen Java comparators in a chain, each also reading one
    /// shared sensor: `sig.c{i} = sig.c{i-1} and sig.b`. The dust
    /// from `sig.b` runs on through every comparator after the one it
    /// taps into, as far as the door.
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

    /// A scope built by hand rather than from source, so the shape of
    /// every wire is known: the sensor pad, at `(0, 0, 0)`, drives one
    /// `kind` cell on `cell`, and that cell drives an actuator pad on
    /// each of `pads`. Through routing, delay and crossing, each
    /// clean.
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

    /// Every cell's buffer coords, then every actuator pad's, in IR
    /// order.
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

    /// The upper bound the strand walk does not pin, on a turn. A Bedrock OR, a dust merge, stands
    /// 6 steps from the sensor and drives a pad at `(16, 0, 2)`, reached east to `(16, 0, 0)` then
    /// south. The strand runs on through the merge as its 6th block of dust, so its 16th is the
    /// corner, and the repeater steps back to `(15, 0, 0)`, the only one in the scope.
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

    /// The upper bound on a fork. A Java comparator AND stands 3 steps
    /// from the sensor and drives two pads: `(16, 0, 0)`, straight down
    /// the row, and `(16, 0, 1)`, which its wire reaches by leaving the
    /// row at `(15, 0, 0)`. The strand runs on through the comparator
    /// with the 3 blocks it spent to get there, so the near pad reads
    /// the 15 blocks before it and the branch's `(15, 0, 1)`, which
    /// turns, is its 16th block of dust. The repeater steps back over
    /// the fork onto the trunk, to `(14, 0, 0)`, and that one block
    /// serves both pads.
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

mod phase4_invariant {
    //! Property tests for the crossing and delay agreement, here rather than in
    //! `tests/redstone/crossing.rs` because the strategy hand-builds `Unrouted`
    //! [`PlacedCellNode`]s through the `pub(crate)` `phase` field.
    use proptest::prelude::*;

    use super::{
        BufferSegment, CellCoord, CellPortDriver, Edition, EditionCell, HashMap, HashSet, NetRef,
        PadColumn, PlacedCellNode, PlacementIr, PlacementPhase, PortName, RouteLayer, Router,
        ScopeKind, ScopedPlacementIr, Span, block_sites, collect_nets, compile_crossing, input_pad,
        net_trees, reservation, scoped,
    };
    use crate::delay::{BUFFER_REPEATER_TICKS, compile_delay};
    use crate::placement_ir::ScopedPlacementIrEntry;
    use crate::routing::compile_routing;
    use crate::routing_geometry::NetTree;

    /// Sink positions for the stage-4 invariant: each `(x, z)` seeds a cell at `(x, 0, z)` driven
    /// from `Input(0)` at `(0, 0, 0)`. `x` in `1..=99` spans sub-limit segments and several
    /// boundaries, so most cases carry buffers.
    ///
    /// `z` varies because collinear sinks are the one shape where a route and the straight line
    /// coincide. The `bool` gives a cell a second port on the *same* net, so the block count and
    /// the attribution count can differ.
    ///
    /// Duplicate `(x, z)` pairs are dropped, keeping the first: two cells on one coord is not a
    /// scope placement emits, and `collapsed_block` asserts on one. The dedup is a `prop_map` over
    /// the same tuple so every seed in `proptest-regressions/crossing.txt` still replays the case
    /// it was recorded for; a seed is an RNG state and replays as an unrelated value under a
    /// different shape.
    fn phase4_scope_strategy() -> impl Strategy<Value = Vec<(u32, u32, bool)>> {
        prop::collection::vec((1u32..=99u32, 0u32..8u32, prop::bool::ANY), 1..=3).prop_map(
            |mut cells| {
                let mut seen: HashSet<(u32, u32)> = HashSet::new();
                cells.retain(|(x, z, _)| seen.insert((*x, *z)));
                cells
            },
        )
    }

    /// The scope one draw of [`phase4_scope_strategy`] stands for:
    /// one cell at `(x, 0, z)` per entry, each driven from `sig.a`.
    fn phase4_ir(xs: &[(u32, u32, bool)]) -> PlacementIr {
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

    /// What `phase4_buffer_tick_invariant_holds` asserts, over one draw of
    /// [`phase4_scope_strategy`], as a function so a named test can replay a draw without the
    /// regressions file.
    fn check_phase4_scope(xs: &[(u32, u32, bool)]) -> Result<(), TestCaseError> {
        let case = format!("xs={xs:?}");
        let legalized = legalize_phase4(phase4_ir(xs), &case)?;
        for entry in &legalized.scopes {
            check_buffer_sites(entry, &case)?;
            check_cell_charges(entry, &case)?;
        }
        for entry in &legalized.scopes {
            check_scope_total(entry, &case)?;
        }
        Ok(())
    }

    /// `compile_routing → compile_delay → compile_crossing` over
    /// one scope, failing on a routing or delay diagnostic and
    /// rejecting the case on a crossing one.
    fn legalize_phase4(ir: PlacementIr, case: &str) -> Result<ScopedPlacementIr, TestCaseError> {
        let routing = compile_routing(&scoped(ScopeKind::Struct, "prop", ir));
        prop_assert!(
            routing.diagnostics.is_empty(),
            "routing diagnostics for {}: {:?}",
            case,
            routing.diagnostics,
        );
        let delayed = compile_delay(&routing.scoped);
        prop_assert!(
            delayed.diagnostics.is_empty(),
            "delay diagnostics for {}: {:?}",
            case,
            delayed.diagnostics,
        );
        let legalized = compile_crossing(&delayed.scoped);
        // A refused scope is dropped and has no buffers to check; refusing is a correct answer
        // for some layouts, so the case is rejected rather than failed.
        prop_assume!(legalized.diagnostics.is_empty());
        Ok(legalized.scoped)
    }

    /// Where the buffers landed, not just how many: totals alone let a coord move anywhere.
    fn check_buffer_sites(entry: &ScopedPlacementIrEntry, case: &str) -> Result<(), TestCaseError> {
        let (router, trees) = laid_trees(entry);
        for cell in &entry.ir.cells {
            for buffer in cell.buffer_coords() {
                let BufferSegment::Port(port) = buffer.port else {
                    panic!("a cell's buffer must name one of its driver ports");
                };
                let driver = cell
                    .drivers
                    .iter()
                    .find(|d| d.port == port)
                    .expect("every buffer names a driver of its own cell");
                let tree = &trees[&driver.net];
                // `Router::dust`, not `wire_path`, for where the
                // buffer stands: the latter holds the terminals
                // too, and would accept a repeater standing on a
                // cell body.
                let dust: HashSet<CellCoord> = router.dust(tree).into_iter().collect();
                // `wire_path` for what it may stand beside: a cell
                // body or a pad of the net is cut off by a repeater
                // beside it as surely as dust is.
                let net: HashSet<CellCoord> = tree.wire_path().into_iter().collect();
                let on = Buffer {
                    coord: buffer.coord,
                    net: driver.net,
                    case,
                };
                check_on_route(&on, tree, &dust, cell.coord)?;
                let (before, after) = check_straight_through(&on, tree)?;
                check_nothing_of_its_net_beside(&on, &net, before, &after)?;
            }
        }
        Ok(())
    }

    /// The router and the trees it lays for one scope, rebuilt the
    /// way the delay and crossing passes rebuild them.
    fn laid_trees(entry: &ScopedPlacementIrEntry) -> (Router, HashMap<NetRef, NetTree>) {
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

    /// One buffer coord under check: where it stands, the net it
    /// stands on, and the case it came from, for the messages.
    struct Buffer<'a> {
        coord: CellCoord,
        net: NetRef,
        case: &'a str,
    }

    /// The buffer stands over dust its own net laid, on the route
    /// into the cell it was charged to.
    fn check_on_route(
        on: &Buffer<'_>,
        tree: &NetTree,
        dust: &HashSet<CellCoord>,
        sink: CellCoord,
    ) -> Result<(), TestCaseError> {
        // The whole coord, layer included. A repeater takes the
        // layer of the route coord it stands on, so comparing
        // footprints would accept one on the plane under wire
        // that had climbed.
        prop_assert!(
            dust.contains(&on.coord),
            "buffer {:?} is not over dust the routing pass laid for {:?} ({})",
            on.coord,
            on.net,
            on.case,
        );
        // And on *this* segment's route: `wire_path` would accept a coord that refreshes a
        // sibling sink instead.
        let route: HashSet<CellCoord> = tree
            .route_to(sink)
            .expect("every cell is a terminal of the net driving it")
            .into_iter()
            .collect();
        prop_assert!(
            route.contains(&on.coord),
            "buffer {:?} is not over the route into this cell for {:?} ({})",
            on.coord,
            on.net,
            on.case,
        );
        if on.coord.layer == RouteLayer::Plane {
            prop_assert_eq!(on.coord.y, 0);
        }
        Ok(())
    }

    /// The wire runs straight through the buffer: one coord in,
    /// exactly one out, all three on one line at one height.
    /// Written out here rather than through the placer's own
    /// check, so a placer that stopped asking it fails. Returns
    /// the coord in and the coords out.
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
            "buffer {:?} does not have the wire straight through it: in from {:?}, out to {:?} ({})",
            c,
            before,
            after,
            on.case,
        );
        Ok((before, after))
    }

    /// The coords on the six faces of a block on `c`, leaving out
    /// one that would not fit a `u32`. Spelled with `with_layer`
    /// rather than `CellCoord::new`, so each layer is written out
    /// here instead of derived by the rule production uses: the
    /// four in-plane faces are on `c`'s own layer, the face above
    /// is a bridge, and the face below is the plane only under the
    /// first bridge layer.
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

    /// The buffer touches no block of its net but the two it joins. A repeater severs every face
    /// but its front and back, so this is asked of the grid, not the tree. `net` is every coord
    /// of the net, terminals included.
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
            "buffer {:?} has a block of its own net beside it that it does not join ({})",
            c,
            on.case,
        );
        Ok(())
    }

    /// Per cell as well as in total: an over-charged cell and an
    /// under-charged neighbour cancel in a sum.
    fn check_cell_charges(entry: &ScopedPlacementIrEntry, case: &str) -> Result<(), TestCaseError> {
        for cell in &entry.ir.cells {
            let dt = cell
                .local_delay_ticks()
                .expect("legalized cells carry Some(local_delay_ticks)");
            let base = cell.cell.base_delay_ticks();
            // `checked_sub` here too: a cell below its edition base would clamp to zero and match
            // a cell with no buffers.
            let delta = dt.checked_sub(base);
            prop_assert!(
                delta.is_some(),
                "local_delay_ticks {} < base_delay_ticks {} for cell at {:?} ({})",
                dt,
                base,
                cell.coord,
                case,
            );
            let delta = delta.unwrap_or_default();
            let blocks: HashSet<CellCoord> = cell.buffer_coords().iter().map(|b| b.coord).collect();
            let placed = u32::try_from(blocks.len()).expect("buffer block count fits in u32");
            prop_assert_eq!(
                delta,
                placed
                    .checked_mul(BUFFER_REPEATER_TICKS)
                    .expect("block count × BUFFER_REPEATER_TICKS fits in u32"),
                "cell at {:?} charged {} ticks of buffer but stands on {} block(s) ({})",
                cell.coord,
                delta,
                placed,
                case,
            );
        }
        Ok(())
    }

    /// `Σ blocks × BUFFER_REPEATER_TICKS = Σ (local − base)` over
    /// one scope's cells.
    fn check_scope_total(entry: &ScopedPlacementIrEntry, case: &str) -> Result<(), TestCaseError> {
        let buffer_total: u32 = entry
            .ir
            .cells
            .iter()
            .map(|c| {
                let blocks: HashSet<CellCoord> =
                    c.buffer_coords().iter().map(|b| b.coord).collect();
                u32::try_from(blocks.len()).expect("buffer block count fits in u32")
            })
            .sum();
        let mut delta_total: u32 = 0;
        for cell in &entry.ir.cells {
            let dt = cell
                .local_delay_ticks()
                .expect("legalized cells carry Some(local_delay_ticks)");
            let base = cell.cell.base_delay_ticks();
            let delta = dt.checked_sub(base);
            prop_assert!(
                delta.is_some(),
                "local_delay_ticks {} < base_delay_ticks {} for cell at {:?} — delay pass regressed below the edition base",
                dt,
                base,
                cell.coord,
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
            "buffer block count × BUFFER_REPEATER_TICKS ({}) must equal Σ(local_delay_ticks − base_delay_ticks) ({}) for scope `{}` with {}",
            lhs,
            delta_total,
            entry.name,
            case,
        );
        Ok(())
    }

    /// Replays a pinned shape. A rejected case never reached the assertions, which is not the
    /// invariant breaking, so it is reported as such.
    fn replay(result: Result<(), TestCaseError>, case: &str) {
        match result {
            Ok(()) => {}
            Err(TestCaseError::Reject(why)) => {
                panic!("{case}: this shape no longer exercises the bug ({why})")
            }
            Err(TestCaseError::Fail(why)) => panic!("{case}: {why}"),
        }
    }

    /// Two sinks of one net in a column at `x = 62`, at `z = 2` and
    /// `z = 4`. The trunk runs along `z = 0` and turns at
    /// `(62, 0, 0)`; the branch to the far sink leaves the fork at
    /// `(62, 0, 1)` and steps back to `x = 61` to get round the near
    /// one, so `(61, 0, 1)` is dust of the net on a face of the
    /// straight coord `(61, 0, 0)` without being linked to it in the
    /// tree. The fourth repeater falls due on `(61, 0, 1)` itself,
    /// on the branch past the turn, and walking back over the fork
    /// and the turn the first coord that runs straight is
    /// `(61, 0, 0)`; a repeater there would sever the branch, so it
    /// stands on `(60, 0, 0)`.
    ///
    /// [`assert_a_branch_beside`] pins the branch on the net as laid,
    /// and the far sink's sites pin where the walk back lands: the
    /// replay alone would pass a column at which no repeater falls
    /// due near the branch.
    #[test]
    fn a_buffer_walked_back_over_a_turn_stops_short_of_a_branch_beside_it() {
        let xs = [(62, 4, false), (62, 2, false)];
        let case = format!("xs={xs:?}");
        replay(check_phase4_scope(&xs), &case);
        let legalized =
            legalize_phase4(phase4_ir(&xs), &case).expect("the replay above legalized it");
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

    /// The column of `x = 62`'s shape moved to `x = 97`, with the
    /// sinks one row apart. The same branch comes back beside
    /// `(96, 0, 0)`, but here the sixth repeater falls due on that
    /// coord itself, 16 steps past the one on `(80, 0, 0)`: the
    /// first coord the placer asks is the one beside the branch,
    /// with no turn to walk back over. It stands on `(95, 0, 0)`.
    ///
    /// The branch and the sites are pinned as for the `x = 62` shape.
    #[test]
    fn a_buffer_due_beside_a_branch_steps_back_off_it() {
        let xs = [(97, 2, false), (97, 3, false)];
        let case = format!("xs={xs:?}");
        replay(check_phase4_scope(&xs), &case);
        let legalized =
            legalize_phase4(phase4_ir(&xs), &case).expect("the replay above legalized it");
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

    /// The walk back of the `x = 62` shape on a trunk that is not
    /// the pad's row. The cell at `(1, 5)` is the nearest sink, so
    /// the net's first route runs up `x = 1`, and the trunk to the
    /// column at `x = 27` leaves it at the fork `(1, 0, 4)` and runs
    /// along `z = 4`, the count carried through that fork. The
    /// second repeater falls due on the fork `(27, 0, 5)`, and the
    /// branch to `(27, 7)` steps back through `(26, 0, 5)`, beside
    /// `(26, 0, 4)`, so it stands on `(25, 0, 4)`.
    ///
    /// The branch and the sites are pinned as for the `x = 62` shape.
    #[test]
    fn a_buffer_on_a_trunk_fed_through_a_fork_stops_short_of_a_branch_beside_it() {
        let xs = [(27, 6, false), (27, 7, false), (1, 5, false)];
        let case = format!("xs={xs:?}");
        replay(check_phase4_scope(&xs), &case);
        let legalized =
            legalize_phase4(phase4_ir(&xs), &case).expect("the replay above legalized it");
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

    /// The coords the buffers of the cell on `sink` stand on, each
    /// once, in the order its `buffer_coords` first names them.
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

    /// Pins the shape a named test about a branch beside the wire
    /// exists for, on the net as laid: `branch` and `beside` are both
    /// dust of the net, and the tree joins neither to the other, so a
    /// repeater on `beside` would cut `branch` off. (`branch` is on a
    /// face of `beside` by the coords the caller passes.) Each
    /// premise fails on a message of its own.
    fn assert_a_branch_beside(
        router: &Router,
        tree: &NetTree,
        beside: CellCoord,
        branch: CellCoord,
        case: &str,
    ) {
        let spelled = |c: CellCoord| format!("({}, {}, {})", c.x, c.y, c.z);
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

    /// The vertical faces. The near pair at `(47, 0)` and `(47, 1)`
    /// is fed along `z = 0` and off the fork at `(46, 0, 0)`; the
    /// route to the far sink at `(74, 0)` climbs at that fork and
    /// runs on the bridge over the cell at `(47, 0, 0)`. Its third
    /// repeater falls due on `(47, 1, 0)`, which runs straight
    /// between `(46, 1, 0)` and `(48, 1, 0)` and has nothing of the
    /// net on its four in-plane faces — only the sink under it. That
    /// face is what keeps the repeater off it: the walk back passes
    /// the climb and the fork and lands on `(45, 0, 0)`.
    ///
    /// What is under `(47, 1, 0)` is a terminal, not dust, which
    /// [`check_nothing_of_its_net_beside`] counts as a block of the
    /// net, so the replay fails on that check if the repeater
    /// stands on `(47, 1, 0)`. [`assert_only_a_terminal_beside`]
    /// pins that shape on the net as laid, and the sites are
    /// asserted as well, to pin where the walk back lands.
    #[test]
    fn a_buffer_never_stands_over_a_sink_of_its_own_net() {
        let xs = [(74, 0, false), (47, 0, false), (47, 1, false)];
        let case = format!("xs={xs:?}");
        replay(check_phase4_scope(&xs), &case);
        let legalized =
            legalize_phase4(phase4_ir(&xs), &case).expect("the replay above legalized it");
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

    /// The planar face of a terminal. Two sinks side by side at
    /// `(31, 0)` and `(32, 0)`: the near one is fed straight along
    /// `z = 0`, and the route to the far one forks at `(30, 0, 0)`
    /// and goes round the near cell along `z = 1`. Its second
    /// repeater falls due on `(31, 0, 1)`, which runs straight
    /// between `(30, 0, 1)` and `(32, 0, 1)` and has no dust of the
    /// net on any other face — only the near cell's body at
    /// `(31, 0, 0)`. The walk back passes the turn and the fork and
    /// lands on `(29, 0, 0)`.
    ///
    /// [`check_nothing_of_its_net_beside`] is what fails here if the
    /// repeater stands on `(31, 0, 1)`. Unlike
    /// [`a_buffer_never_stands_over_a_sink_of_its_own_net`], the
    /// terminal is in the repeater's own plane, so no climb is
    /// needed to reach this case. [`assert_only_a_terminal_beside`]
    /// pins the shape on the net as laid, and the sites pin where
    /// the walk back lands.
    #[test]
    fn a_buffer_never_stands_beside_a_sink_of_its_own_net() {
        let xs = [(31, 0, false), (32, 0, false)];
        let case = format!("xs={xs:?}");
        replay(check_phase4_scope(&xs), &case);
        let legalized =
            legalize_phase4(phase4_ir(&xs), &case).expect("the replay above legalized it");
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

    /// Pins the shape a named test about a terminal on a face
    /// exists for, on the net as laid rather than through the
    /// sites: the route into `sink` passes `due`, which the wire
    /// reaches from `before` and leaves for `after`; the only block
    /// of the net on `due`'s faces besides those two is `terminal`;
    /// and `terminal` is a terminal, which [`Router::dust`] leaves
    /// out, not dust. Dust of the net on a face of `due` would keep
    /// the repeater off it just the same, and the replay and the
    /// sites would still pass with the terminal no longer the
    /// reason. Each premise fails on a message of its own.
    fn assert_only_a_terminal_beside(
        router: &Router,
        tree: &NetTree,
        sink: CellCoord,
        [before, due, after]: [CellCoord; 3],
        terminal: CellCoord,
        case: &str,
    ) {
        let spelled = |c: CellCoord| format!("({}, {}, {})", c.x, c.y, c.z);
        let gone = format!("{case}: this shape no longer exercises the bug");
        let route = tree
            .route_to(sink)
            .expect("every cell is a terminal of the net driving it");
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

    proptest! {
        #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

        /// On every emitted scope, `Σ (blocks under cell.buffer_coords()) ×
        /// BUFFER_REPEATER_TICKS` equals `Σ (cell.local_delay_ticks() −
        /// cell.cell.base_delay_ticks())`. The blocks under a cell are its buffer coords
        /// deduplicated, since segments of one net share the repeater on their prefix; the sum is
        /// over cells because a repeater two cells hang off is charged to both. A drift in either
        /// pass's repeater count, in `BUFFER_REPEATER_TICKS` or in the base-delay table trips this
        /// one assertion.
        ///
        /// Every cell here is driven by one net, so a sum and a max over the driving nets agree;
        /// `two_distinct_nets_charge_a_cell_for_every_buffer_under_it` covers the multi-net shape.
        /// `checked_sub` and `checked_mul` refuse to clamp a delay below the edition base or a
        /// count past `u32`.
        #[test]
        fn phase4_buffer_tick_invariant_holds(xs in phase4_scope_strategy()) {
            check_phase4_scope(&xs)?;
        }
    }
}
