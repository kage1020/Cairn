//! Plumbing shared by the routing, delay, and crossing passes: opening a
//! scope, laying its nets, measuring each node against the routed trees,
//! and folding the per-scope results into a pass output.
//!
//! The three passes rebuild the same trees from the same IR, so the
//! steps that decide what those trees are live here once. What each
//! pass writes into the phase, and what it refuses over, stays with the
//! pass.

use std::collections::HashMap;

use crate::delay::MAX_ATTENUATION_SEGMENT;
use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CellIdentity, CircuitRegionReservation, PlacedCellNode, PlacedOutputNode,
    PlacementIr, PlacementPhase, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{
    BlockSite, NetTree, Router, block_sites, collapsed_block, collect_nets, input_pad, manhattan,
    net_trees, unroutable,
};
use crate::saturating_index;

/// Why [`open_scope`] found nothing to lay out.
pub(crate) enum Skipped {
    /// Neither cells nor actuator pads, so the pass hands the scope back
    /// verbatim. `ScopedPlacementIr::push` elides these on the input
    /// side; this covers a hand-built IR.
    Empty,
    /// Cells or pads but no reservation. The placement pass fires
    /// `E_NO_CIRCUIT_REGION` and elides such a scope before it can reach
    /// a later pass, so this is a caller-side hand-built IR.
    MissingRegion,
}

/// Refuse a scope that reached `stage` carrying cells or output drivers
/// but no `circuit region=` reservation. Reuses `E_NO_CIRCUIT_REGION`
/// because the failure mode is the one the placement pass catches, so a
/// reader matching on the code sees one taxonomy across stages. The span
/// anchors on the first cell, falling back to a default span when the
/// scope carries only outputs.
pub(crate) fn missing_region_diagnostic(
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    stage: &str,
) -> Diagnostic {
    let span = entry
        .ir
        .cells
        .first()
        .map(|c| c.span.clone())
        .unwrap_or_default();
    let primary = format!(
        "{netlist} netlist for {kind} `{name}` reached {stage} carrying cells or output drivers but no `circuit region=<label> void=<N>` reservation — the placement pass should have elided this scope",
        kind = entry.kind.label(),
        name = entry.name,
    );
    error_with_footer(
        DiagnosticCode::NoCircuitRegion,
        span,
        primary,
        "Fix: add a `circuit region=<label> void=<N>` line to the enclosing scope, or run `--stage placement` first to see the underlying error",
    )
}

/// One scope a pass is working on: the IR it will write into, cloned
/// from the entry, and the geometry every pass measures against.
pub(crate) struct OpenScope {
    pub(crate) ir: PlacementIr,
    pub(crate) region: CircuitRegionReservation,
    /// Snapshot of every cell's coord, so `ir.cells` can be borrowed
    /// mutably while a `NetRef → source coord` closure reads these.
    pub(crate) cell_coords: Vec<CellCoord>,
    /// `ir.inputs.len()`, for the same reason and read by the same
    /// closure: the bound `NetRef::Input(i)` is checked against.
    pub(crate) inputs: usize,
    /// Every block standing in the reservation, per [`block_sites`].
    pub(crate) blocks: Vec<BlockSite>,
}

pub(crate) fn open_scope(entry: &ScopedPlacementIrEntry) -> Result<OpenScope, Skipped> {
    let source = &entry.ir;
    if source.cells.is_empty() && source.outputs.is_empty() {
        return Err(Skipped::Empty);
    }
    let Some(region) = source.region.clone() else {
        return Err(Skipped::MissingRegion);
    };
    let ir = source.clone();
    let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();
    let inputs = ir.inputs.len();
    let blocks = block_sites(&ir, &region);
    Ok(OpenScope {
        ir,
        region,
        cell_coords,
        inputs,
        blocks,
    })
}

