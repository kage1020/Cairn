//! Crate-internal coverage for the routing pass's phase-transition
//! commit. `tests/routing.rs` drives real synth fixtures and can
//! only ever hand this pass a uniformly `Unrouted` IR; building a
//! scope whose cells sit in different phases needs the
//! `pub(crate)` `phase` field, so it lives here.

use cairn_lang_core::Edition;
use cairn_lang_core::error::Span;

use super::compile_routing;
use crate::edition_netlist_ir::EditionCell;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
use crate::placement_ir::{CellCoord, PlacedCellNode, PlacementIr, PlacementPhase};
use crate::test_fixtures::{reservation, scoped};

fn placed_cell(coord: CellCoord, phase: PlacementPhase) -> PlacedCellNode {
    PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: vec![],
        coord,
        phase,
        span: Span::default(),
    }
}

/// `wire_length` reports the dust that feeds a cell, so it counts
/// the routed path and not the straight line between the driver's
/// source and the cell. The sink here is 14 blocks from its pad
/// with something standing halfway, so it is fed by 16 blocks of
/// dust; a record carrying 14 beside a `local_delay_ticks` charged for
/// 16 describes no single layout.
///
/// The blocker drives nothing — a block is all the fixture needs,
/// and a driver would make it a second sink of the same net.
/// No net in the example corpus has dust drawn inside a component.
///
/// The property the router exists for, checked against every `.crn`
/// that ships rather than against generated layouts: a coord of a
/// net's wire is either free space, that net's own source, or one
/// of its own sinks. Anything else is a strand of redstone inside a
/// comparator or a pressure plate.
///
/// A unit test rather than one in `tests/routing.rs` because the
/// wire coords are crate-internal — the routed IR carries lengths,
/// not paths.
#[test]
fn no_example_draws_dust_inside_a_component() {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    use cairn_lang_core::{lower, parse};

    use crate::routing_geometry::{
        PadColumn, Router, block_sites, collect_nets, input_pad, manhattan, net_trees,
    };

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    // What the corpus is worth is what it still routes. A count of
    // wire coords would stay comfortably positive while an example
    // slid into `E_ROUTE_CONGESTION` and vanished from the walk, so
    // the guards below are the scopes placement laid against the
    // scopes routing kept, and the number of strands that actually
    // go round something.
    let mut placed_scopes = 0usize;
    let mut routed_scopes = 0usize;
    let mut detours = 0usize;
    let mut climbs = 0usize;
    for file in std::fs::read_dir(&dir).expect("read examples") {
        let path = file.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("crn") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read example");
        let Ok(module) = parse(&source) else { continue };
        let intent = lower(&module);
        let synth = crate::synth::synthesize(&intent);
        for edition in [Edition::Java, Edition::Bedrock] {
            let netlist = crate::netlist::compile_netlist(&synth.scoped);
            let edition_netlist =
                crate::edition_netlist::compile_edition_netlist(&netlist, edition);
            let placed = crate::placement::compile_placement(&edition_netlist, &intent);
            let laid = compile_routing(&placed.scoped);
            placed_scopes += placed.scoped.scopes.len();
            routed_scopes += laid.scoped.scopes.len();
            for entry in &laid.scoped.scopes {
                let ir = &entry.ir;
                let Some(region) = ir.region.clone() else {
                    continue;
                };
                let coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();
                let blocks = block_sites(ir, &region);
                let occupied: HashSet<CellCoord> = blocks.iter().map(|site| site.coord).collect();
                let router = Router::new(&region, &blocks);
                let nets = collect_nets(ir);
                let trees = net_trees(&nets, &router, |net| match net {
                    NetRef::Input(i) => input_pad(i as usize, PadColumn::of(ir), &region),
                    NetRef::Cell(j) => coords[j as usize],
                });
                let where_it_is = format!(
                    "{}: {edition:?} {} `{}`",
                    path.display(),
                    entry.kind.label(),
                    entry.name,
                );
                let mut owner: HashMap<CellCoord, NetRef> = HashMap::new();
                for (net, tree) in &trees {
                    let source = tree.wire_path()[0];
                    let mine: HashSet<CellCoord> =
                        nets[net].iter().copied().chain([source]).collect();
                    for coord in tree.wire_path() {
                        assert!(
                            !occupied.contains(&coord) || mine.contains(&coord),
                            "{where_it_is} draws {net:?} through {coord:?}",
                        );
                    }
                    let dust = router.dust(tree);
                    climbs += dust.iter().filter(|coord| coord.y > 0).count();
                    claim(&mut owner, *net, &dust, &where_it_is);
                    for sink in &nets[net] {
                        let route = tree.route_to(*sink).expect("a sink of this net");
                        let walked = u32::try_from(route.len() - 1).expect("a route fits in u32");
                        if walked > manhattan(source, *sink) {
                            detours += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        routed_scopes, placed_scopes,
        "routing refused a scope the corpus used to lay out, and this walk \
         cannot see what it no longer visits",
    );
    assert!(
        routed_scopes >= 4,
        "the corpus has to keep placing circuits for this to mean anything: \
         {routed_scopes} scope(s)",
    );
    assert!(
        detours > 0,
        "no strand in the corpus goes round anything, so nothing here would \
         notice a router that drew straight through",
    );
    assert!(
        climbs > 0,
        "no strand in the corpus leaves the ground layer, so the escape is \
         no longer shown by any `.crn` that ships and the byte-identity \
         test named for it is measuring a scope without one",
    );
}

/// One net's dust, checked against the nets already claimed and
/// then added to them.
///
/// Two nets on one coord, or one step apart in one plane, are two
/// signals on one strand of dust. The proptest in
/// `routing_geometry` holds this over generated boxes; the walk
/// below holds it over the geometry the placement pass actually
/// produces, which is where the corpus's shorts used to come from.
///
/// `owner` holds dust and the reach is asked about per coord, so
/// two strands two apart — each reaching the coord between them —
/// are not mistaken for one.
fn claim(
    owner: &mut std::collections::HashMap<CellCoord, NetRef>,
    net: NetRef,
    dust: &[CellCoord],
    where_it_is: &str,
) {
    use crate::routing_geometry::beside;

    for coord in dust {
        for taken in std::iter::once(*coord).chain(beside(*coord)) {
            if let Some(other) = owner.get(&taken)
                && *other != net
            {
                panic!(
                    "{where_it_is} runs {net:?} through {coord:?}, which \
                     {other:?} stands on or reaches",
                );
            }
        }
    }
    for coord in dust {
        owner.insert(*coord, net);
    }
}

/// A scope with several unwireable sinks says how many, so sizing
/// it is one decision rather than a fix-one-recompile loop.
///
/// `Router::tree` strands every sink still unconnected the moment
/// one round of the search comes back empty, so `unreachable()`
/// holds them in batches; naming only the first would send the
/// author round again for each.
#[test]
fn a_scope_with_several_unwireable_sinks_counts_them() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(4, 2, 1));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    // The pad lands at (0,0,0). Walling the row at (1,0,0) and the
    // course above the two sinks leaves them nothing to be fed
    // from, and `void=1` reserves no layer to come in over.
    for coord in [
        CellCoord::new(1, 0, 0),
        CellCoord::new(2, 0, 1),
        CellCoord::new(3, 0, 1),
    ] {
        ir.cells.push(placed_cell(coord, PlacementPhase::Unrouted));
    }
    for coord in [CellCoord::new(2, 0, 0), CellCoord::new(3, 0, 0)] {
        ir.cells.push(PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers: vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            coord,
            phase: PlacementPhase::Unrouted,
            span: Span::default(),
        });
    }

    let routed = compile_routing(&scoped(ScopeKind::Struct, "boxed", ir));
    let refusal = routed
        .diagnostics
        .iter()
        .find(|d| d.code == crate::DiagnosticCode::RouteCongestion)
        .unwrap_or_else(|| panic!("two walled-in sinks must refuse: {:?}", routed.diagnostics));
    assert!(
        refusal.primary.contains("cannot reach (2,0,0)"),
        "the first sink in net order is the anchor: {}",
        refusal.primary,
    );
    assert!(
        refusal
            .primary
            .contains("1 more of this scope's sinks cannot be reached either"),
        "and the rest are counted rather than left for the next run: {}",
        refusal.primary,
    );
}

/// A sink the reservation cannot reach is refused by every pass
/// that would measure it, naming the same two coords.
///
/// Stage 2 elides the scope, so stages 3 and 4 never see one in a
/// real run. They rebuild the trees from the IR, though, and a
/// caller who skipped stage 2 would hand them one. The tree answers
/// for a stranded sink — one step, straight to the source — so
/// without the check the later stages would not panic; they would
/// write a tick count and a buffer list for a circuit that cannot
/// be wired, and nothing in the dump would say so.
#[test]
fn an_unreachable_sink_is_refused_by_every_pass_that_measures_it() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(3, 2, 1));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    // The pad lands at (0,0,0). Walling (1,0,0) and (2,0,1) in
    // leaves the sink at (2,0,0) with no free neighbour, and
    // `void=1` reserves no layer to come in over the top.
    for coord in [CellCoord::new(1, 0, 0), CellCoord::new(2, 0, 1)] {
        ir.cells.push(placed_cell(coord, PlacementPhase::Unrouted));
    }
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        coord: CellCoord::new(2, 0, 0),
        phase: PlacementPhase::Unrouted,
        span: Span::default(),
    });

    let routed = compile_routing(&scoped(ScopeKind::Struct, "boxed", ir.clone()));
    let refusal = routed
        .diagnostics
        .iter()
        .find(|d| d.code == crate::DiagnosticCode::RouteCongestion)
        .unwrap_or_else(|| panic!("a walled-in sink must refuse: {:?}", routed.diagnostics));
    assert!(
        refusal.primary.contains("cannot reach (2,0,0)")
            && refusal.primary.contains("from the driver at (0,0,0)"),
        "the refusal names both ends: {}",
        refusal.primary,
    );
    assert!(
        refusal
            .primary
            .contains("a wire passes through none of the three"),
        "the refusal says why a route cannot be found, not only that one \
         was not: {}",
        refusal.primary,
    );
    assert!(
        routed.scoped.scopes.is_empty(),
        "the failed scope is elided rather than half-attributed",
    );
    // Everything past the opening noun has to be identical across
    // the three; the noun itself is what tells an author which pass
    // refused, so it is the one word that must differ.
    let said = refusal
        .primary
        .strip_prefix("placed netlist for ")
        .unwrap_or_else(|| panic!("stage 2 reads a placed netlist: {}", refusal.primary))
        .to_owned();

    // The same layout handed straight to stage 3, and then to
    // stage 4, as a caller who skipped stage 2 would. Each rebuilds
    // the trees, and each has to reach the same verdict: the
    // alternative is a dump whose ticks and buffer coords describe
    // a circuit nothing can build.
    for cell in &mut ir.cells {
        cell.phase = PlacementPhase::Routed { wire_length: 0 };
    }
    let delayed = crate::delay::compile_delay(&scoped(ScopeKind::Struct, "boxed", ir.clone()));
    let delay_refusal = delayed
        .diagnostics
        .iter()
        .find(|d| d.code == crate::DiagnosticCode::RouteCongestion)
        .unwrap_or_else(|| panic!("stage 3 must refuse too: {:?}", delayed.diagnostics));
    assert_eq!(delay_refusal.primary, format!("routed netlist for {said}"));
    assert!(delayed.scoped.scopes.is_empty());

    for cell in &mut ir.cells {
        cell.phase = PlacementPhase::Delayed {
            wire_length: 0,
            local_delay_ticks: 0,
        };
    }
    let legalized = crate::crossing::compile_crossing(&scoped(ScopeKind::Struct, "boxed", ir));
    let crossing_refusal = legalized
        .diagnostics
        .iter()
        .find(|d| d.code == crate::DiagnosticCode::RouteCongestion)
        .unwrap_or_else(|| panic!("stage 4 must refuse too: {:?}", legalized.diagnostics));
    assert_eq!(
        crossing_refusal.primary,
        format!("delayed netlist for {said}")
    );
    assert!(legalized.scoped.scopes.is_empty());
}

