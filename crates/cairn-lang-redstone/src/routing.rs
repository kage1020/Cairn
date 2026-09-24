//! Placement IR → routed Placement IR lowering (Steiner routing).
//!
//! Stage 2 of the five-stage pipeline `spec/redstone` "Place-and-route"
//! lays out. Lays a rectilinear Steiner tree per driver net inside each
//! scope's [`crate::placement_ir::CircuitRegionReservation`] and
//! rewrites every cell's and actuator pad's
//! [`crate::placement_ir::PlacedCellNode::wire_length`] from `None` to
//! `Some(routed length of the nets driving it, summed)`.
//! [`crate::placement_ir::PlacedCellNode::local_delay_ticks`] stays
//! `None`; stage 3 ([`crate::delay::compile_delay`]) promotes it.
//!
//! - **Nets.** Each cell driver is a sink on its driver's net and each
//!   output driver a sink on its actuator's net; the source is the input
//!   pad for `NetRef::Input` and the cell body for `NetRef::Cell`. An
//!   unused input adds its pad to the occupancy set but no net.
//! - **Trees.** [`crate::routing_geometry::Router`] grows each net one
//!   sink at a time by the cheapest path that runs through no block and
//!   neither over nor one step beside an earlier net's dust in its own
//!   plane, climbing to a [`crate::placement_ir::RouteLayer::Bridge`]
//!   layer where the plane offers no way round. Nets are laid in
//!   [`crate::routing_geometry::net_order`], a total order, so the delay
//!   and crossing passes rebuild the same trees. Keeping nets apart here
//!   rather than at stage 4 is what gets the climb measured into
//!   `wire_length` and into the delay pass's tick count.
//! - **Cross-layer pairs.** A net that climbed runs over, or one step
//!   across from, the net it cleared. Separating those is the physical
//!   tile layer's obligation, so they are named once, here, by
//!   `W_ROUTE_CROSS_LAYER_CLEARANCE` rather than refused.
//! - **Refusals.** All `E_ROUTE_CONGESTION`, each eliding the scope so a
//!   partial `wire_length` never reaches stage 3: a pad the reservation
//!   cannot fit (its saturated z collapses onto a cell or another pad);
//!   a sink with no free path; and, after every net is laid,
//!   `cells * CELL_FOOTPRINT + wire-only coords > reserved area`. Each
//!   primary says which, so a reader can tell the placement pass's
//!   pessimistic cell budget from the routed layout.
//! - **Attribution.** `wire_length` is summed over the distinct nets
//!   driving a cell (two ports reading one signal are one strand), each
//!   measured as the routed path from the net's source into the cell —
//!   the measure stage 3 counts buffer repeaters against.
//!
//! The pad coordinates are derived on the fly and not stored; they would
//! become `PlacementIr` fields if a consumer outside routing ever needed
//! them. `RouteLayer::Bridge` has one producer, this pass; `RouteLayer::Via`
//! has none, because a climb is a step between two coords rather than a
//! coord of its own.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::pass::{
    OpenScope, Skipped, attribute_nodes, lay_nets, lower_scopes, open_scope, source_of_net_lenient,
};
use crate::placement::{CONGESTION_FIX, area_ratio_tenths};
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{net_order, sum_over_driving_nets, tile_layer_clearance};

/// Per-cell footprint used by the post-routing congestion budget.
/// Re-exports [`crate::placement::CELL_FOOTPRINT`] so this pass carries
/// the same footprint model the placement pass used and adds wire
/// occupancy on top.
pub const CELL_FOOTPRINT: u32 = crate::placement::CELL_FOOTPRINT;

/// Output of a [`compile_routing`] run.
///
/// Mirrors the shape of [`crate::placement::PlacementOutput`] so
/// callers see a uniform result type across every stage of the
/// place-and-route pipeline. The routed IR is a
/// [`ScopedPlacementIr`] with every non-failed scope's
/// `wire_length` promoted from `None` to `Some(_)` — no new IR type;
/// the routing pass is one
/// [`crate::placement_ir::PlacementPhase::route`] transition per
/// cell, per the producer↔variant table on that enum.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct RoutingOutput {
    /// Placement IR for every scope whose routing succeeded, with
    /// every cell's `wire_length` field populated.
    pub scoped: ScopedPlacementIr,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