/// `NetRef → source coord`: the input pad for a sensor, the cell body
/// for a gate.
///
/// Both arms are in range for any IR a prior stage built. `Cell(j)` is
/// in range because the topological invariant carried across every
/// stage (`j < i` inside `cells[i]`) implies it, and `Input(i)` by
/// `inputs` being copied verbatim from the netlist the nets were
/// collected out of.
///
/// What is checked here is the weaker half: that the index names some
/// cell, not that it names an *earlier* one. A closure handed a bare
/// `NetRef` has no reading cell to compare against, and the same
/// closure answers for output drivers, which have no index at all. So
/// a forward reference or a cycle (`i <= j < len`) is answered rather
/// than refused, and the ordering half stays where it is built: the
/// `debug_assert!`s in [`crate::netlist`]. A caller assembling the IR
/// by hand can still get a wrong-but-in-range coord out of this.
///
/// # Panics
///
/// Panics on either index out of range, which is a caller-side
/// hand-built IR and nothing an author can edit. The alternative is a
/// source coord the pass then lays a wire from: an out-of-range `Cell`
/// saturated onto another cell roots the net in the wrong corridor, and
/// an out-of-range `Input` answers with a pad coord that stands in no
/// block [`block_sites`] emitted. Both are a layout, not a number, so
/// they reach the author as a plausible circuit rather than as a bug.
pub(crate) fn source_of_net<'a>(
    region: &'a CircuitRegionReservation,
    cell_coords: &'a [CellCoord],
    inputs: usize,
) -> impl Fn(NetRef) -> CellCoord + Copy + 'a {
    move |net| {
        match net {
        NetRef::Input(i) => {
            assert!(
                (i as usize) < inputs,
                "NetRef::Input({i}) out of range (inputs.len()={inputs}) — netlist invariant broken by caller-side hand-built IR",
            );
            input_pad(i as usize, region)
        }
        NetRef::Cell(j) => *cell_coords.get(j as usize).unwrap_or_else(|| {
            panic!(
                "NetRef::Cell({j}) out of range (cells.len()={}) — topological invariant broken by caller-side hand-built IR",
                cell_coords.len(),
            )
        }),
    }
    }
}

/// The router over this scope, every net and its routed tree, or the
/// first refusal the reservation earns.
///
/// Three refusals under two codes. `E_ROUTE_CONGESTION` for a
/// reservation too shallow to hold its pad row and for a sink no route
/// reaches; `E_ATTENUATION_LIMIT` for a sink further from its driver
/// than [`MAX_ATTENUATION_SEGMENT`] in a straight line.
///
/// Only one comes out, so the order is a decision rather than a race:
///
/// 1. the collapsed pad row, because it is the cause the other two are
///    symptoms of — a pad landing on a cell body strands sinks, and no
///    distance the author changes makes it fit;
/// 2. the over-cap sink, because its repair is the *opposite* of the
///    stranded sink's. `E_ROUTE_CONGESTION` says to enlarge the
///    reservation; a pair already too far apart in a straight line is
///    not helped by room, and for the shape that raises this most often
///    — a reservation as wide as the `size=` it came from — enlarging
///    makes it worse. A scope carrying both faults gets the refusal
///    whose fix line is true rather than the one that sends the author
///    the wrong way;
/// 3. the sink no route reaches.
///
/// The pad row and the stranded sink are asked by all three passes for
/// one reason. Stage 2 elides the scope that earns either, and the CLI
/// always runs it, so through `cairn` stages 3 and 4 never see one.
/// What reaches them is a caller that assembled the IR itself: an
/// in-crate test, or a library consumer calling [`crate::compile_delay`]
/// or [`crate::compile_crossing`] directly, both of which the crate root
/// re-exports. Neither failure announces itself downstream.
/// A stranded sink's route is one step: under every cap, worth no
/// repeater, and indistinguishable in the dump from a circuit that
/// works. A collapsed pad makes one of the two blocks something the
/// router routes *to* and the other a coord it routes *through*, and
/// the dump that comes out reads as a layout.
///
/// The over-cap sink is shared for a different reason, and the argument
/// above does not carry to it: it is the loudest thing downstream, the
/// very refusal the delay pass exists to make. What asking here saves is
/// not silence but time — the route would otherwise be laid coord by
/// coord before the pass that measures it could say no.
///
/// The router is built here rather than by each caller, so the refusals
/// cannot be reached around: a pass holding a [`Router`] is a pass that
/// has been through both.
///
/// # The `netlist` noun
///
/// All three refusals open with `<netlist> netlist for ...`, and
/// `netlist` is a parameter — `placed`, `routed`, `delayed` — because
/// all three passes arrive here and a constant would name the wrong one
/// on two of the three paths, the common one (`--stage route` over any
/// `.crn`) among them.
///
/// It names the netlist the message is *about*, which for everything
/// raised here is the netlist that was handed in: these are the ways a
/// netlist cannot be wired, so the fault is in what arrived, not in
/// what came out. An advisory about the layout the router produced
/// names that one instead — [`crate::routing_geometry::tile_layer_clearance`]
/// says `routed netlist` from the same pass that says `placed netlist`
/// here, and correctly: a placed netlist has no dust to leave pairs of.
pub(crate) fn lay_nets<F>(
    ir: &PlacementIr,
    blocks: &[BlockSite],
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    region: &CircuitRegionReservation,
    source_of_net: F,
) -> Result<Nets, Diagnostic>
where
    F: Fn(NetRef) -> CellCoord + Copy,
{
    if let Some(site) = collapsed_block(blocks) {
        return Err(pad_overlap_diagnostic(entry, netlist, region, site));
    }
    if let Some(diagnostic) = beyond_attenuation(ir, entry, netlist, region, source_of_net) {
        return Err(diagnostic);
    }
    let router = Router::new(region, blocks);
    let sinks = collect_nets(ir);
    let trees = net_trees(&sinks, &router, source_of_net);
    if let Some(diagnostic) = unroutable(&sinks, &trees, entry, netlist, region, source_of_net) {
        return Err(diagnostic);
    }
    Ok(Nets {
        router,
        sinks,
        trees,
    })
}

