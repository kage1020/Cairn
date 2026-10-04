//! Delayed Placement IR → legalized Placement IR lowering (crossing
//! legalization).
//!
//! Stage 4 of the five-stage pipeline `spec/redstone` "Place-and-route"
//! lays out. Rebuilds every net's Steiner tree through the same call the
//! routing and delay passes make, and fills every cell's and actuator
//! pad's [`crate::placement_ir::PlacedCellNode::buffer_coords`] with the
//! coord of each implicit buffer repeater the delay pass counted.
//!
//! The wire itself needs no legalizing here: stage 2 lays each net round
//! the dust of the nets before it and the coords beside that dust, so a
//! scope that reaches this pass has no two signals on one strand. What
//! is left is the crossing a *repeater* would make by standing on a
//! coord it does not own. The delay pass chose those coords off the
//! same routed tree, and this pass reads them back.
//!
//! A repeater refreshes the dust it stands on, so each one stands on its
//! net's routed tree, where `crate::delay::repeater_sites` puts it. This
//! pass builds its own set from the trees it rebuilt; it is the set the
//! delay pass charged ticks from because that function is deterministic
//! and nothing between the two stages moves a cell, not because one is
//! handed to the other. Each segment records the repeaters on
//! `route_to`, the routed path from the net's source to *this* sink.
//!
//! A repeater is never contested, and never stands on a cell body or a
//! pad. A site needs a parent and exactly one child, so it is never the
//! source and never a leaf; every terminal is a leaf, because
//! `block_sites` makes cells and pads obstacles and `Router::tree` keeps
//! each path's far end out of the coords a later path may leave from.
//! So a site is dust of its own net, and stage 2 gave that net its dust
//! alone. Two segments of one net do pass the
//! same repeater (their routes share a prefix); both record it, because
//! `buffer_coords` is an attribution list rather than a block list.
//!
//! Neither [`crate::placement_ir::RouteLayer::Bridge`] nor
//! [`crate::placement_ir::RouteLayer::Via`] has a producer here: bridge
//! coords reach the legalized IR from the routing pass, and `Via` has no
//! producer anywhere.
//!
//! Failed scopes are elided from the output so a stage-5 consumer never
//! reads a partially populated `buffer_coords`. The pass is one
//! [`crate::placement_ir::PlacementPhase::legalize`] transition per
//! cell; no new IR type. Both the layer and the coord vector serde-skip
//! on their defaults, so a scope with nothing to legalize dumps as its
//! delay-pass input did apart from the `stage` tag — which is why that
//! tag exists (see [`crate::placement_ir::PlacementStage`]).

