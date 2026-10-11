//! Plumbing shared by the routing, delay, and crossing passes: opening a
//! scope, laying its nets, measuring each node against the routed trees,
//! and folding the per-scope results into a pass output.
//!
//! The three passes rebuild the same trees from the same IR, so the
//! steps that decide what those trees are live here once. What each
//! pass writes into the phase, and what it refuses over, stays with the
//! pass.

use std::collections::HashMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::grid::{MAX_ATTENUATION_SEGMENT, PadColumn, input_pad, manhattan};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CellIdentity, CircuitRegionReservation, PlacedCellNode, PlacedOutputNode,
    PlacementIr, PlacementPhase, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{
    BlockSite, NetTree, Router, block_sites, collapsed_block, collect_nets, net_trees, unroutable,
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
    /// How the scope's pads are laid, for the same reason: the closure
    /// derives each sensor's pad from it.
    pub(crate) column: PadColumn,
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
    let column = PadColumn::of(&ir);
    let blocks = block_sites(&ir, &region);
    Ok(OpenScope {
        ir,
        region,
        cell_coords,
        inputs,
        column,
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
    column: PadColumn,
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
            input_pad(i as usize, column, region)
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
/// Four refusals under two codes. `E_ROUTE_CONGESTION` for a
/// reservation too shallow to hold its pad row and for a sink the
/// router proves no route reaches; `E_ATTENUATION_LIMIT` for a sink
/// further from its driver than [`MAX_ATTENUATION_SEGMENT`] in a
/// straight line, and for one no route within that cap reaches, where
/// the router cannot prove it walled in.
///
/// Only one comes out, so the order is a decision rather than a race:
///
/// 1. the collapsed pad row, because it is the cause the other two are
///    symptoms of — a pad landing on a cell body strands sinks, and no
///    distance the author changes makes it fit;
/// 2. the sink over the cap in a straight line, because its repair is
///    the *opposite* of the stranded sink's. `E_ROUTE_CONGESTION` says
///    to enlarge the reservation; a pair already too far apart in a
///    straight line is not helped by room, and for the shape that
///    raises this most often — a reservation as wide as the `size=` it
///    came from — enlarging makes it worse. A scope carrying both faults
///    gets the refusal whose fix line is true rather than the one that
///    sends the author the wrong way;
/// 3. the stranded sink: walled in, or with no route within the cap.
///    The second shares the first's code with the straight-line gate,
///    but not its repair — its fix line asks for a shorter way round,
///    which room can give — so the argument in (2) is about the
///    straight-line gate alone, and the two shapes of a stranded sink
///    are one step, ordered among themselves by [`unroutable`].
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
/// All four refusals open with `<netlist> netlist for ...`, and
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
/// there, which is over the cap and not over this. The router's search,
/// bounded by the same cap, refuses such a route when the path it lays
/// from the net's wire is itself over the cap, and stage 3's check
/// catches the rest: a branch within the cap on a trunk that carries it
/// over.
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
        "{netlist} netlist for {kind} `{name}` puts {sink} {straight} steps from its driver in a straight line — exceeds the v1 attenuation limit of {cap} steps, and no route between two coords is shorter than the straight line between them",
        kind = entry.kind.label(),
        name = entry.name,
        cap = MAX_ATTENUATION_SEGMENT,
    );
    error_with_footer(
        DiagnosticCode::AttenuationLimit,
        reservation.span.clone(),
        primary,
        "Fix: split the logic across several scopes, each with its own `circuit` line, or reserve a `region=` whose pad column sits within the cap of the cells it serves — a larger reservation cannot help, because the straight line between these two is already over the cap",
    )
}

/// Refuse a scope whose reservation collapses two blocks onto one
/// coord, naming the block that landed second and the reservation that
/// could not hold it.
///
/// `E_ROUTE_CONGESTION`, because the cause is the reserved area. In a
/// layout the placement pass's coordinates describe — cells on the cell
/// row, pads laid by [`PadColumn`] — two blocks share a coord in one of
/// two ways:
///
/// - a pad column taller than the region. [`PadColumn::rows`] is the
///   depth it needs (`max(inputs, outputs) + 1` once an edge carries two
///   pads in a scope with cells, `max(inputs, outputs)` in one without),
///   and the pads saturate at `depth - 1` past it. The placement pass
///   refuses on exactly that number, so the two stages hand the author
///   the same arithmetic, [`PadColumn::depth_rule`];
/// - a region one column wide, in a scope with no cells and both
///   sensors and actuators. The sensor column is `x = 0` and the
///   actuator column `x = width - 1`, so at `width == 1` they are one
///   column and pad #0 of each stands at `(0,0,0)` at any depth. The
///   placement pass refuses this one too, with
///   `placement::pad_column_diagnostic`; a scope with cells cannot
///   reach it, because its row needs three columns or more.
///
/// The fix line names the width when the second applies and the depth
/// rule always. A hand-built IR can put a cell anywhere and collide in
/// ways neither names; it gets the same fix line, since a reservation
/// is still what an author can edit.
///
/// The width branch (`one_column` below) is unreachable through `cairn`:
/// the CLI always runs the placement pass first, and its
/// `pad_column_diagnostic` refuses a one-column region with pads on both
/// edges before any stage here sees one. It is kept as a defensive layer
/// for a library caller that hands a stage a Placement IR it built
/// itself, which that guard never saw. Neither check makes the other
/// redundant: the placement guard is what the CLI meets, and this one is
/// what a hand-built IR meets.
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
    let ir = &entry.ir;
    let rule = PadColumn::of(ir).depth_rule();
    let one_column = reservation.width < 2 && !ir.inputs.is_empty() && !ir.outputs.is_empty();
    let fix = if one_column {
        format!(
            "Fix: widen `size=WxH` to at least two columns — at one, the sensor and actuator pads share a column and collide at any depth — and keep {rule}, or split the logic across several scopes, each with its own `circuit` line"
        )
    } else {
        format!(
            "Fix: enlarge `size=WxH` so {rule}, or split the logic across several scopes, each with its own `circuit` line"
        )
    };
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        fix,
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

/// Measure every cell and every actuator pad, each told its index in
/// `ir.cells` or `ir.outputs` so a panic can name it, then commit each figure
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
    measure_cell: impl Fn(usize, &PlacedCellNode) -> T,
    measure_output: impl Fn(usize, &PlacedOutputNode) -> T,
    commit: impl Fn(&mut PlacementPhase, T, CellIdentity<'_>),
) {
    let cell_values: Vec<T> = ir
        .cells
        .iter()
        .enumerate()
        .map(|(index, cell)| measure_cell(index, cell))
        .collect();
    for (index, (cell, value)) in ir.cells.iter_mut().zip(cell_values).enumerate() {
        let identity = CellIdentity::new(index, cell.coord, entry);
        commit(&mut cell.phase, value, identity);
    }
    let output_values: Vec<T> = ir
        .outputs
        .iter()
        .enumerate()
        .map(|(index, output)| measure_output(index, output))
        .collect();
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
mod tests;