/// Refuse a sink the attenuation cap already puts out of reach, before
/// a route to it is laid.
///
/// [`manhattan`] is a floor on every route between two coords — it is
/// the router's own search heuristic, admissible for exactly that
/// reason — so a pair further apart than [`MAX_ATTENUATION_SEGMENT`]
/// has no route the delay pass would accept. Laying one first costs the
/// search a coord of work per block of distance, and a `region=` taken
/// from a `size=` makes that millions: a four-million-wide floor spent
/// over a minute laying a wire out to its pad, in each of the three
/// passes that lay nets, before stage 3 measured it and said no.
///
/// A floor and not the measure, so this refuses strictly less than
/// [`crate::delay`] does and replaces nothing: a route is as long as the
/// straight line only where nothing stands in the way. A region 256
/// wide puts its pad 255 blocks from the driver and routes 257 to get
/// there, which is over the cap and not over this, and stage 3's check
/// is what catches it.
///
/// Walked as [`crate::delay`] walks it — every cell's drivers in index
/// order, then every actuator pad — so that where both would refuse,
/// they refuse in the same order and the earlier answer reads as the
/// later one arriving sooner rather than as a second finding about one
/// reservation.
///
/// Sharing the order is not the same as naming the same sink, because
/// the two measure different quantities. A scope holding a sink over
/// the cap in a straight line and a *different* sink over it only once
/// routed is refused here, naming the first; had this gate not run, the
/// delay pass would have named whichever of the two its own walk
/// reached first. The order makes them agree on the common case, not on
/// every case.
fn beyond_attenuation<F>(
    ir: &PlacementIr,
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    region: &CircuitRegionReservation,
    source_of_net: F,
) -> Option<Diagnostic>
where
    F: Fn(NetRef) -> CellCoord,
{
    for (cell_index, cell) in ir.cells.iter().enumerate() {
        for (driver_index, driver) in cell.drivers.iter().enumerate() {
            let straight = manhattan(source_of_net(driver.net), cell.coord);
            if straight > MAX_ATTENUATION_SEGMENT {
                return Some(unreachable_sink_diagnostic(
                    entry,
                    netlist,
                    region,
                    &format!("cell #{cell_index} port #{driver_index}"),
                    straight,
                ));
            }
        }
    }
    for (output_index, output) in ir.outputs.iter().enumerate() {
        let straight = manhattan(source_of_net(output.driver), output.pad);
        if straight > MAX_ATTENUATION_SEGMENT {
            return Some(unreachable_sink_diagnostic(
                entry,
                netlist,
                region,
                &format!("output pad #{output_index}"),
                straight,
            ));
        }
    }
    None
}

