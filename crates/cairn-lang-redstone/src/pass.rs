//! Plumbing shared by the routing, delay and crossing passes, which must rebuild the same trees
//! from the same IR.

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

/// Refuses a scope with cells or outputs but no reservation, under placement's own code.
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

/// One scope a pass is working on, its nets laid.
pub(crate) struct OpenScope {
    pub(crate) ir: PlacementIr,
    pub(crate) region: CircuitRegionReservation,
    /// Every block standing in the reservation, per [`block_sites`].
    pub(crate) blocks: Vec<BlockSite>,
    /// What [`lay_nets`] laid over the blocks.
    pub(crate) nets: Nets,
}

/// Opens `entry` and lays its nets; `Ok(None)` for a scope with neither cells nor actuator pads.
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
/// # Panics
///
/// On an index out of range, which only a hand-built IR can cause.
fn source_of_net<'a>(
    region: &'a CircuitRegionReservation,
    column: PadColumn,
    cell_coords: &'a [CellCoord],
    inputs: usize,
) -> impl Fn(NetRef) -> CellCoord + Copy + 'a {
    move |net| match net {
        NetRef::Input(i) => {
            assert!(
                (i as usize) < inputs,
                "NetRef::Input({i}) out of range (inputs.len()={inputs}) — netlist invariant \
                 broken by caller-side hand-built IR",
            );
            input_pad(i as usize, column, region)
        }
        NetRef::Cell(j) => *cell_coords.get(j as usize).unwrap_or_else(|| {
            panic!(
                "NetRef::Cell({j}) out of range (cells.len()={}) — topological invariant broken \
                 by caller-side hand-built IR",
                cell_coords.len(),
            )
        }),
    }
}

/// The router and every net's routed tree, or the first refusal, cause first: a collapsed pad row,
/// a sink over the cap in a straight line, then a stranded sink.
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

/// Refuses a sink further than the cap from its driver in a straight line, before laying a route;
/// walked in [`crate::delay`]'s order, so both name a sink the same way.
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

/// Its fix line is not delay's "enlarge `region=`": room cannot shorten a straight line.
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

/// Refuses a reservation that puts two blocks on one coord, naming the one that landed second.
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

/// The nets of one scope: the router they were laid with, driver → sinks, and driver → tree.
pub(crate) struct Nets {
    /// Kept so a caller needing the obstacle set again cannot build it differently.
    pub(crate) router: Router,
    pub(crate) sinks: HashMap<NetRef, Vec<CellCoord>>,
    pub(crate) trees: HashMap<NetRef, NetTree>,
}

impl Nets {
    /// The routed length from `net`'s source to `sink`.
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

/// Measures every cell and actuator pad, then commits each figure into its phase through `commit`.
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

/// Runs `lower` over every scope, keeping each IR that succeeds and every finding, in scope order.
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
