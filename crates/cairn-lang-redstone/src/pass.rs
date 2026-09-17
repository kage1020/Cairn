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
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CellIdentity, CircuitRegionReservation, PlacedCellNode, PlacedOutputNode,
    PlacementIr, PlacementPhase, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{
    BlockSite, NetTree, Router, block_sites, collect_nets, input_pad, net_trees, unroutable,
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
    let blocks = block_sites(&ir, &region);
    Ok(OpenScope {
        ir,
        region,
        cell_coords,
        blocks,
    })
}

/// `NetRef → source coord`: the input pad for a sensor, the cell body
/// for a gate.
///
/// `NetRef::Cell(j)` is in range by the topological invariant carried
/// across every prior IR stage (`j < i` inside `cells[i]`); a hand-built
/// IR that breaks it panics rather than under-reporting what the pass
/// writes.
pub(crate) fn source_of_net<'a>(
    region: &'a CircuitRegionReservation,
    cell_coords: &'a [CellCoord],
) -> impl Fn(NetRef) -> CellCoord + Copy + 'a {
    move |net| {
        match net {
        NetRef::Input(i) => input_pad(i as usize, region),
        NetRef::Cell(j) => *cell_coords.get(j as usize).unwrap_or_else(|| {
            panic!(
                "NetRef::Cell({j}) out of range (cells.len()={}) — topological invariant broken by caller-side hand-built IR",
                cell_coords.len(),
            )
        }),
    }
    }
}

/// [`source_of_net`] for the routing pass, which asserts the invariant
/// in debug builds and saturates the lookup to the last cell in release
/// so a caller-side bug still produces deterministic output.
pub(crate) fn source_of_net_lenient<'a>(
    region: &'a CircuitRegionReservation,
    cell_coords: &'a [CellCoord],
) -> impl Fn(NetRef) -> CellCoord + Copy + 'a {
    move |net| match net {
        NetRef::Input(i) => input_pad(i as usize, region),
        NetRef::Cell(j) => {
            debug_assert!(
                (j as usize) < cell_coords.len(),
                "NetRef::Cell({j}) out of range (cells.len()={}) — topological invariant broken",
                cell_coords.len(),
            );
            cell_coords
                .get(j as usize)
                .copied()
                .unwrap_or_else(|| *cell_coords.last().expect("cells.is_empty checked above"))
        }
    }
}

/// Every net of the scope and its routed tree, or the refusal the scope
/// earns when the reservation cannot wire one of its sinks.
///
/// Asked by all three passes: stage 2 elides an unroutable scope, but
/// stages 3 and 4 rebuild the trees from the IR, and a stranded sink's
/// route is one step — under every cap, worth no repeater, and
/// indistinguishable in the dump from a circuit that works.
pub(crate) fn lay_nets<F>(
    ir: &PlacementIr,
    router: &Router,
    entry: &ScopedPlacementIrEntry,
    region: &CircuitRegionReservation,
    source_of_net: F,
) -> Result<Nets, Diagnostic>
where
    F: Fn(NetRef) -> CellCoord + Copy,
{
    let sinks = collect_nets(ir);
    let trees = net_trees(&sinks, router, source_of_net);
    if let Some(diagnostic) = unroutable(&sinks, &trees, entry, region, source_of_net) {
        return Err(diagnostic);
    }
    Ok(Nets { sinks, trees })
}

/// The nets of one scope: driver → sinks, and driver → routed tree.
pub(crate) struct Nets {
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