/// The refusal [`beyond_attenuation`] returns, under the cap's own code
/// because it is the cap: what changes is which pass has to lay a wire
/// to find out.
///
/// The fix line is not [`crate::delay`]'s. That one opens with
/// "enlarge `region=`", which is the repair when a route is long
/// because it had to go round something — room to go straight shortens
/// it. Nothing shortens a straight line, so a larger reservation cannot
/// answer this one, and for the shape that raises it most often — a
/// `region=` as wide as the `size=` it was taken from, with the
/// actuator column at `x = width - 1` — enlarging is the direction that
/// makes it worse.
fn unreachable_sink_diagnostic(
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    reservation: &CircuitRegionReservation,
    sink: &str,
    straight: u32,
) -> Diagnostic {
    let primary = format!(
        "{netlist} netlist for {kind} `{name}` puts {sink} {straight} blocks from its driver in a straight line — exceeds the v1 attenuation limit of {cap} blocks, and no route between two coords is shorter than the straight line between them",
        kind = entry.kind.label(),
        name = entry.name,
        cap = MAX_ATTENUATION_SEGMENT,
    );
    error_with_footer(
        DiagnosticCode::AttenuationLimit,
        reservation.span.clone(),
        primary,
        "Fix: split the logic across several `circuit` blocks, or reserve a `region=` whose pad column sits within the cap of the cells it serves — a larger reservation cannot help, because the straight line between these two is already over the cap",
    )
}

/// Refuse a scope whose reservation collapses two blocks onto one
/// coord, naming the block that landed second and the reservation that
/// could not hold it.
///
/// `E_ROUTE_CONGESTION`, because the cause is the reserved area: the
/// pad row wants a row per sensor or actuator and has fewer.
///
/// `depth >= max(inputs, outputs)`, not `+ 1`. `edge_pad` saturates at
/// `z = min(index, depth - 1)`, so N pads collide only once `N > depth`
/// — and the placement pass guards on exactly that number
/// (`pad_rows > reservation.depth`). Two stages under one code have to
/// hand the author the same arithmetic.
fn pad_overlap_diagnostic(
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    reservation: &CircuitRegionReservation,
    site: &BlockSite,
) -> Diagnostic {
    let primary = format!(
        "{netlist} netlist for {kind} `{name}` cannot fit its {pad_kind} pad #{pad_index} at ({x},{y},{z}) — the reserved area (void={void}, region {width}x{depth}) collapses I/O pads onto a cell coord or another pad",
        kind = entry.kind.label(),
        name = entry.name,
        pad_kind = site.kind.as_str(),
        pad_index = site.index,
        x = site.coord.x,
        y = site.coord.y,
        z = site.coord.z,
        void = reservation.void,
        width = reservation.width,
        depth = reservation.depth,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        "Fix: enlarge `size=WxH` so `depth >= max(inputs, outputs)` — one row per sensor or actuator — or split into multiple `circuit` blocks",
    )
}

/// The nets of one scope: the router they were laid with, driver →
/// sinks, and driver → routed tree.
pub(crate) struct Nets {
    /// Carried out so a caller that needs the obstacle set again — the
    /// routing pass, for its tile-layer advisory — does not rebuild one
    /// and get a second chance to build it differently.
    pub(crate) router: Router,
    pub(crate) sinks: HashMap<NetRef, Vec<CellCoord>>,
    pub(crate) trees: HashMap<NetRef, NetTree>,
}

impl Nets {
    /// The routed length from `net`'s source to `sink` — the measure
    /// stage 2 records as wire and stage 3 counts buffer repeaters
    /// against.
    ///
    /// `route_to` answers `None` only for a sink that is not a terminal
    /// of the net, which [`collect_nets`] rules out: it built the tree's
    /// terminal list from the same driver list.
    pub(crate) fn segment(&self, net: NetRef, sink: CellCoord) -> u32 {
        let route = self
            .trees
            .get(&net)
            .and_then(|tree| tree.route_to(sink))
            .unwrap_or_else(|| {
                panic!(
                    "sink ({x},{y},{z}) is not a terminal of the net driving it — the driver list and the collected nets disagree",
                    x = sink.x,
                    y = sink.y,
                    z = sink.z,
                )
            });
        saturating_index(route.len().saturating_sub(1))
    }
}