impl RoutingOutput {
    /// Empty output (no routed scopes, no diagnostics).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lower a [`ScopedPlacementIr`] into a routed [`ScopedPlacementIr`].
///
/// Reads the reservation from every scope's `region` field rather than
/// re-consulting the Intent IR — the Placement IR is self-describing
/// by construction, so the routing pass has no `IntentModule`
/// dependency.
///
/// One entry per non-empty [`PlacementIr`] whose routing succeeded;
/// scopes whose routing raises an Error-severity diagnostic (today,
/// only `E_ROUTE_CONGESTION`) are elided from the output so a partial
/// `wire_length` cannot pollute the delay-insertion pass downstream.
#[must_use]
pub fn compile_routing(placement: &ScopedPlacementIr) -> RoutingOutput {
    let (scoped, diagnostics) = lower_scopes(placement, route_scope);
    RoutingOutput {
        scoped,
        diagnostics,
    }
}

/// Result of routing one scope: the routed IR plus whatever the layout
/// is advised of on success, a single Error-severity diagnostic on
/// failure. A refused scope carries no advisory: what an elided scope
/// would have left the tile layer is not an obligation anything will be
/// asked to discharge.
type ScopeRouting = Result<(PlacementIr, Vec<Diagnostic>), Diagnostic>;

fn route_scope(entry: &ScopedPlacementIrEntry) -> ScopeRouting {
    let source = &entry.ir;
    let OpenScope {
        mut ir,
        region,
        cell_coords,
        blocks,
    } = match open_scope(entry) {
        Err(Skipped::Empty) => return Ok((source.clone(), Vec::new())),
        Err(Skipped::MissingRegion) => {
            // Loud in debug so a fixture regression trips fast; a
            // deterministic pass-through in release.
            debug_assert!(
                source.cells.is_empty() && source.outputs.is_empty(),
                "route_scope received a PlacementIr with cells or pads but no region — placement should have refused it",
            );
            return Ok((source.clone(), Vec::new()));
        }
        Ok(scope) => scope,
    };

    let nets = lay_nets(
        &ir,
        &blocks,
        entry,
        &region,
        source_of_net_lenient(&region, &cell_coords),
    )?;

    // Occupancy for the congestion figure below: the blocks, then the
    // dust laid over them. The refusal a repeated coord earns is
    // `lay_nets`', asked before any of this, so what is left here is
    // counting.
    let mut occupancy: HashSet<CellCoord> = HashSet::with_capacity(ir.cells.len() * 4);
    occupancy.extend(blocks.iter().map(|site| site.coord));
    for net in net_order(&nets.sinks) {
        for coord in nets.trees[&net].wire_path() {
            occupancy.insert(coord);
        }
    }

    // Per net rather than the shared tree total: dust a cell shares with
    // a sibling sink feeds both, and the congestion budget below is
    // where the shared total is counted once. An output has exactly one
    // segment, its driver to its pad.
    attribute_nodes(
        &mut ir,
        entry,
        |cell| sum_over_driving_nets(&cell.drivers, |net| nets.segment(net, cell.coord)),
        |output| nets.segment(output.driver, output.pad),
        |phase, len, identity| phase.route_at(len, identity),
    );

    // Congestion against the actual post-routing footprint: the
    // placement pass's pessimistic per-cell budget, plus the wire-only
    // coords laid on top (cell coords excluded so they are not counted
    // twice).
    let cell_coord_set: HashSet<CellCoord> = cell_coords.iter().copied().collect();
    let wire_only: u64 = occupancy
        .iter()
        .filter(|c| !cell_coord_set.contains(c))
        .count() as u64;
    let cell_budget = (ir.cells.len() as u64).saturating_mul(u64::from(CELL_FOOTPRINT));
    let used = cell_budget.saturating_add(wire_only);
    let reserved = region.reserved_area();
    if used > reserved {
        return Err(congestion_diagnostic(entry, &region, used));
    }

    // Below the refusals, so a scope this pass elides carries no
    // advisory; said once, here, because this is the stage that made
    // the pairs.
    let advisories: Vec<Diagnostic> =
        tile_layer_clearance(&nets.sinks, &nets.trees, &nets.router, entry, &region)
            .into_iter()
            .collect();

    Ok((ir, advisories))
}

fn congestion_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    used: u64,
) -> Diagnostic {
    let reserved = reservation.reserved_area();
    // `reserved_area > 0` is a placement-side invariant; a hand-built IR
    // that breaks it gets its own primary rather than a division panic.
    if reserved == 0 {
        return zero_reservation_diagnostic(entry, reservation);
    }
    let (whole, tenths) = area_ratio_tenths(used, reserved);
    let primary = format!(
        "routed netlist for {kind} `{name}` occupies ~{whole}.{tenths}x the reserved area (void={void}, region {width}x{depth})",
        kind = entry.kind.label(),
        name = entry.name,
        void = reservation.void,
        width = reservation.width,
        depth = reservation.depth,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        CONGESTION_FIX,
    )
}

fn zero_reservation_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` has a zero-area reservation (void={void}, region {width}x{depth}) — routing cannot lay any wire",
        kind = entry.kind.label(),
        name = entry.name,
        void = reservation.void,
        width = reservation.width,
        depth = reservation.depth,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        CONGESTION_FIX,
    )
}

#[cfg(test)]
mod tests {
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
            Router, block_sites, collect_nets, input_pad, manhattan, net_trees,
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
                    let occupied: HashSet<CellCoord> =
                        blocks.iter().map(|site| site.coord).collect();
                    let router = Router::new(&region, &blocks);
                    let nets = collect_nets(ir);
                    let trees = net_trees(&nets, &router, |net| match net {
                        NetRef::Input(i) => input_pad(i as usize, &region),
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
                            let walked =
                                u32::try_from(route.len() - 1).expect("a route fits in u32");
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
        // The pad lands at (0,0,1). Walling the row at (1,0,0) and the
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
        assert_eq!(delay_refusal.primary, refusal.primary);
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
        assert_eq!(crossing_refusal.primary, refusal.primary);
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
}