#[test]
fn wire_length_counts_the_routed_path() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(16, 3, 2));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(placed_cell(
        CellCoord::new(7, 0, 0),
        PlacementPhase::Unrouted,
    ));
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        coord: CellCoord::new(14, 0, 0),
        phase: PlacementPhase::Unrouted,
        span: Span::default(),
    });
    let routed = compile_routing(&scoped(ScopeKind::Struct, "walled", ir));
    assert!(routed.diagnostics.is_empty(), "{:?}", routed.diagnostics);
    let lengths: Vec<Option<u32>> = routed.scoped.scopes[0]
        .ir
        .cells
        .iter()
        .map(PlacedCellNode::wire_length)
        .collect();
    assert_eq!(
        lengths,
        vec![Some(0), Some(16)],
        "the sink is 14 blocks away and 16 blocks of wire from its driver",
    );
}

#[test]
#[should_panic(
    expected = "for cell #1 at (4,0,1) in struct `mixed` — routing must run exactly once per placement"
)]
fn route_panic_names_the_offending_cell_not_the_first_one() {
    // Re-running the whole pass always trips on `cells[0]`, which
    // would let a regression that hardcoded the index to zero — or
    // that read the coord off the wrong cell — pass unnoticed. A
    // hand-built IR whose first cell is still `Unrouted` while the
    // second is already `Routed` forces the panic past the head of
    // the loop, so both the index and the coord have to be
    // threaded from the cell actually being transitioned.
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(8, 3, 2));
    ir.cells.push(placed_cell(
        CellCoord::new(0, 0, 0),
        PlacementPhase::Unrouted,
    ));
    ir.cells.push(placed_cell(
        CellCoord::new(4, 0, 1),
        PlacementPhase::Routed { wire_length: 0 },
    ));
    let _ = compile_routing(&scoped(ScopeKind::Struct, "mixed", ir));
}