/// Measure every cell and every actuator pad, then commit each figure
/// through `commit` with the identity a phase-transition panic names.
///
/// Measured into a side vector first so `ir` is borrowed immutably while
/// the routes are read, then committed in a mutable pass. The commit is
/// loud in release too: the `*_at` transitions panic on a phase the pass
/// does not consume, which is what a caller who ran a pass twice hands
/// us.
pub(crate) fn attribute_nodes<T>(
    ir: &mut PlacementIr,
    entry: &ScopedPlacementIrEntry,
    measure_cell: impl Fn(&PlacedCellNode) -> T,
    measure_output: impl Fn(&PlacedOutputNode) -> T,
    commit: impl Fn(&mut PlacementPhase, T, CellIdentity<'_>),
) {
    let cell_values: Vec<T> = ir.cells.iter().map(measure_cell).collect();
    for (index, (cell, value)) in ir.cells.iter_mut().zip(cell_values).enumerate() {
        let identity = CellIdentity::new(index, cell.coord, entry);
        commit(&mut cell.phase, value, identity);
    }
    let output_values: Vec<T> = ir.outputs.iter().map(measure_output).collect();
    for (index, (output, value)) in ir.outputs.iter_mut().zip(output_values).enumerate() {
        let identity = CellIdentity::output(index, output.pad, entry);
        commit(&mut output.phase, value, identity);
    }
}

/// Run `lower` over every scope, keeping the IR of each one that
/// succeeds and collecting the findings in scope order. A refused scope
/// contributes its refusal and no IR, so a partial result never reaches
/// the next pass.
pub(crate) fn lower_scopes<F>(
    input: &ScopedPlacementIr,
    mut lower: F,
) -> (ScopedPlacementIr, Vec<Diagnostic>)
where
    F: FnMut(&ScopedPlacementIrEntry) -> Result<(PlacementIr, Vec<Diagnostic>), Diagnostic>,
{
    let mut scoped = ScopedPlacementIr::new();
    let mut diagnostics = Vec::new();
    for entry in &input.scopes {
        match lower(entry) {
            Ok((ir, advisories)) => {
                diagnostics.extend(advisories);
                scoped.scopes.push(ScopedPlacementIrEntry {
                    kind: entry.kind,
                    name: entry.name.clone(),
                    ir,
                });
            }
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    (scoped, diagnostics)
}

#[cfg(test)]
mod tests {
    //! The refusals this module owns, asked of every stage that calls
    //! [`lay_nets`].

    use cairn_lang_core::Edition;
    use cairn_lang_core::error::Span;

    use super::MAX_ATTENUATION_SEGMENT;
    use crate::diagnostic::DiagnosticCode;
    use crate::edition_netlist_ir::EditionCell;
    use crate::logic_ir::ScopeKind;
    use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, PortName};
    use crate::placement_ir::{
        CellCoord, PlacedCellNode, PlacementIr, PlacementPhase, ScopedPlacementIr,
    };
    use crate::test_fixtures::{
        CollapsedRow, DanglingNet, FarSink, collapsed_pad_row, dangling_net, far_sink,
        regionless_scope, reservation, scoped,
    };

    /// A reservation too shallow to hold its pad row is refused by
    /// stage 2, stage 3 and stage 4 alike, and the scope is elided
    /// rather than measured against a collapsed layout.
    ///
    /// What the refusal prevents is an IR whose actuator pad and cell
    /// body are one voxel carrying a `wire_length` and a tick count
    /// measured against itself. Nothing downstream says so: the
    /// numbers are small and plausible, and the dump reads as a
    /// layout. The stages that can be handed one without stage 2
    /// having run are stages 3 and 4, reached by a caller that
    /// assembled the IR itself — an in-crate test, or a library
    /// consumer calling `compile_delay` or `compile_crossing`, which
    /// the crate root re-exports.
    ///
    /// Written per stage rather than per pass because what is being
    /// held is that the three answer one geometry the same way; a case
    /// that asked only the stage under test could not fail on two of
    /// them at once. Both ends of the pad column are rowed, because
    /// they saturate onto different things — the actuator column onto
    /// the cell row, the sensor column onto its own previous pad — and
    /// the message names which. `input_pad` and `output_pad` are
    /// tested as pure functions in `routing_geometry`; what their
    /// saturation feeds into is this refusal.
    ///
    /// Hand-built, because the placement pass refuses a region this
    /// small one stage earlier and the CLI always runs it. That is not
    /// a reason to leave the shape untested: it is the shape a library
    /// consumer reaches, and until this change two of the three stages
    /// took it without a word.
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
                // The one word that is *meant* to differ between the
                // three. Asserted here, and stripped before the
                // cross-stage comparison below, so "one geometry, one
                // sentence" is checked on the sentence rather than
                // defeated by the noun.
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
                // The fix line and the span are what an author acts on, and
                // nothing else in the crate asserts either. The span is the
                // reservation's, because `size=` is the edit.
                let footer = refusal
                    .notes
                    .iter()
                    .find(|n| n.message.starts_with("Fix:"))
                    .unwrap_or_else(|| panic!("the {stage} refusal carries a fix line"));
                assert!(
                    footer.message.contains("depth >= max(inputs, outputs)")
                        && !footer.message.contains("+ 1"),
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

        // One geometry, one sentence. A stage that refused for the
        // right reason in different words would still leave an author
        // comparing two messages to see they are the same finding.
        for (row, primaries) in said {
            assert!(
                primaries.windows(2).all(|w| w[0] == w[1]),
                "{row:?}: the three stages word the refusal identically past the netlist \
                 noun, got {primaries:?}",
            );
        }
    }

    /// A scope that collapses its pad row *and* strands a sink is
    /// refused over the pad row.
    ///
    /// Both are `E_ROUTE_CONGESTION`, so the code tells the author
    /// nothing about which they have, and both have one cause and one
    /// edit: the reserved area is too small. The pad-row message says
    /// that and names the `size=` line. The stranded-sink message
    /// describes a sink the router gave up on, which is a symptom of
    /// the same shortage and sends the author looking at their cells.
    /// Only one comes out — the pass returns at the first — so which
    /// one is a decision, made here by asking the pad row before the
    /// trees are grown.
    ///
    /// The one-input row is the control: the same walls, the same
    /// boxed sinks, a pad column that fits. It has to keep reporting
    /// the stranded sink, or the rule above collapses into "always say
    /// congestion".
    #[test]
    fn a_scope_that_earns_both_refusals_is_refused_over_the_pad_row() {
        for (inputs, names, why) in [
            // `cannot reach (x,y,z)`, not `cannot be reached`: the
            // latter is only in the plural suffix, so a row keyed on
            // it would pass on the count of stranded sinks rather than
            // on the finding being the stranded-sink one.
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
                ir.cells.push(cell(coord, Vec::new()));
            }
            for coord in [CellCoord::new(2, 0, 0), CellCoord::new(3, 0, 0)] {
                ir.cells.push(cell(
                    coord,
                    vec![CellPortDriver {
                        port: PortName::A,
                        net: NetRef::Input(0),
                    }],
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
            // The premise of the paragraph above: one code for both, so
            // the message is all the author has to tell them apart.
            assert_eq!(
                routed.diagnostics[0].code,
                DiagnosticCode::RouteCongestion,
                "{inputs} input(s): both refusals are E_ROUTE_CONGESTION",
            );
        }
    }

    /// The three stages that lay nets, each with the phase it expects
    /// to be handed and a reader for what it produced.
    ///
    /// The shared shape of the cross-stage tests below: a scope's
    /// geometry does not vary with the phase, so one fixture can be put
    /// to all three, and a case that asked only one stage could not
    /// fail on two of them at once — which is the whole subject here,
    /// three passes that were answering one shape three ways.
    ///
    /// The second field is the noun that pass's refusals name — the
    /// netlist it read. It is carried here rather than derived from the
    /// stage name so a test can assert the noun as part of the message
    /// instead of stripping it off to compare the rest.
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

    /// A scope with cells and no reservation is `E_NO_CIRCUIT_REGION`
    /// at whichever stage meets it, and the scope is elided.
    ///
    /// The routing pass used to pass this through: a `debug_assert!`
    /// that the scope was empty, and in release a scope handed on
    /// carrying cells that were never routed. Nothing downstream said
    /// so — the delay pass met the same scope and refused it there, one
    /// stage later, naming its own stage. What each pass writes is
    /// promised by the producer↔variant table on `PlacementPhase` after
    /// its own stage, so the pass that cannot write it is the pass that
    /// has to say why.
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

    /// And a scope with no reservation *and* nothing to lay out is
    /// still handed back, at every stage.
    ///
    /// The guard above fires on cells or outputs, not on the absent
    /// region, so this is the case that says the refusal is keyed on
    /// what the scope carries rather than on what it lacks.
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

    /// A driver naming a net no list can answer panics at every stage,
    /// naming the variant, the index, and the length it ran past.
    ///
    /// Not a diagnostic: no edit to the source produces or repairs it,
    /// because no source produces it. It is a caller-side hand-built IR,
    /// and the crate's convention for one is to assert.
    ///
    /// Both variants, because they used to fail differently and one of
    /// them did not fail at all. `Cell(j)` saturated onto the last cell
    /// in the routing pass's release build, rooting the net in the wrong
    /// corridor and laying its dust down the wrong one; `Input(i)`
    /// answered for any index in all three, with a pad coord standing in
    /// no block `block_sites` emitted.
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

    /// A sink the cap already puts out of reach is refused before a
    /// route to it is laid, at every stage that lays one.
    ///
    /// The cap is on the routed segment, and the straight line between
    /// two coords is a floor on every route between them, so a sink
    /// further away than the cap has no route any stage would accept.
    /// Stage 3 used to be the one that said so, after stage 2 had laid
    /// the wire and stage 3 had laid it again: a `circuit region=`
    /// taken from a four-million-wide `size=` spent over a minute of a
    /// debug build doing that before answering.
    ///
    /// Asked of all three because the check sits in the routine all
    /// three call to lay their nets, and the reason it sits there is
    /// that all three pay the cost.
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
                        panic!(
                            "the {stage} pass must refuse a {which:?} past the cap: {diagnostics:?}"
                        )
                    });
                assert!(
                    refusal.primary.contains(which.label())
                        && refusal.primary.contains(&format!("{} blocks", width - 1))
                        && refusal
                            .primary
                            .contains(&format!("limit of {MAX_ATTENUATION_SEGMENT} blocks")),
                    "the {stage} pass names the sink, the distance and the cap: {}",
                    refusal.primary,
                );
                // Which netlist the refusing pass read. A constant noun
                // here would name the delay pass on the two paths that
                // are not it, and the common one — `--stage route` over
                // any `.crn` — is one of those two.
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

    /// And a sink exactly at the cap is routed rather than refused, at
    /// every stage.
    ///
    /// The boundary is the whole content of the check: one block closer
    /// and the wire is laid, measured, and accepted. A gate that
    /// refused here would be refusing layouts the language allows,
    /// which is the cost of putting a cheap test in front of an exact
    /// one.
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

    /// An unrouted cell at `coord`, driven by `drivers`.
    fn cell(coord: CellCoord, drivers: Vec<CellPortDriver>) -> PlacedCellNode {
        PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers,
            coord,
            phase: PlacementPhase::Unrouted,
            span: Span::default(),
        }
    }

    /// A scope carrying *both* an over-cap sink and a stranded one is
    /// answered by the cap, not by congestion.
    ///
    /// The two repairs point opposite ways: `E_ROUTE_CONGESTION` tells
    /// the author to enlarge the reservation, and enlarging is exactly
    /// what cannot fix a straight line already over the cap — for the
    /// shape that raises it most often, a `region=` as wide as the
    /// `size=` it came from, enlarging makes it worse. So the order in
    /// [`lay_nets`] is a decision, and this is the fixture that holds
    /// it: the same scope minus its far sink earns the other code, so
    /// both faults really are present and the precedence is what picks.
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

    /// The walk names a cell before an actuator pad, and names the
    /// cell and port it actually reached.
    ///
    /// Every other fixture here carries one cell or one output, so the
    /// index in `cell #N port #M` is always `#0` and "cells before
    /// outputs" is never put to the test: a walk that visited outputs
    /// first, or that reported the loop counter of the enclosing scope,
    /// would read identically. This scope carries two cells and an
    /// output pad, all three over the cap, so only one ordering and one
    /// pair of indices produces the sentence asserted below.
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
}
