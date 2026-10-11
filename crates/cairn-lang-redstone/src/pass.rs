//! Plumbing shared by the routing, delay and crossing passes: opening a scope, laying its nets,
//! measuring each node against the routed trees, and folding the per-scope results into a pass
//! output.
//!
//! The three passes rebuild the same trees from the same IR, so the steps that decide what those
//! trees are live here once. What each pass writes into the phase, and what it refuses over, stays
//! with the pass.

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

/// Refuses a scope that reached `stage` with cells or output drivers but no reservation, under
/// the placement pass's `E_NO_CIRCUIT_REGION` so one fault has one code across stages. Anchored on
/// the first cell, or a default span for a scope with only outputs.
fn missing_region_diagnostic(
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

/// One scope a pass is working on: the IR it writes into, cloned from the entry, the geometry
/// every pass measures against, and the nets laid over it.
pub(crate) struct OpenScope {
    pub(crate) ir: PlacementIr,
    pub(crate) region: CircuitRegionReservation,
    /// Every block standing in the reservation, per [`block_sites`].
    pub(crate) blocks: Vec<BlockSite>,
    /// What [`lay_nets`] laid over the blocks.
    pub(crate) nets: Nets,
}

/// Open `entry` for the pass that runs as `stage` over the `netlist`
/// netlist, and lay its nets.
///
/// `Ok(None)` for a scope with neither cells nor actuator pads, which the pass hands back verbatim;
/// `ScopedPlacementIr::push` drops these, so only a hand-built IR has one.
///
/// A scope with cells or pads but no reservation is refused rather than passed through, because
/// each pass writes a phase field that `PlacementPhase` promises after its stage. The placement
/// pass already refuses such a scope, so only a hand-built IR reaches this. Every other refusal is
/// [`lay_nets`]'.
pub(crate) fn open_scope(
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    stage: &str,
) -> Result<Option<OpenScope>, Diagnostic> {
    let source = &entry.ir;
    if source.cells.is_empty() && source.outputs.is_empty() {
        return Ok(None);
    }
    let Some(region) = source.region.clone() else {
        return Err(missing_region_diagnostic(entry, netlist, stage));
    };
    let ir = source.clone();
    let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();
    let blocks = block_sites(&ir, &region);
    let nets = lay_nets(
        &ir,
        &blocks,
        entry,
        netlist,
        &region,
        source_of_net(&region, PadColumn::of(&ir), &cell_coords, ir.inputs.len()),
    )?;
    Ok(Some(OpenScope {
        ir,
        region,
        blocks,
        nets,
    }))
}

/// `NetRef → source coord`: the input pad for a sensor, the cell body for a gate.
///
/// Any IR a prior stage built is in range: `Cell(j)` by the topological invariant (`j < i` inside
/// `cells[i]`), `Input(i)` because `inputs` is copied from the netlist the nets came from. Only
/// the range is checked, not that `j` names an *earlier* cell: the closure has no reading cell to
/// compare against, and it also answers for output drivers. The ordering half is the
/// `debug_assert!`s in [`crate::netlist`].
///
/// # Panics
///
/// Panics on either index out of range, which only a hand-built IR can cause. Clamping instead
/// would root the net somewhere plausible, so a bug would reach the author as a circuit.
fn source_of_net<'a>(
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

/// The router over this scope, every net and its routed tree, or the first refusal the
/// reservation earns.
///
/// Four refusals under two codes: `E_ROUTE_CONGESTION` for a reservation too shallow to hold its
/// pad row and for a sink the router proves walled in; `E_ATTENUATION_LIMIT` for a sink further
/// from its driver than [`MAX_ATTENUATION_SEGMENT`] in a straight line, and for one no route
/// within that cap reaches. Only one comes out, in this order:
///
/// 1. the collapsed pad row, the cause the others are symptoms of: a pad on a cell body strands
///    sinks, and no distance the author changes makes it fit;
/// 2. the sink over the cap in a straight line, because its repair is the *opposite* of a
///    stranded sink's. Room does not shorten a straight line, and for a reservation as wide as
///    the `size=` it came from, enlarging makes it worse, so a scope with both faults gets the
///    fix line that is true;
/// 3. the stranded sink, walled in or with no route within the cap, ordered by [`unroutable`].
///    The second shape shares the cap's code but not this repair: room can give it a shorter
///    way round.
///
/// All three passes ask the pad row and the stranded sink because only stage 2 refuses them
/// otherwise, and a caller of [`crate::compile_delay`] or [`crate::compile_crossing`] with its own
/// IR skips stage 2. Downstream, neither shows: a stranded sink's route is one step, and a
/// collapsed pad reads as a layout. The over-cap sink is the delay pass's own refusal; asking it
/// here only saves laying the route first.
///
/// The router is built here rather than by each caller, so a pass holding a [`Router`] has been
/// through every refusal.
///
/// # The `netlist` noun
///
/// Every refusal opens with `<netlist> netlist for ...`, and `netlist` (`placed`, `routed`,
/// `delayed`) is a parameter because all three passes arrive here. It names the netlist that was
/// handed in, since these are the ways it cannot be wired. An advisory about the layout the router
/// produced names that one instead, as
/// [`crate::routing_geometry::tile_layer_clearance`] says `routed netlist`.
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

/// Refuses a sink the attenuation cap already puts out of reach, before a route to it is laid.
///
/// [`manhattan`] is a floor on every route between two coords, so a pair further apart than
/// [`MAX_ATTENUATION_SEGMENT`] has no route the delay pass would accept. Laying one first costs a
/// coord of search per block of distance, which a `region=` taken from a `size=` makes millions.
///
/// A floor, not the measure, so this refuses strictly less than [`crate::delay`] does: a region
/// 256 wide puts its pad 255 blocks from the driver and routes 257 to get there. The router's
/// cap-bounded search and stage 3 catch the rest.
///
/// Walked in [`crate::delay`]'s order (every cell's drivers by index, then every actuator pad), so
/// where both refuse the same sink they name it the same way. They can still name different sinks,
/// since they measure different quantities.
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

/// The refusal [`beyond_attenuation`] returns, under the cap's own code.
///
/// The fix line is not [`crate::delay`]'s "enlarge `region=`": room shortens a route that goes
/// round something, but not a straight line, and for a `region=` as wide as its `size=`, with the
/// actuator column at `x = width - 1`, enlarging makes it worse.
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

/// Refuses a scope whose reservation collapses two blocks onto one coord, naming the block that
/// landed second, under `E_ROUTE_CONGESTION` because the cause is the reserved area.
///
/// With the placement pass's coordinates, two blocks share a coord in one of two ways:
///
/// - a pad column taller than the region: past [`PadColumn::rows`] the pads saturate at
///   `depth - 1`. The placement pass refuses on the same number, so both hand the author
///   [`PadColumn::depth_rule`];
/// - a region one column wide in a scope with no cells and both sensors and actuators: the two
///   pad columns are one, and pad #0 of each stands at `(0,0,0)` at any depth. A scope with cells
///   cannot reach this, because its row needs three columns or more.
///
/// The fix line names the width when the second applies and the depth rule always; a hand-built IR
/// that collides some other way gets the same fix line. Through `cairn` the width branch is
/// unreachable, since `placement::pad_column_diagnostic` refuses first; it serves a library caller
/// with its own IR.
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

/// The nets of one scope: the router they were laid with, driver → sinks, and driver → routed
/// tree.
pub(crate) struct Nets {
    /// Kept so a caller that needs the obstacle set again (the routing pass's tile-layer advisory)
    /// cannot build it differently.
    pub(crate) router: Router,
    pub(crate) sinks: HashMap<NetRef, Vec<CellCoord>>,
    pub(crate) trees: HashMap<NetRef, NetTree>,
}

impl Nets {
    /// The routed length from `net`'s source to `sink`: what stage 2 records as wire and stage 3
    /// counts buffer repeaters against.
    ///
    /// `route_to` answers `None` only for a sink that is not a terminal of the net, which
    /// [`collect_nets`] rules out by building the terminals from the same driver list.
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

/// Measures every cell and actuator pad, each told its index so a panic can name it, then commits
/// each figure through `commit` with the identity a phase-transition panic names.
///
/// The commit panics in release too: the `*_at` transitions refuse a phase the pass does not
/// consume, which is what running a pass twice hands it.
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

/// Runs `lower` over every scope, keeping the IR of each one that succeeds and collecting the
/// findings in scope order. A refused scope contributes its refusal and no IR.
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