use crate::delay::{Fed, RepeaterSites, repeater_sites_of_scope};
use crate::diagnostic::Diagnostic;
use crate::pass::{
    OpenScope, Skipped, lay_nets, lower_scopes, missing_region_diagnostic, open_scope,
    source_of_net,
};
use crate::placement_ir::{
    BufferCoord, BufferSegment, CellIdentity, PlacementIr, ScopedPlacementIr,
    ScopedPlacementIrEntry,
};
/// Output of a [`compile_crossing`] run.
///
/// Mirrors [`crate::delay::DelayOutput`]'s shape so callers see a
/// uniform result type across every stage of the place-and-route
/// pipeline. The legalized IR is a [`ScopedPlacementIr`] with every
/// non-failed scope's `buffer_coords` populated with one entry per
/// implicit buffer repeater the delay pass counted, each carrying the
/// [`RouteLayer`] of the route coord it stands on. No new IR type; the
/// crossing pass is one [`PlacementPhase::legalize`] transition per
/// cell, per the producer↔variant table on that enum.
///
/// [`RouteLayer`]: crate::placement_ir::RouteLayer
/// [`PlacementPhase::legalize`]: crate::placement_ir::PlacementPhase::legalize
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct CrossingOutput {
    /// Placement IR for every scope whose crossing legalization
    /// succeeded, with every cell's `buffer_coords` populated to match
    /// the buffer tick contribution the delay pass folded into
    /// `local_delay_ticks`.
    pub scoped: ScopedPlacementIr,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

impl CrossingOutput {
    /// Empty output (no legalized scopes, no diagnostics).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lower a delayed [`ScopedPlacementIr`] into a legalized
/// [`ScopedPlacementIr`].
///
/// Reads every cell's coord and the scope's
/// [`CircuitRegionReservation`] out of the input IR — the Placement
/// IR is self-describing by construction, so the crossing pass has no
/// `IntentModule` dependency.
///
/// One entry per non-empty [`PlacementIr`] whose legalization
/// succeeded; a scope that raises [`DiagnosticCode::NoCircuitRegion`],
/// either [`DiagnosticCode::RouteCongestion`] that
/// `crate::pass::lay_nets` raises — a pad row the reservation cannot
/// hold, or a sink no route reaches — or the
/// [`DiagnosticCode::AttenuationLimit`] it raises for a sink further
/// from its driver than the v1 cap in a straight line, or the one
/// raised for a stretch of dust no repeater can stand on, is
/// elided from the output so a partial `buffer_coords` set cannot
/// pollute the downstream block-array voxel lowering. Every finding
/// this pass makes refuses its scope, so there is no warning that
/// outlives one.
///
/// [`CircuitRegionReservation`]: crate::placement_ir::CircuitRegionReservation
/// [`DiagnosticCode::NoCircuitRegion`]: crate::DiagnosticCode::NoCircuitRegion
/// [`DiagnosticCode::RouteCongestion`]: crate::DiagnosticCode::RouteCongestion
/// [`DiagnosticCode::AttenuationLimit`]: crate::DiagnosticCode::AttenuationLimit
#[must_use]
pub fn compile_crossing(delayed: &ScopedPlacementIr) -> CrossingOutput {
    let (scoped, diagnostics) = lower_scopes(delayed, |entry| {
        legalize_scope(entry).map(|ir| (ir, Vec::new()))
    });
    debug_assert!(
        diagnostics
            .iter()
            .all(|d| d.severity() == d.code.severity()),
        "a diagnostic renders with its code's severity: every producer in \
         this pass has to agree with `DiagnosticCode::severity`, including \
         one written after the builders below",
    );
    CrossingOutput {
        scoped,
        diagnostics,
    }
}

/// Result of legalizing one scope: the legalized IR on success, the
/// single Error-severity diagnostic that elides the scope on failure.
///
/// The pass raises nothing else. Every finding it can make refuses the
/// scope, so there is no warning arm to carry.
type ScopeLegalization = Result<PlacementIr, Diagnostic>;

fn legalize_scope(entry: &ScopedPlacementIrEntry) -> ScopeLegalization {
    let source = &entry.ir;
    let OpenScope {
        mut ir,
        region,
        cell_coords,
        inputs,
        column,
        blocks,
    } = match open_scope(entry) {
        Err(Skipped::Empty) => return Ok(source.clone()),
        // Same policy as the delay pass: this pass writes `buffer_coords`,
        // which the phase table promises after stage 4.
        Err(Skipped::MissingRegion) => {
            return Err(missing_region_diagnostic(
                entry,
                "delayed",
                "crossing legalization",
            ));
        }
        Ok(scope) => scope,
    };

    let nets = lay_nets(
        &ir,
        &blocks,
        entry,
        "delayed",
        &region,
        source_of_net(&region, column, &cell_coords, inputs),
    )?;

    // Every coord a repeater can be asked to stand on is a coord of its
    // own net's route, and `net_trees` asserts as it builds that no
    // net's dust stands on, or one step from, another's. That is what
    // leaves this pass with a coord to record and no coord to contest.
    let sites = repeater_sites_of_scope(&ir, entry, "delayed", &region, &nets.trees)?;
    let allocation = allocate_buffer_coords(&ir, &sites);

    for (index, (cell, buffers)) in ir.cells.iter_mut().zip(allocation.per_cell).enumerate() {
        // Loud in release too: `legalize_at` panics on any non-`Delayed`
        // variant, naming the cell that tripped it.
        let identity = CellIdentity::new(index, cell.coord, entry);
        cell.phase.legalize_at(buffers, identity);
    }

    for (index, (output, buffers)) in ir.outputs.iter_mut().zip(allocation.per_output).enumerate() {
        let identity = CellIdentity::output(index, output.pad, entry);
        output.phase.legalize_at(buffers, identity);
    }

    Ok(ir)
}

/// Buffer coord allocation for one scope: every cell driver segment,
/// then every actuator segment.
///
/// Each segment records the repeaters of `sites` its routed path runs
/// through. `sites` is this pass's own, built as the delay pass built
/// the one it charged ticks from, so stage 3 and stage 4 describe one
/// circuit. Split out of `legalize_scope` so
/// the entry function stays under clippy's `too_many_lines` budget and
/// the allocation strategy reads as a self-contained table.
fn allocate_buffer_coords(ir: &PlacementIr, sites: &RepeaterSites<'_>) -> BufferAllocation {
    let mut per_cell: Vec<Vec<BufferCoord>> = Vec::with_capacity(ir.cells.len());
    for (cell_index, cell) in ir.cells.iter().enumerate() {
        let mut buffers_for_cell: Vec<BufferCoord> = Vec::new();
        for driver in &cell.drivers {
            // `along` is `None` for a sink that is not a terminal of
            // the net, which `collect_nets` makes unreachable — it
            // built the tree's terminal list out of this very driver
            // list.
            buffers_for_cell.extend(
                sites
                    .along(driver.net, cell.coord)
                    .unwrap_or_else(|| {
                        Fed::Cell(cell_index).is_not_a_terminal(driver.net, cell.coord)
                    })
                    .into_iter()
                    .map(|coord| BufferCoord::new(BufferSegment::Port(driver.port), coord)),
            );
        }
        // Producer-side contract on [`BufferCoord::port`]: every entry
        // must name a driver that actually exists on the owning cell.
        // Trivially true today because the push site sources
        // `driver.port` from the enclosing `for driver in &cell.drivers`
        // loop, but debug-asserted so a future buffer producer (e.g.
        // fan-out duplication) added elsewhere cannot silently emit a
        // `BufferCoord` whose `port` does not match any driver — the
        // downstream voxel lowering would then group buffers under a
        // driver that does not exist.
        debug_assert!(
            buffers_for_cell.iter().all(|b| matches!(
                b.port,
                BufferSegment::Port(port) if cell.drivers.iter().any(|d| d.port == port)
            )),
            "BufferCoord::port must reference a driver on cells[{cell_index}]",
        );
        per_cell.push(buffers_for_cell);
    }

    // The segment out to an actuator is charged for buffers by stage 3
    // exactly as a segment into a cell is, so stage 4 has to give those
    // buffers coords or the two stages disagree about how many exist.
    //
    // An actuator's route leaves its driver along the same trunk a
    // cell's does, so the two pass the same repeaters wherever they
    // share a prefix — and record the same coord, because one repeater
    // standing there refreshes both.
    let mut per_output: Vec<Vec<BufferCoord>> = Vec::with_capacity(ir.outputs.len());
    for (output_index, output) in ir.outputs.iter().enumerate() {
        let buffers_for_output: Vec<BufferCoord> = sites
            .along(output.driver, output.pad)
            .unwrap_or_else(|| {
                Fed::Output(output_index).is_not_a_terminal(output.driver, output.pad)
            })
            .into_iter()
            .map(|coord| BufferCoord::new(BufferSegment::Out, coord))
            .collect();
        // The mirror of the cell-side contract: a buffer on the wire to
        // an actuator belongs to no input port, and saying otherwise
        // would group it under a driver of a cell it is not on.
        debug_assert!(
            buffers_for_output
                .iter()
                .all(|b| matches!(b.port, BufferSegment::Out)),
            "a buffer on the wire to outputs[{output_index}] must name the outward segment",
        );
        per_output.push(buffers_for_output);
    }

    BufferAllocation {
        per_cell,
        per_output,
    }
}

/// Where every implicit buffer repeater in one scope goes: one entry
/// per cell, then one per actuator pad. Two vectors rather than one
/// because the two commit into different nodes, and a single flat list
/// would need the split re-derived at the commit site.
struct BufferAllocation {
    per_cell: Vec<Vec<BufferCoord>>,
    per_output: Vec<Vec<BufferCoord>>,
}

#[cfg(test)]
mod tests {
    //! Crossing-legalization behaviours `tests/crossing.rs` cannot reach
    //! through synth fixtures: shapes only a hand-built `PlacementIr`
    //! produces, such as a segment long enough to need a buffer.

    use cairn_lang_core::Edition;
    use cairn_lang_core::error::Span;

    use std::collections::{HashMap, HashSet};

    use super::compile_crossing;
    use crate::delay::{BUFFER_REPEATER_TICKS, compile_delay};
    use crate::diagnostic::DiagnosticCode;
    use crate::edition_netlist_ir::EditionCell;
    use crate::logic_ir::ScopeKind;
    use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
    use crate::placement_ir::{BufferSegment, PlacedOutputNode};
    use crate::placement_ir::{
        CellCoord, PlacedCellNode, PlacementIr, PlacementPhase, RouteLayer, ScopedPlacementIr,
    };
    use crate::routing::compile_routing;
    use crate::routing_geometry::{
        PadColumn, Router, block_sites, collect_nets, input_pad, net_trees,
    };
    use crate::test_fixtures::{reservation, scoped, staircase};

    fn placed_cell(
        cell: EditionCell,
        coord: CellCoord,
        drivers: Vec<CellPortDriver>,
    ) -> PlacedCellNode {
        PlacedCellNode {
            cell,
            drivers,
            coord,
            phase: PlacementPhase::Delayed {
                wire_length: 0,
                local_delay_ticks: 0,
            },
            span: Span::default(),
        }
    }

    #[test]
    fn no_scopes_input_yields_no_scopes_output() {
        // A module without any redstone (upstream stages already
        // elided every empty scope through `ScopedPlacementIr::push`)
        // survives crossing legalization as a no-op, matching the
        // delay pass's shape.
        let legalized = compile_crossing(&ScopedPlacementIr::new());
        assert!(legalized.diagnostics.is_empty());
        assert!(legalized.scoped.scopes.is_empty());
    }

    #[test]
    fn empty_scope_passes_through_untouched() {
        // A caller-side hand-built input that hands the pass a scope
        // with no cells and no outputs must return that scope verbatim
        // — the "empty" elision is `ScopedPlacementIr::push`'s job on
        // the input side, not this pass's. Mirrors the delay pass's
        // pass-through so a downstream reader sees consistent
        // behaviour across stages.
        let ir = PlacementIr::new(Edition::Java);
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "empty", ir));
        assert!(legalized.diagnostics.is_empty());
        assert_eq!(legalized.scoped.scopes.len(), 1);
        assert!(legalized.scoped.scopes[0].ir.cells.is_empty());
    }

    #[test]
    fn single_net_no_buffer_is_untouched() {
        // 1 cell driven from Input(0), routed segment <= 15 → no
        // buffer coord, no crossing. `buffer_coords` stays empty.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        // `(1, 0, 1)` is what the placement pass stamps for the first
        // cell of a scope. The pad column is `x = 0`, so a cell at the
        // origin stands on input pad #0 — which every stage refuses,
        // and which this fixture is not about. It would also make the
        // assertion below vacuous: pad and cell on one coord is a
        // zero-length net, which needs no buffer whatever the rule is.
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(1, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
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
        // A routed driver segment of 17 steps carries 16 blocks of
        // dust, one past the attenuation limit → exactly one buffer
        // coord, in place of the 16th block, 16 steps along the route.
        // No collision → the buffer sits on `RouteLayer::Plane`.
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
        // Pins the `PlacedCellNode` `Serialize` impl's widest path
        // (stage + cell + drivers + coord + wire_length + local_delay_ticks
        // + buffer_coords). Without this, no test would exercise the
        // full `Legalized { buffer_coords: <non-empty> }` JSON shape —
        // only the narrower `Legalized { buffer_coords: empty }` case
        // is covered by the byte-identity tests. A regression that
        // dropped `buffer_coords` (or announced the wrong
        // `field_count`) would slip past every other assertion here.
        let json = serde_json::to_string(&legalized.scoped)
            .expect("legalized scoped IR must serialise cleanly");
        assert!(
            json.contains(
                "\"buffer_coords\":[{\"port\":\"a\",\"coord\":{\"x\":16,\"y\":0,\"z\":0}}]"
            ),
            "expected buffer_coords entry to appear in JSON verbatim, got {json}",
        );
        // The stage tag and a populated `buffer_coords` coexist: no
        // `.crn` example reaches this path (every fixture's segments
        // sit below the attenuation limit), so this hand-built IR is
        // the only place the pairing is observable.
        assert!(
            json.contains("\"stage\":\"crossing\""),
            "expected the crossing stage tag alongside buffer_coords, got {json}",
        );
    }

    /// A sink fifteen blocks from its driver with something standing
    /// halfway.
    ///
    /// The pad is at `(0,0,0)` and the sink at `(15,0,0)`, so the
    /// straight line between them is 15 steps, 14 blocks of dust, and
    /// asks for no buffer repeater at all. A block at `(7,0,0)` sends
    /// the wire around it, and the two steps that costs put the segment
    /// over the attenuation limit: 17 steps, 16 blocks of dust, and one
    /// repeater on them.
    /// That gap between the straight line and the route is what every
    /// measurement in this pass has to be taken along.
    ///
    /// The blocker drives nothing. A block is all the fixture needs,
    /// and a driver would make it a sink of the net under test — a
    /// different layout, and one with wire of its own running through
    /// the coords the assertions are about.
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
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(15, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        scoped(ScopeKind::Struct, "walled", ir)
    }

    /// The index of the sink in [`walled_scope`]'s cell list.
    const WALLED_SINK: usize = 1;

    /// The buffer materialises on the dust the signal actually
    /// travels, on the 17-step route.
    ///
    /// The straight line from the pad to this sink is 15 steps, so
    /// measuring it asks for no buffer at all: 16 blocks of dust with
    /// nothing refreshing it, which is the signal never arriving. The
    /// coord is pinned rather than derived so a change to the axis
    /// order or the tie-break has to say so here. It is 15 steps along
    /// rather than 16: the route comes back along `z=1` and turns into
    /// the sink at `(15,0,1)`, and a repeater on the turn would face
    /// away from it.
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

    /// A second net over the walled fixture, so the "no buffer sits on
    /// a foreign net's dust" half of `every_buffer_stands_on_dust_the_
    /// routing_pass_laid` has something to reject. `sig.b` runs from
    /// its own pad along the row the `sig.a` route comes back down.
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
        ));
        scope
    }

    /// Every buffer stands on dust **stage 2 laid** — on the plane, a
    /// coord of its own net's [`NetTree::wire_path`]; on a bridge, a
    /// layer above one in the same column — and never on a coord
    /// another net's wire runs through, whichever layer that is.
    ///
    /// "A layer above one in the same column" rather than "the plane
    /// coord below it", because the routed wire itself climbs now: a
    /// candidate the router lifted over a block is a bridge coord, and
    /// a repeater escaping that stands over a bridge rather than over
    /// the ground layer.
    ///
    /// Checked against `wire_path` rather than against the `route_to`
    /// the allocator itself reads: comparing the production path to
    /// itself would assert nothing, and the failure this guards is
    /// precisely a route that wanders off the wire the routing pass
    /// put in the occupancy set.
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
                        (dust.x, dust.z) == (buffer.coord.x, buffer.coord.z)
                            && dust.y <= buffer.coord.y
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

    /// Stage 3 charges ticks for the repeaters a cell's signals pass
    /// through and stage 4 materialises them; the two counts are one
    /// number or the delay is a fiction.
    ///
    /// Distinct coords rather than `buffer_coords().len()`, because
    /// the vector attributes one entry per driver segment and two
    /// segments of one net share the repeater standing on their
    /// prefix — see [`BufferCoord`]. Every cell in this fixture has a
    /// single driver, so the two counts coincide here;
    /// `ports_sharing_a_net_share_the_repeater_the_charge_and_the_dust`
    /// is where they come apart.
    ///
    /// `phase4_invariant` already property-tests that agreement, but
    /// its strategy seeds sinks along one row from one pad, and an
    /// unobstructed layout is exactly where the straight line and the
    /// route coincide — the invariant held there before this pass read
    /// the route at all. The walled fixture is the discriminating
    /// case: Manhattan says 15 steps and no buffer, the route says 17
    /// and one.
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

    /// Two nets want the same 15-block point, and the reservation
    /// decides whether the second one fits.
    ///
    /// `sig.a` runs left-to-right from its pad along `z=1` and cell #0
    /// drives right-to-left, and the straight line each of them would
    /// take is the same row. One of them gets it.
    ///
    /// Both sinks sit one row off that shared row so neither cell body
    /// stands on the other net's line — a block in the way would send
    /// the router round it, and the two nets would stop wanting the
    /// same coords for a reason this fixture is not about.
    fn two_nets_that_want_one_row_scope(void: u32) -> ScopedPlacementIr {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(40, 4, void));
        for name in ["a", "b"] {
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
                span: Span::default(),
            });
        }
        // #0 is the far driver; its own segment buffers off to the
        // side, on the `sig.b` pad row.
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(30, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        // #1 asks for (15,0,1) first and is lifted off it, because the
        // wire cell #0 drives runs through the coord on its way back
        // down the row.
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(20, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        // #2 is fed by cell #0 across 20 blocks and wants the same
        // coord, with `sig.a`'s wire on the plane below it.
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(10, 0, 2),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(0),
            }],
        ));
        scoped(ScopeKind::Struct, "shared", ir)
    }

    /// Two nets that want one row are laid apart, and the row goes to
    /// one of them.
    ///
    /// This is the shape the crossing pass used to report and could
    /// not repair: two signals on one strand of dust. Stage 2 is where
    /// it is answered now, so the assertion is about the wires rather
    /// than about a finding — there is no finding, and that is the
    /// point.
    ///
    /// The fixture checks itself: routing each net against the blocks
    /// alone — which is what the pass did before — puts them both on
    /// the same coord, so the disjointness below is the change and not
    /// the geometry.
    ///
    /// The region is roomy, so the second net goes round rather than
    /// over; the climb is the same rule where there is no room, and
    /// `a_net_with_no_way_round_climbs_over_the_dust_in_its_way` in
    /// `routing_geometry` is where that half is pinned.
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

        // The third hand-copy of the adjacency rule, and the reason
        // `routing_geometry`'s doc counts two: this one is an
        // integration-level check on the pass, so going through
        // `beside` would let a mutant that empties it pass here as
        // well as in the sweeps. Widening the rule means editing three
        // places, not two.
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

    /// A fan-out's shared prefix carries one repeater, and every sink
    /// past it is fed by that one.
    ///
    /// The first cell ends the straight run at `(16,0,0)`, and the
    /// route to every later one leaves the row at `(15,0,0)` to go
    /// round it, so that coord is a fork. A repeater there would face
    /// one branch and starve the other; the one on the trunk just
    /// before it, at `(14,0,0)`, refreshes the signal for all of them,
    /// and a second sink asking for one is asking for a block that is
    /// already in the layout.
    ///
    /// The `void` column is what the rows are for. Escaping around
    /// the repeater needs a layer above the plane, so two sinks used
    /// to cost two blocks under `void=3` and three sinks were refused
    /// under `void=2` for wanting a third layer. Reuse needs no layer
    /// at all, which is why `void=1` is a row here.
    ///
    /// The coord is compared whole rather than by `x`/`z`: a
    /// `CellCoord::new` is on [`RouteLayer::Plane`] by construction
    /// and `layer` is part of the comparison, so a buffer that
    /// escaped upward fails this assertion rather than passing it on
    /// its footprint.
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

    /// A buffer candidate is never a component.
    ///
    /// A candidate sits strictly between the ends of a route, and the
    /// router keeps every coord strictly between them off the blocks —
    /// so the coord a repeater lands on is wire, never a cell body or a
    /// pad. That, together with each net owning its dust alone, is why
    /// this pass has no coord to contest and no escape to make.
    ///
    /// The layout below is the one that used to refuse: `sig.a`'s route
    /// ran straight down the row and its 15-step point landed inside
    /// `sig.b`'s cell, with `void=1` reserving no layer to lift the
    /// repeater onto.
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

    /// `sig.b`'s cell standing where `sig.a`'s route used to put its
    /// repeater.
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
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(16, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        scoped(ScopeKind::Struct, "packed", ir)
    }

    /// Two ports of one cell on one net are fed by one strand of
    /// dust: two attributions, one block, one charge.
    ///
    /// Threaded through routing → delay → crossing rather than handed
    /// a `Delayed` fixture, because the three numbers this pins are
    /// written by three different passes — `wire_length` by stage 2,
    /// `local_delay_ticks` by stage 3, `buffer_coords` by stage 4 — and the
    /// defect was that each of them counted the one segment once per
    /// port.
    ///
    /// `JavaComparatorAnd` rather than a Mux so the base delay is a
    /// pinned 1 rather than the `_Unpinned` sentinel, and because
    /// `sig.s0 = sig.a and sig.a` is how a `.crn` reaches this shape —
    /// see `ports_sharing_a_net_are_measured_once` in
    /// `tests/routing.rs`.
    /// A cell fed by two *distinct* nets is charged for every buffer
    /// standing on both of them.
    ///
    /// `phase4_buffer_tick_invariant_holds` generates every cell from
    /// one net, so on its cases a sum over the driving nets and a max
    /// over them are the same number and the proptest cannot tell
    /// them apart. This is the shape where they differ, and it is the
    /// shape the stage-3 / stage-4 identity is actually load-bearing
    /// on: `sig.a` reaches the comparator over 42 blocks of dust and
    /// two buffers, the upstream cell over 20 blocks and one, so the
    /// figure is `base + 3` where a max would write `base + 2` and
    /// leave the block on the shorter segment standing under no tick
    /// at all.
    ///
    /// Threaded through routing → delay → crossing rather than handed
    /// a `Delayed` fixture, for the reason
    /// `ports_sharing_a_net_share_the_repeater_the_charge_and_the_dust`
    /// is: the tick figure and the blocks it is checked against are
    /// written by two different passes, and a fixture that skipped
    /// one would check a pass against itself. `cells[0]` drives the
    /// comparator without being driven itself, which is what makes
    /// its segment short while `sig.a` runs the width of the region.
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
        // The counts, not just the total: equal counts would make the
        // sum and the max differ by an amount this fixture could not
        // attribute to either net.
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
        // And the figure a max would have written, so the test fails
        // rather than narrows if the fold ever changes.
        let widest = u32::try_from(per_port(PortName::A).max(per_port(PortName::B)))
            .expect("two buffers fit in u32");
        assert_ne!(
            ticks - base,
            widest * BUFFER_REPEATER_TICKS,
            "a max over the driving nets would drop the shorter segment's block",
        );
    }

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

    /// The wire out to an actuator records a repeater a *cell*
    /// segment placed.
    ///
    /// One placer walks the cells and then the actuator pads, so this
    /// is the only direction the two segment kinds can meet: the cell
    /// takes the plane coord, and the pad's route reaches it 15 steps
    /// along as well. The reverse cannot happen, because no cell is
    /// allocated after an output.
    ///
    /// The pad's route is longer than the cell's by more than the 13
    /// blocks between them, because the cell is a block on the row and
    /// the wire out now steps around it — so the pad's segment asks for
    /// a second repeater of its own past the shared one.
    ///
    /// The route forks at `(16,0,0)`, into the cell and round it, so the
    /// shared repeater stands on the trunk one coord before the fork.
    /// The second stands at `(29,0,1)`, the last straight coord before
    /// the wire turns at `(30,0,1)` into the pad at `(30,0,0)`.
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

    /// Every port spelling reaches the push site and the wire form.
    ///
    /// A regression that hard-coded `PortName::A` would slip past
    /// every other buffer test, which drives a single `A`. The three
    /// drivers are on three *different* nets, so the three segments
    /// are three routes rather than one shared prefix, and each port
    /// has to carry its own coord out.
    ///
    /// The four nets contend for the rows across the region. Their pads
    /// stand at `z=0`, `2`, `3` and `4`, the column stepping over the
    /// cell row. `sig.sel` is laid first — same fanout, lowest
    /// [`crate::routing_geometry::net_ref_key`] — and runs out along
    /// `z=0`; `sig.blocker` takes the row at `z=2`, and `sig.port_a`,
    /// whose pad stands beside that row, has no row left and climbs at
    /// its own pad; `sig.port_b` runs out along `z=4`. Every repeater
    /// still stands on its own port's route, and the one on
    /// [`RouteLayer::Bridge`] is that rule holding through the escape —
    /// `CellCoord::new` decides the layer from the height, so the
    /// comparison below carries it. `sig.sel` turns off `z=0` at
    /// `(14,0,0)` and back east at `(14,0,1)`, and its repeater falls
    /// due 16 steps along, on `(15,0,1)`, where the wire runs straight
    /// again past both turns.
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
        // Chaining `compile_crossing(&legalized.scoped)` is forbidden
        // by the producer↔variant table on `PlacementPhase`. Loud in
        // release so
        // a caller cannot silently double-populate `buffer_coords`.
        // The expected substring pins the cell identity as well as the
        // invariant: the breadcrumb is what tells an operator which
        // cell tripped the guard without walking the backtrace back
        // into the IR.
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
        ));
        let first = compile_crossing(&scoped(ScopeKind::Struct, "twice", ir));
        let _second = compile_crossing(&first.scoped);
    }

    #[test]
    fn missing_region_with_cells_fires_no_circuit_region() {
        // Hand-built `PlacementIr` with cells but no region reaches
        // crossing because it skipped placement. Mirrors the delay
        // pass's hardening.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(0, 0, 0),
            vec![],
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
        // No region + no cells + no outputs = harmless empty scope
        // that the pass returns verbatim. Prior stages will not
        // produce this shape, but a hand-built input reaching here
        // must not trip the `NoCircuitRegion` refusal — that guard
        // only fires when the scope carries cells or outputs.
        let ir = PlacementIr::new(Edition::Java);
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "harmless", ir));
        assert!(legalized.diagnostics.is_empty());
        assert_eq!(legalized.scoped.scopes.len(), 1);
        assert!(legalized.scoped.scopes[0].ir.region.is_none());
    }

    #[test]
    fn output_only_scope_with_missing_region_refuses() {
        // Outputs but no cells with no region: still a phase-table
        // violation because `PlacementIr::is_empty` returns false as
        // long as outputs exist. Should refuse with `NoCircuitRegion`
        // for parity with the delay pass.
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

    /// Stage 4 refuses a stretch no repeater can stand on as stage 3
    /// does, on the tree stage 2 laid.
    ///
    /// Stage 3 refuses the scope, so nothing hands stage 4 a delayed
    /// staircase; the routed one is promoted by hand, which is the IR
    /// `--stage crossing` would be given without stage 3 having run.
    /// The two refusals agree word for word but for the netlist they
    /// name.
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

    /// A route that turns at the 16-step point takes a second repeater
    /// its length alone does not ask for, and stage 3 charges for it.
    ///
    /// 32 steps from `(0,0,0)` to `(16,0,16)`, east then south. The
    /// first repeater falls due 16 steps along, on the corner, so it
    /// stands one short of it; the 16 blocks of dust it leaves ahead
    /// are one too many, and the second stands on `(16,0,15)`, the last
    /// coord before the sink. `buffer_count_for_segment(32)` is 1, so a
    /// delay pass that still counted from the length would charge one
    /// tick for two blocks.
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
        // A routed driver segment of 49 steps trips the attenuation
        // limit three times, so three buffer coords land. The delay
        // pass has a mirrored boundary-row test on the tick side
        // (`s → buffers`); this is its mirror on the coord side.
        //
        // The route `(0, 0, 0) → (48, 0, 1)` walks x++ 48 steps then
        // z++ 1 step. The first two repeaters stand on the straight
        // run at `path[16]` and `path[32]`. `path[48]` is `(48, 0, 0)`,
        // where the wire turns towards the sink: a repeater there
        // would drive into `(49, 0, 0)`, which is not the wire, so the
        // third stands on `(47, 0, 0)`, the last straight coord before
        // the turn. No collision → every buffer stays on plane.
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
        // Companion to `buffer_coord_index_at_kth_boundary`, at the far
        // end of the range the delay pass's `MAX_ATTENUATION_SEGMENT =
        // 256` sanity cap permits. A segment of exactly 256 steps
        // yields 15 buffers, matching the tick-side boundary table row
        // for that segment, all at `k * 16` on the straight run: the
        // turn at `path[255]` is not one of those, so nothing steps
        // back.
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
        // Mirrors `delay.rs`'s equivalent guard: a hand-built IR with
        // `NetRef::Cell(u32::MAX)` violates the synthesis-side
        // topological invariant, and the crossing pass panics loud so
        // a caller-side bug cannot produce silently wrong
        // `buffer_coords`.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 3, 2));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(0, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(u32::MAX),
            }],
        ));
        let _ = compile_crossing(&scoped(ScopeKind::Struct, "broken", ir));
    }

    #[test]
    #[should_panic(
        expected = "for cell #1 at (4,0,1) in struct `mixed` — crossing legalization must run exactly once per delayed IR"
    )]
    fn legalize_panic_names_the_offending_cell_not_the_first_one() {
        // Re-running the whole pass always trips on `cells[0]`, which
        // would let a regression that hardcoded the index to zero — or
        // that read the coord off the wrong cell — pass unnoticed. A
        // hand-built IR whose first cell is still `Delayed` while the
        // second is already `Legalized` forces the panic past the head
        // of the loop, so both the index and the coord have to be
        // threaded from the cell actually being transitioned.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(8, 3, 2));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(0, 0, 0),
            vec![],
        ));
        let mut already_legalized =
            placed_cell(EditionCell::JavaRepeaterOr, CellCoord::new(4, 0, 1), vec![]);
        already_legalized.phase = PlacementPhase::Legalized {
            wire_length: 0,
            local_delay_ticks: 0,
            buffer_coords: Vec::new(),
        };
        ir.cells.push(already_legalized);
        let _ = compile_crossing(&scoped(ScopeKind::Struct, "mixed", ir));
    }

    mod strand_invariant {
        //! The attenuation limit holds along whole strands, across the
        //! cells that pass strength on — checked by a walk written out
        //! here rather than through the placer's own budgets, so a
        //! placer that stopped carrying the dust across a cell fails.
        //!
        //! "Strand" here is the run of dust counted from the last block
        //! that restored strength, which carries on through a cell that
        //! passes strength on. It is not the physically contiguous dust
        //! the rest of this module means when it keeps two signals off
        //! one strand: a cell's body stands in the middle of this one.

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

        /// Walks every net in topological order, carrying the dust spent
        /// since the last block that restores strength — a sensor pad, a
        /// repeater, or a cell whose output is at full strength — across
        /// the cells that pass it on, and asserts that no block of dust
        /// is past the limit, and no sink or repeater is reached over
        /// more than the limit's worth of dust. Returns how many buffer
        /// repeaters it met, so a caller can tell the walk saw some.
        ///
        /// What it catches: it reads the repeaters out of the IR and
        /// never calls the placer, so a repeater missing where the dust
        /// runs past the limit fails it, and so does a placer that stops
        /// carrying the count across a cell. What it does not:
        ///
        /// - It tells dust from a sink or a repeater by the same rule as
        ///   the placer: a coord the tree runs on past, unless a repeater
        ///   stands there. So it holds the placer to the placer's own
        ///   idea of which coords are dust, and were that idea off by one
        ///   — as counting the sink as a block of dust once was — the two
        ///   would agree and it would not notice.
        /// - It is one-sided. It asserts that no strand runs too far, not
        ///   that every repeater is needed, so a placer that stood a
        ///   repeater on every coord that could hold one would pass it.
        ///   [`a_strand_through_a_dust_merge_steps_back_off_a_turn_once`]
        ///   and [`a_strand_through_a_comparator_steps_back_off_a_fork_once`]
        ///   pin that side, by exact equality, on a turn and on a fork.
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

        /// The upper bound the strand walk does not pin, on a turn. A
        /// Bedrock OR, a dust merge, stands 6 steps from the sensor and
        /// drives a pad at `(16, 0, 2)`, which its wire reaches by running
        /// east to `(16, 0, 0)` and turning south. The strand runs on
        /// through the merge, its 6th block of dust, so its 16th is the
        /// corner, and the repeater steps back to `(15, 0, 0)`. That is
        /// the only one in the scope: the strand walk passes this, and
        /// would pass it as well with a repeater on every straight coord;
        /// the count and the exact lists are what would not.
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
        //! Property tests for the crossing / delay agreement invariant
        //! (see the `phase4_buffer_tick_invariant_holds` doc). Kept
        //! inside the crossing pass's own crate-internal test module so
        //! the strategy can hand-build `Unrouted` [`PlacedCellNode`]s
        //! (the `PlacementPhase` variants and the `pub(crate)`
        //! `phase` field are not reachable from `tests/crossing.rs`).
        use proptest::prelude::*;

        use super::{
            BufferSegment, CellCoord, CellPortDriver, Edition, EditionCell, HashMap, HashSet,
            NetRef, PadColumn, PlacedCellNode, PlacementIr, PlacementPhase, PortName, RouteLayer,
            Router, ScopeKind, ScopedPlacementIr, Span, block_sites, collect_nets,
            compile_crossing, input_pad, net_trees, reservation, scoped,
        };
        use crate::delay::{BUFFER_REPEATER_TICKS, compile_delay};
        use crate::placement_ir::ScopedPlacementIrEntry;
        use crate::routing::compile_routing;
        use crate::routing_geometry::NetTree;

        /// Strategy over sink positions for the phase-4 invariant
        /// property test. Each `(x, z)` in the returned `Vec` seeds one
        /// cell at `(x, 0, z)` driven from `Input(0)` at `(0, 0, 0)`.
        /// `x` in `1..=99` covers both the sub-limit segments (zero
        /// buffers) and the multi-boundary ones, so `buffer_total` is
        /// non-zero on most cases and the invariant discriminates.
        ///
        /// `z` varies rather than sitting at 0. A row of collinear
        /// sinks is the one shape where every strand runs along a
        /// single axis, which is where a route and the straight line
        /// between its ends coincide — and coinciding is what hid the
        /// bug this suite missed. Off-axis terminals are what let the
        /// two differ.
        ///
        /// The `bool` gives a cell a second port on the *same* net.
        /// Without it every cell has one driver, and the block count
        /// and the attribution count coincide for the whole strategy
        /// — which is how the invariant below could be stated in
        /// attributions and still hold.
        ///
        /// Duplicate `(x, z)` pairs are dropped, because two cells on
        /// one coord is not a scope the placement pass can emit — it
        /// derives a cell's column from its topological index — and
        /// `collapsed_block` asserts on one. Before that assert
        /// existed a repeated pair generated a degenerate scope, two
        /// blocks on one voxel with a zero-length net between them,
        /// and the invariant held over it without meaning much. The
        /// first of each pair is kept, so at least one cell survives.
        ///
        /// The dedup is a `prop_map` over the same tuple rather than a
        /// constrained strategy on purpose: changing the tuple
        /// invalidates every persisted seed in
        /// `proptest-regressions/crossing.txt`, because a seed is an
        /// RNG state and replays as an unrelated value under a
        /// different shape rather than failing to load. Mapping the
        /// generated value leaves the draw identical, so the seeds
        /// still replay the cases they were recorded for. Anything a
        /// retired seed was holding has to be written as a named test
        /// before the shape itself changes.
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

        /// What `phase4_buffer_tick_invariant_holds` asserts, over one
        /// draw of [`phase4_scope_strategy`]. A function of its own so
        /// a named test can replay a draw that once failed without
        /// leaning on the regressions file to keep it.
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
        fn legalize_phase4(
            ir: PlacementIr,
            case: &str,
        ) -> Result<ScopedPlacementIr, TestCaseError> {
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
            // A refused scope is elided, and an elided scope
            // carries no buffers to check. Off-axis terminals make
            // two sinks share a route prefix often enough that
            // `void=3` runs out of bridge layers, and refusing is
            // the documented answer there — so the case is
            // rejected rather than failed.
            prop_assume!(legalized.diagnostics.is_empty());
            Ok(legalized.scoped)
        }

        /// Where the buffers landed, not just how many. Totals alone
        /// let a coord move anywhere as long as the count holds, which
        /// is exactly the shape of the bug this suite missed.
        fn check_buffer_sites(
            entry: &ScopedPlacementIrEntry,
            case: &str,
        ) -> Result<(), TestCaseError> {
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
            // And on *this* segment's route, not just somewhere on
            // the net. The two differ for an entry the placer took
            // from a memo rather than from the route it was walking:
            // `wire_path` is every coord of the net and would accept a
            // coord that refreshes a sibling sink instead of this
            // one.
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

        /// The buffer touches no block of its net but the two it
        /// joins: no dust, and no cell body or pad of the net either.
        /// A repeater severs every face but its front and back, so a
        /// block of the same net on any other face would be cut off
        /// even where the tree does not link the two — which is why
        /// this is asked of the grid, not of the tree the placer
        /// walked. `net` is every coord of the net, terminals
        /// included.
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
            let joined: HashSet<CellCoord> =
                before.into_iter().chain(after.iter().copied()).collect();
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
        fn check_cell_charges(
            entry: &ScopedPlacementIrEntry,
            case: &str,
        ) -> Result<(), TestCaseError> {
            for cell in &entry.ir.cells {
                let dt = cell
                    .local_delay_ticks()
                    .expect("legalized cells carry Some(local_delay_ticks)");
                let base = cell.cell.base_delay_ticks();
                // `checked_sub` here as well as in the scope total: a
                // cell that regressed below its edition base would
                // clamp to zero and match a cell with no buffers,
                // which is the failure the per-cell check exists to
                // name.
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
                let blocks: HashSet<CellCoord> =
                    cell.buffer_coords().iter().map(|b| b.coord).collect();
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
        fn check_scope_total(
            entry: &ScopedPlacementIrEntry,
            case: &str,
        ) -> Result<(), TestCaseError> {
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

        /// Replays a pinned shape. A rejected case has stopped reaching
        /// the assertions at all, which is not the invariant breaking,
        /// so it says so rather than reporting a failure.
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

            /// Crossing / delay agreement invariant: on every emitted
            /// scope, `Σ (blocks under cell.buffer_coords()) ×
            /// BUFFER_REPEATER_TICKS` must equal `Σ (cell.local_delay_ticks()
            /// − cell.cell.base_delay_ticks())`, where the blocks under
            /// a cell are its buffer coords deduplicated: the vector
            /// attributes an entry per driver segment, and segments of
            /// one net share the repeater on their prefix. The sum is
            /// over cells rather than over the scope's blocks, because
            /// what it adds up is the per-cell wire cost — a repeater
            /// two cells hang off is charged to both of them. A drift in either
            /// pass's repeater count, in `BUFFER_REPEATER_TICKS`, or in the
            /// per-edition base-delay table trips this shared
            /// assertion rather than each pass's own boundary rows
            /// in isolation.
            ///
            /// Every cell this strategy builds is driven by one net
            /// (`Input(0)`, sometimes on both ports), so a sum over
            /// the driving nets and a max over them agree on every
            /// case it generates and this proptest cannot tell them
            /// apart — changing the fold to a max leaves it green.
            /// `two_distinct_nets_charge_a_cell_for_every_buffer_under_it`
            /// covers the multi-net shape, which is where the two
            /// part company and where this identity is what keeps
            /// stage 3 a sum.
            ///
            /// Uses hand-built `Unrouted` cells threaded through
            /// `compile_routing → compile_delay → compile_crossing`.
            /// `checked_sub` / `checked_mul` refuse to silently clamp
            /// on the failure direction the invariant is meant to
            /// catch (a delay pass regressing below the edition
            /// `base_delay_ticks`, or a buffer count overflowing
            /// `u32`).
            #[test]
            fn phase4_buffer_tick_invariant_holds(xs in phase4_scope_strategy()) {
                check_phase4_scope(&xs)?;
            }
        }
    }
}
