use cairn_lang_core::Edition;
use cairn_lang_core::error::Span;

use super::compile_routing;
use crate::edition_netlist_ir::EditionCell;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
use crate::placement_ir::{CellCoord, PlacedCellNode, PlacementIr, PlacementPhase};
use crate::test_fixtures::{ROUTED, placed_cell, reservation, scoped};

#[test]
fn no_example_draws_dust_inside_a_component() {
    use std::collections::{HashMap, HashSet};

    use cairn_lang_core::{lower, parse};

    use crate::grid::{PadColumn, input_pad, manhattan};
    use crate::routing_geometry::{Router, block_sites, collect_nets, net_trees};

    let mut placed_scopes = 0usize;
    let mut routed_scopes = 0usize;
    let mut detours = 0usize;
    let mut climbs = 0usize;
    for (path, source) in examples() {
        let module =
            parse(&source).unwrap_or_else(|err| panic!("{} must parse: {err}", path.display()));
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
        "routing refused a scope placement laid out",
    );
    assert!(
        routed_scopes >= 4,
        "the corpus places too few circuits: {routed_scopes} scope(s)",
    );
    assert!(detours > 0, "no strand in the corpus goes round anything");
    assert!(
        climbs > 0,
        "no strand in the corpus leaves the ground layer",
    );
}

/// Every `.crn` under `examples/`, with its path.
fn examples() -> Vec<(std::path::PathBuf, String)> {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    let mut found = Vec::new();
    for file in std::fs::read_dir(&dir).expect("read examples") {
        let path = file.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("crn") {
            let source = std::fs::read_to_string(&path).expect("read example");
            found.push((path, source));
        }
    }
    assert!(
        found.len() >= 12,
        "the walk reads every `.crn` under `examples/`: {} found",
        found.len(),
    );
    found
}

/// Adds one net's dust to `owner`, panicking where it stands on or beside another net's.
fn claim(
    owner: &mut std::collections::HashMap<CellCoord, NetRef>,
    net: NetRef,
    dust: &[CellCoord],
    where_it_is: &str,
) {
    use crate::grid::beside;

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

#[test]
fn a_scope_with_several_unwireable_sinks_counts_them() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(4, 2, 1));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    for coord in [
        CellCoord::new(1, 0, 0),
        CellCoord::new(2, 0, 1),
        CellCoord::new(3, 0, 1),
    ] {
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            coord,
            vec![],
            PlacementPhase::Unrouted,
        ));
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

#[test]
fn an_unreachable_sink_is_refused_by_every_pass_that_measures_it() {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(3, 2, 1));
    ir.inputs.push(crate::netlist_ir::NetlistInput {
        name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    for coord in [CellCoord::new(1, 0, 0), CellCoord::new(2, 0, 1)] {
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            coord,
            vec![],
            PlacementPhase::Unrouted,
        ));
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
    // Only the opening noun, which names the pass, may differ across the three.
    let said = refusal
        .primary
        .strip_prefix("placed netlist for ")
        .unwrap_or_else(|| panic!("stage 2 reads a placed netlist: {}", refusal.primary))
        .to_owned();

    // The same layout handed straight to stages 3 and 4, as a caller who skipped stage 2 would.
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
        EditionCell::JavaRepeaterOr,
        CellCoord::new(7, 0, 0),
        vec![],
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
    // A `Routed` second cell, so the panic cannot name `cells[0]` by accident.
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(8, 3, 2));
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(0, 0, 0),
        vec![],
        PlacementPhase::Unrouted,
    ));
    ir.cells.push(placed_cell(
        EditionCell::JavaRepeaterOr,
        CellCoord::new(4, 0, 1),
        vec![],
        ROUTED,
    ));
    let _ = compile_routing(&scoped(ScopeKind::Struct, "mixed", ir));
}
