//! Stage 1 of `spec/redstone` "Place-and-route": couples each sensor and actuator to the layer
//! under the floor, then lays each cell's tile at the free spot nearest the tiles its nets join.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::block_array::{BlockArrayIr, Dims, MAX_STRUCTURE_VOLUME};
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::{self, IntentModule, Member};
use indexmap::IndexMap;

use crate::coupling::{self, Coupled};
use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::edition_netlist_ir::{EditionCell, EditionNetlistIr, ScopedEditionNetlistIr};
use crate::geometry::{Rel, Rotation, SIDES, Voxel};
use crate::grid::{Grid, Label, Slot, labels};
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CircuitLayout, CircuitNet, LayoutStage, NetSink, PlacedTile, Reservation, ScopedCircuitLayout,
    TileInput, TileKind,
};
use crate::saturating_index;
use crate::tile::{self, MAX_COUNTER_STAGES, MAX_PULSE_STAGES, Template, TilePort};

/// The fewest layers a reservation needs: the bottom one, the cells' and the one under the floor.
pub const MIN_VOID: u32 = 3;

/// The fewest layers a reservation holding a `counter` needs, its feedback crossing a layer up.
pub const MIN_COUNTER_VOID: u32 = 4;

/// The clear columns between the parts of a cell's tile and another tile's, widest first, that
/// placement tries.
pub(crate) const MARGINS: [i32; 3] = [2, 1, 0];

/// The `Fix:` footer of a reservation too small for its circuit.
pub(crate) const CONGESTION_FIX: &str = "Fix: widen the struct's `size=WxH`, increase `void`, or split the logic across several scopes, each with its own `circuit` line";

/// Output of a [`compile_placement`] run: the placed circuits and the findings.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct PlacementOutput {
    /// One circuit per built struct a scope's circuit lies under.
    pub scoped: ScopedCircuitLayout,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Lays each scope of `scoped` under every struct `built` holds for it, leaving out a scope that
/// fails to place.
#[must_use]
pub fn compile_placement(
    scoped: &ScopedEditionNetlistIr,
    module: &IntentModule,
    built: &BlockArrayIr,
) -> PlacementOutput {
    let mut out = PlacementOutput::default();
    let lines = intent::circuit_lines(module);
    for entry in &scoped.scopes {
        let ir = &entry.ir;
        if ir.cells.is_empty() && ir.outputs.is_empty() {
            continue;
        }
        let scope = format!("{}={}:", entry.kind.label(), entry.name);
        let region = lines.iter().find(|line| match line {
            Ok(region) => same_scope(
                region.scope_kind,
                &region.scope_name,
                entry.kind,
                &entry.name,
            ),
            Err(_) => false,
        });
        let Some(Ok(region)) = region else {
            let rejected = lines.iter().find_map(|line| match line {
                Err(rejected)
                    if same_scope(
                        rejected.scope_kind,
                        &rejected.scope_name,
                        entry.kind,
                        &entry.name,
                    ) =>
                {
                    Some(rejected)
                }
                _ => None,
            });
            out.diagnostics.push(match rejected {
                Some(rejected) => rejected_region_diagnostic(rejected),
                None => missing_region_diagnostic(entry.kind, ir),
            });
            continue;
        };
        if let Some(diagnostic) = refusal(&scope, ir, region) {
            out.diagnostics.push(diagnostic);
            continue;
        }
        let Some(members) = scope_members(module, entry.kind, &entry.name) else {
            continue;
        };
        for key in structures(built, entry.kind, &entry.name) {
            let (Some(array), Some(resolved)) =
                (built.structures.get(&key), built.members.get(&key))
            else {
                continue;
            };
            let grown = Dims {
                y: array.dims.y.saturating_add(region.void),
                ..array.dims
            };
            if !grown.fits_volume_budget() {
                extend_unique(&mut out.diagnostics, vec![too_large(&scope, region)]);
                continue;
            }
            let region = Reservation::new(
                region.label.clone(),
                region.void,
                array.dims.x,
                array.dims.z,
                region.span.clone(),
            );
            let (coupled, findings) =
                coupling::couple(ir, members, array, resolved, region.layers, &scope);
            if !findings.is_empty() {
                extend_unique(&mut out.diagnostics, findings);
                continue;
            }
            if let Some(diagnostic) = overlong(&scope, ir, &region) {
                extend_unique(&mut out.diagnostics, vec![diagnostic]);
                continue;
            }
            match place(&scope, ir, &key, region, coupled) {
                Ok(layout) => out.scoped.push(entry.kind, entry.name.clone(), layout),
                Err(diagnostic) => extend_unique(&mut out.diagnostics, vec![diagnostic]),
            }
        }
    }
    out
}

/// The refusal of a `delay` whose row of repeaters is longer than the struct it lies under.
fn overlong(scope: &str, ir: &EditionNetlistIr, region: &Reservation) -> Option<Diagnostic> {
    let side = u64::from(region.width.max(region.depth));
    ir.cells.iter().find_map(|cell| {
        let (EditionCell::JavaRepeaterDelay { stages }
        | EditionCell::BedrockRepeaterDelay { stages }) = cell.cell
        else {
            return None;
        };
        let columns = tile::delay_columns(stages.get());
        (columns > side).then(|| {
            error_with_footer(
                DiagnosticCode::RouteCongestion,
                cell.span.clone(),
                format!(
                    "{scope} a `{name}` of {stages} stages cannot be laid: its repeaters run in one row of {columns} columns, longer than the {width}x{depth} struct it lies under",
                    name = cell.cell.name(),
                    width = region.width,
                    depth = region.depth,
                ),
                "Fix: use fewer stages, or widen the struct's `size=WxH`",
            )
        })
    })
}

/// The refusal of a reservation that takes its struct past what one structure holds.
fn too_large(scope: &str, region: &intent::CircuitRegion) -> Diagnostic {
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        region.span.clone(),
        format!(
            "{scope} `void={void}` takes the struct past {MAX_STRUCTURE_VOLUME} blocks, the most one structure holds",
            void = region.void,
        ),
        "Fix: lower `void`, or narrow the struct's `size=WxH`",
    )
}

/// Adds each of `found` that `diagnostics` does not hold already, so one scope built twice
/// reports once.
fn extend_unique(diagnostics: &mut Vec<Diagnostic>, found: Vec<Diagnostic>) {
    for diagnostic in found {
        if !diagnostics.contains(&diagnostic) {
            diagnostics.push(diagnostic);
        }
    }
}

fn same_scope(kind: intent::ScopeKind, name: &str, scope: ScopeKind, scope_name: &str) -> bool {
    name == scope_name
        && matches!(
            (kind, scope),
            (intent::ScopeKind::Struct, ScopeKind::Struct)
                | (intent::ScopeKind::Def, ScopeKind::Def)
        )
}

/// The members of the struct or def `name`.
fn scope_members<'a>(
    module: &'a IntentModule,
    kind: ScopeKind,
    name: &str,
) -> Option<&'a [Member]> {
    match kind {
        ScopeKind::Struct => module
            .structs
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.members.as_slice()),
        ScopeKind::Def => module
            .defs
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.members.as_slice()),
        ScopeKind::Site => None,
    }
}

/// The keys of every built struct the scope's body lowered to: a struct's own, and each place of a
/// def.
pub(crate) fn structures(built: &BlockArrayIr, kind: ScopeKind, name: &str) -> Vec<String> {
    match kind {
        ScopeKind::Struct => {
            let key = format!("struct::{name}");
            built
                .structures
                .contains_key(&key)
                .then_some(key)
                .into_iter()
                .collect()
        }
        ScopeKind::Def => built
            .placements
            .iter()
            .filter(|(_, placement)| placement.source_def == name)
            .map(|(key, _)| key.clone())
            .collect(),
        ScopeKind::Site => Vec::new(),
    }
}

/// The first reason the scope's netlist cannot be built in a reservation `region` describes.
fn refusal(
    scope: &str,
    ir: &EditionNetlistIr,
    region: &intent::CircuitRegion,
) -> Option<Diagnostic> {
    for cell in &ir.cells {
        let (stages, max, what, instead) = match cell.cell {
            EditionCell::JavaTorchPulse { stages } | EditionCell::BedrockTorchPulse { stages } => (
                stages,
                MAX_PULSE_STAGES,
                "a `pulse`'s inverted operand runs one block of dust past each of its repeaters",
                ", or write a longer pulse of `x` as `x and not delay(x, n)`",
            ),
            EditionCell::JavaRepeaterCounter { stages }
            | EditionCell::BedrockRepeaterCounter { stages } => (
                stages,
                MAX_COUNTER_STAGES,
                "a `counter`'s operand and the operand inverted run past every stage on one strand of dust",
                "",
            ),
            _ => continue,
        };
        if stages.get() > max {
            return Some(error_with_footer(
                DiagnosticCode::AttenuationLimit,
                cell.span.clone(),
                format!(
                    "{scope} a {name} of {stages} stages cannot be built: {what}, so it holds at most {max} stages before the dust goes dark",
                    name = cell.cell.name(),
                ),
                format!("Fix: use at most {max} stages{instead}"),
            ));
        }
    }
    let counter = ir.cells.iter().any(|cell| {
        matches!(
            cell.cell,
            EditionCell::JavaRepeaterCounter { .. } | EditionCell::BedrockRepeaterCounter { .. }
        )
    });
    let (needed, why) = if counter {
        (
            MIN_COUNTER_VOID,
            "a `counter` crosses its feedback over its own wire a layer up, so its circuit needs the bottom layer, two layers of cells and the layer under the floor",
        )
    } else {
        (
            MIN_VOID,
            "a circuit needs the bottom layer, a layer of cells and the layer under the floor that reaches its sensors and actuators",
        )
    };
    (region.void < needed).then(|| {
        error_with_footer(
            DiagnosticCode::RouteCongestion,
            region.span.clone(),
            format!(
                "{scope} `void={void}` reserves too few layers: {why}, {needed} in all",
                void = region.void,
            ),
            format!("Fix: set `void={needed}` or more"),
        )
    })
}

/// The tiles of `ir` and the nets between them, the cells first, in the netlist's order.
fn tiles_and_nets(
    ir: &EditionNetlistIr,
    coupled: &Coupled,
) -> (Vec<PlacedTile>, Vec<CircuitNet>, IndexMap<usize, DottedRef>) {
    let mut tiles: Vec<PlacedTile> = ir
        .cells
        .iter()
        .map(|cell| PlacedTile {
            kind: TileKind::Cell { cell: cell.cell },
            origin: Voxel::new(0, 1, 0),
            rotation: Rotation::East,
            inputs: Vec::new(),
            output: None,
            span: cell.span.clone(),
        })
        .collect();
    let mut drivers: HashMap<NetRef, usize> = (0..ir.cells.len())
        .map(|index| (NetRef::Cell(saturating_index(index)), index))
        .collect();
    for pad in &coupled.pads {
        drivers.insert(NetRef::Input(saturating_index(pad.input)), tiles.len());
        tiles.push(PlacedTile {
            kind: TileKind::Pad { pad: pad.kind },
            origin: pad.origin,
            rotation: Rotation::East,
            inputs: Vec::new(),
            output: None,
            span: pad.span.clone(),
        });
    }
    let mut sinks: Vec<(NetRef, NetSink)> = Vec::new();
    for (index, cell) in ir.cells.iter().enumerate() {
        for driver in &cell.drivers {
            sinks.push((
                driver.net,
                NetSink {
                    tile: index,
                    port: TilePort::Cell(driver.port),
                },
            ));
        }
        if matches!(
            cell.cell,
            EditionCell::JavaRepeaterCounter { .. } | EditionCell::BedrockRepeaterCounter { .. }
        ) {
            sinks.push((
                NetRef::Cell(saturating_index(index)),
                NetSink {
                    tile: index,
                    port: TilePort::Feedback,
                },
            ));
        }
    }
    let mut riser_signals = IndexMap::new();
    for riser in &coupled.risers {
        riser_signals.insert(tiles.len(), ir.outputs[riser.output].name.clone());
        sinks.push((
            ir.outputs[riser.output].driver,
            NetSink {
                tile: tiles.len(),
                port: TilePort::Actuator,
            },
        ));
        tiles.push(PlacedTile {
            kind: TileKind::Riser,
            origin: riser.top,
            rotation: Rotation::East,
            inputs: Vec::new(),
            output: None,
            span: riser.span.clone(),
        });
    }
    let nets = join(ir, &mut tiles, &drivers, &sinks);
    (tiles, nets, riser_signals)
}

/// The nets from each driver to the sinks it feeds, inputs first, each tile told its own.
fn join(
    ir: &EditionNetlistIr,
    tiles: &mut [PlacedTile],
    drivers: &HashMap<NetRef, usize>,
    sinks: &[(NetRef, NetSink)],
) -> Vec<CircuitNet> {
    let order = (0..ir.inputs.len())
        .map(|i| NetRef::Input(saturating_index(i)))
        .chain((0..ir.cells.len()).map(|i| NetRef::Cell(saturating_index(i))));
    let mut nets = Vec::new();
    for source in order {
        let Some(&driver) = drivers.get(&source) else {
            continue;
        };
        let feeds: Vec<NetSink> = sinks
            .iter()
            .filter(|(net, _)| *net == source)
            .map(|(_, sink)| *sink)
            .collect();
        if feeds.is_empty() {
            continue;
        }
        let net = nets.len();
        tiles[driver].output = Some(net);
        for sink in &feeds {
            tiles[sink.tile].inputs.push(TileInput {
                port: sink.port,
                net,
            });
        }
        nets.push(CircuitNet {
            source,
            driver,
            sinks: feeds,
            wire: crate::placement_ir::Wire::default(),
        });
    }
    nets
}

/// The circuit of `ir` under the struct `structure`, its cells laid at each of [`MARGINS`] and the
/// layout whose cells lie nearest the tiles they share nets with kept.
fn place(
    scope: &str,
    ir: &EditionNetlistIr,
    structure: &str,
    region: Reservation,
    coupled: Coupled,
) -> Result<CircuitLayout, Diagnostic> {
    let (tiles, nets, riser_signals) = tiles_and_nets(ir, &coupled);
    let mut layout = CircuitLayout {
        edition: ir.edition,
        structure: structure.to_owned(),
        stage: LayoutStage::Placed,
        region,
        tiles,
        nets,
        blocks: Vec::new(),
        signal_defs: ir.signal_defs.clone(),
        riser_signals,
        surface: coupled.surface,
        margin: MARGINS[0],
    };
    let mut grid = Grid::new(&layout.region);
    let fixed: Vec<usize> = (0..layout.tiles.len())
        .filter(|&i| !matches!(layout.tiles[i].kind, TileKind::Cell { .. }))
        .collect();
    lay_fixed(scope, &mut layout, &mut grid, &fixed)?;
    let mut best: Option<(u32, CircuitLayout)> = None;
    let mut refused = None;
    for margin in MARGINS {
        let mut laid = layout.clone();
        match lay_cells(scope, &mut laid, grid.clone(), margin) {
            Ok(()) => {
                let spread = spread(&laid);
                if best.as_ref().is_none_or(|(least, _)| spread < *least) {
                    best = Some((spread, laid));
                }
            }
            Err(diagnostic) => refused = refused.or(Some(diagnostic)),
        }
    }
    match best {
        Some((_, laid)) => Ok(laid),
        None => Err(refused.unwrap_or_else(|| congestion(scope, &layout))),
    }
}

/// How far, in all, every cell of `layout` lies from the tiles it shares nets with.
fn spread(layout: &CircuitLayout) -> u32 {
    let all: Vec<usize> = (0..layout.tiles.len()).collect();
    layout
        .tiles
        .iter()
        .filter(|tile| matches!(tile.kind, TileKind::Cell { .. }))
        .map(|tile| pull(layout, &all, tile, &tile.kind.template()))
        .fold(0, u32::saturating_add)
}

/// The most turns of pads and risers [`lay_fixed`] tries before it gives up.
const FIXED_TRIALS: u32 = 1024;

/// Turns every pad and riser of `fixed` a way that fits beside the others, each nearest the middle
/// of the reservation that leaves the rest room.
fn lay_fixed(
    scope: &str,
    layout: &mut CircuitLayout,
    grid: &mut Grid,
    fixed: &[usize],
) -> Result<(), Diagnostic> {
    let mut trials = FIXED_TRIALS;
    match lay_from(layout, grid, fixed, &mut trials) {
        Ok(laid) => {
            *grid = laid;
            Ok(())
        }
        Err(index) => {
            let tile = &layout.tiles[index];
            let what = match tile.kind {
                TileKind::Riser => "the torches that carry its signal up to it",
                _ => "the pad its signal enters the circuit by",
            };
            Err(error_with_footer(
                DiagnosticCode::RouteCongestion,
                tile.span.clone(),
                format!(
                    "{scope} the layers under this component have no room for {what}: each way of laying them leaves the reservation or meets another component's"
                ),
                "Fix: move the component away from the struct's edge and from the other components beside it",
            ))
        }
    }
}

/// `grid` with the tiles of `fixed` laid on it in order, or the tile no turn of which fits.
fn lay_from(
    layout: &mut CircuitLayout,
    grid: &Grid,
    fixed: &[usize],
    trials: &mut u32,
) -> Result<Grid, usize> {
    let Some((&index, rest)) = fixed.split_first() else {
        return Ok(grid.clone());
    };
    let mut failed = index;
    for (rotation, trial) in turns(layout, grid, index) {
        if *trials == 0 {
            break;
        }
        *trials -= 1;
        layout.tiles[index].rotation = rotation;
        match lay_from(layout, &trial, rest, trials) {
            Ok(laid) => return Ok(laid),
            Err(index) => failed = index,
        }
    }
    Err(failed)
}

/// Every turn of the pad or riser `index` that fits on `grid`, nearest the middle first, with
/// the grid it leaves.
fn turns(layout: &CircuitLayout, grid: &Grid, index: usize) -> Vec<(Rotation, Grid)> {
    let template = layout.tiles[index].kind.template();
    let labels = labels(layout, index);
    let middle = Voxel::new(
        i32::try_from(layout.region.width / 2).unwrap_or(0),
        layout.tiles[index].origin.y,
        i32::try_from(layout.region.depth / 2).unwrap_or(0),
    );
    let lead = lead(&layout.tiles[index], &template);
    let mut fits: Vec<(u32, Rotation, Grid)> = Vec::new();
    for rotation in Rotation::ALL {
        let mut tile = layout.tiles[index].clone();
        tile.rotation = rotation;
        if !fixed_fits(layout, &tile, &template) {
            continue;
        }
        let mut trial = grid.clone();
        if trial.lay_tile(index, &tile, &template, &labels).is_ok() {
            fits.push((tile.at(lead).distance(middle), rotation, trial));
        }
    }
    fits.sort_by_key(|(cost, _, _)| *cost);
    fits.into_iter()
        .map(|(_, rotation, trial)| (rotation, trial))
        .collect()
}

/// The part of a tile that faces the rest of the circuit: a riser's input, a pad's output.
fn lead(tile: &PlacedTile, template: &Template) -> Rel {
    match tile.kind {
        TileKind::Riser => template
            .port(TilePort::Actuator)
            .map_or(Rel::new(0, 0, 0), |port| port.at),
        _ => template.outs.first().copied().unwrap_or(Rel::new(0, 0, 0)),
    }
}

/// Whether a pad or riser turned as `tile` is keeps its parts inside the reservation, any part
/// beside its origin in the top layer under columns quiet but for its own sensor, and a riser's
/// upper block under a plain one.
fn fixed_fits(layout: &CircuitLayout, tile: &PlacedTile, template: &Template) -> bool {
    let top = layout.region.top();
    template.parts.iter().all(|(rel, _)| {
        let at = tile.at(*rel);
        if !layout.region.contains(at) || at.y < 1 {
            return false;
        }
        if at.y != top || (at.x == tile.origin.x && at.z == tile.origin.z) {
            return true;
        }
        let Some(column) = layout.surface.column(at.x, at.z) else {
            return false;
        };
        match tile.kind {
            TileKind::Riser => column.plain,
            _ => column.quiet_but_for((tile.origin.x, tile.origin.z)),
        }
    })
}

/// `layout` with its cells laid again, `margin` columns apart, around its pads and risers.
pub(crate) fn relay(
    scope: &str,
    layout: &CircuitLayout,
    margin: i32,
) -> Result<CircuitLayout, Diagnostic> {
    let mut layout = layout.clone();
    for net in &mut layout.nets {
        net.wire = crate::placement_ir::Wire::default();
    }
    let mut grid = Grid::new(&layout.region);
    for index in 0..layout.tiles.len() {
        if matches!(layout.tiles[index].kind, TileKind::Cell { .. }) {
            continue;
        }
        let template = layout.tiles[index].kind.template();
        grid.lay_tile(
            index,
            &layout.tiles[index],
            &template,
            &labels(&layout, index),
        )
        .map_err(|_| congestion(scope, &layout))?;
    }
    lay_cells(scope, &mut layout, grid, margin)?;
    Ok(layout)
}

/// The 2D box a tile's parts cover, as `(x0, z0, x1, z1)`.
pub(crate) type Bounds = (i32, i32, i32, i32);

/// The [`Bounds`] of `tile`.
pub(crate) fn footprint(tile: &PlacedTile, template: &Template) -> Bounds {
    let cells = template.parts.iter().map(|(rel, _)| tile.at(*rel));
    cells.fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |b, at| {
        (b.0.min(at.x), b.1.min(at.z), b.2.max(at.x), b.3.max(at.z))
    })
}

fn overlaps(a: Bounds, b: Bounds, margin: i32) -> bool {
    a.0 - margin <= b.2 && b.0 <= a.2 + margin && a.1 - margin <= b.3 && b.1 <= a.3 + margin
}

/// Lays every cell's tile, in the netlist's order, at the spot nearest the tiles its nets join,
/// `margin` columns clear of every other tile.
pub(crate) fn lay_cells(
    scope: &str,
    layout: &mut CircuitLayout,
    mut grid: Grid,
    margin: i32,
) -> Result<(), Diagnostic> {
    layout.margin = margin;
    let cells: Vec<usize> = (0..layout.tiles.len())
        .filter(|&i| matches!(layout.tiles[i].kind, TileKind::Cell { .. }))
        .collect();
    let mut placed: Vec<usize> = (0..layout.tiles.len())
        .filter(|i| !cells.contains(i))
        .collect();
    let mut boxes: Vec<Bounds> = placed
        .iter()
        .map(|&i| footprint(&layout.tiles[i], &layout.tiles[i].kind.template()))
        .collect();
    let top = layout.region.top();
    for index in cells {
        let template = layout.tiles[index].kind.template();
        let ends = labels(layout, index);
        let mut tile = layout.tiles[index].clone();
        let ties = Ties::of(layout, &placed, &tile, &template);
        // Each spot as how far its nets run, then its `z`, `x` and turn, least first.
        let mut candidates: Vec<Reverse<(u32, i32, i32, usize)>> = Vec::new();
        for (turn, rotation) in Rotation::ALL.into_iter().enumerate() {
            tile.origin = Voxel::new(0, 1, 0);
            tile.rotation = rotation;
            let (low, high) = extent(&tile, &template);
            let (x0, z0, x1, z1) = footprint(&tile, &template);
            for z in 0..coord(layout.region.depth) {
                for x in 0..coord(layout.region.width) {
                    let inside = layout
                        .region
                        .contains(Voxel::new(low.x + x, low.y, low.z + z))
                        && layout
                            .region
                            .contains(Voxel::new(high.x + x, high.y, high.z + z))
                        && high.y < top;
                    if !inside {
                        continue;
                    }
                    let bounds = (x0 + x, z0 + z, x1 + x, z1 + z);
                    if boxes.iter().any(|other| overlaps(bounds, *other, margin)) {
                        continue;
                    }
                    tile.origin = Voxel::new(x, 1, z);
                    candidates.push(Reverse((ties.cost(&tile), z, x, turn)));
                }
            }
        }
        let mut candidates = BinaryHeap::from(candidates);
        let best = std::iter::from_fn(|| candidates.pop()).find(|Reverse((_, z, x, turn))| {
            tile.origin = Voxel::new(*x, 1, *z);
            tile.rotation = Rotation::ALL[*turn];
            grid.trial(index, &tile, &template, &ends, |trial| {
                reachable(trial, &tile, &ends)
                    && placed.iter().all(|&other| {
                        reachable(trial, &layout.tiles[other], &labels(layout, other))
                    })
            })
        });
        if best.is_none() {
            let TileKind::Cell { cell } = layout.tiles[index].kind else {
                unreachable!("only cells are laid here")
            };
            return Err(error_with_footer(
                DiagnosticCode::RouteCongestion,
                layout.tiles[index].span.clone(),
                format!(
                    "{scope} the reservation has no room left for this `{name}` cell: its tile needs {w}x{d} columns clear of the other tiles' (region {width}x{depth}, void={void})",
                    name = cell.name(),
                    w = bounds_of(&template).0,
                    d = bounds_of(&template).1,
                    width = layout.region.width,
                    depth = layout.region.depth,
                    void = layout.region.void,
                ),
                CONGESTION_FIX,
            ));
        }
        grid.lay_tile(index, &tile, &template, &ends)
            .map_err(|_| congestion(scope, layout))?;
        boxes.push(footprint(&tile, &template));
        layout.tiles[index] = tile;
        placed.push(index);
    }
    Ok(())
}

/// Whether every port and output of `tile`, its nets `labels`, still has a side a wire can reach
/// it from: an open voxel beside it, level or a step up or down, that no other signal's dust
/// touches, or its own net's dust.
fn reachable(grid: &Grid, tile: &PlacedTile, labels: &[(Rel, usize)]) -> bool {
    let near = |at: Voxel| {
        SIDES.into_iter().flat_map(move |face| {
            let beside = at.step(face);
            [beside, beside.up(), beside.down()]
        })
    };
    labels.iter().all(|(rel, net)| {
        let end = tile.at(*rel);
        near(end).any(|cell| match grid.get(cell) {
            Some(Slot::Open) => near(cell).all(|other| {
                other == end
                    || !matches!(grid.get(other), Some(Slot::Dust(label)) if label != Label::Net(*net))
            }),
            Some(Slot::Dust(Label::Net(other))) => other == *net,
            _ => false,
        })
    })
}

/// A tile's extent along `x` and `z` as laid east.
fn bounds_of(template: &Template) -> (i32, i32) {
    let tile = PlacedTile {
        kind: TileKind::Riser,
        origin: Voxel::new(0, 0, 0),
        rotation: Rotation::East,
        inputs: Vec::new(),
        output: None,
        span: Span::default(),
    };
    let (x0, z0, x1, z1) = footprint(&tile, template);
    (x1 - x0 + 1, z1 - z0 + 1)
}

/// The lowest and highest corner of the box `tile`'s parts fill.
fn extent(tile: &PlacedTile, template: &Template) -> (Voxel, Voxel) {
    let cells = template.parts.iter().map(|(rel, _)| tile.at(*rel));
    let far = Voxel::new(i32::MAX, i32::MAX, i32::MAX);
    let near = Voxel::new(i32::MIN, i32::MIN, i32::MIN);
    cells.fold((far, near), |(low, high), at| {
        (
            Voxel::new(low.x.min(at.x), low.y.min(at.y), low.z.min(at.z)),
            Voxel::new(high.x.max(at.x), high.y.max(at.y), high.z.max(at.z)),
        )
    })
}

/// How far `tile` lies from the placed tiles it shares nets with, port to output.
fn pull(layout: &CircuitLayout, placed: &[usize], tile: &PlacedTile, template: &Template) -> u32 {
    Ties::of(layout, placed, tile, template).cost(tile)
}

/// Each net a tile shares with a placed tile: the tile's ends on it and the placed tile's.
struct Ties(Vec<(Vec<Rel>, Vec<Voxel>)>);

impl Ties {
    fn of(
        layout: &CircuitLayout,
        placed: &[usize],
        tile: &PlacedTile,
        template: &Template,
    ) -> Self {
        let mut ties = Vec::new();
        for input in &tile.inputs {
            let net = &layout.nets[input.net];
            let (Some(port), true) = (template.port(input.port), placed.contains(&net.driver))
            else {
                continue;
            };
            let driver = &layout.tiles[net.driver];
            let outs = driver
                .kind
                .template()
                .outs
                .iter()
                .map(|rel| driver.at(*rel))
                .collect();
            ties.push((vec![port.at], outs));
        }
        if let Some(net) = tile.output {
            for sink in &layout.nets[net].sinks {
                let other = &layout.tiles[sink.tile];
                let port = other
                    .kind
                    .template()
                    .port(sink.port)
                    .map(|p| other.at(p.at));
                if let (Some(port), true) = (port, placed.contains(&sink.tile)) {
                    ties.push((template.outs.clone(), vec![port]));
                }
            }
        }
        Self(ties)
    }

    fn cost(&self, tile: &PlacedTile) -> u32 {
        self.0
            .iter()
            .map(|(ends, others)| {
                let gaps = ends
                    .iter()
                    .flat_map(|end| others.iter().map(|o| o.distance(tile.at(*end))));
                gaps.min().unwrap_or(0)
            })
            .fold(0, u32::saturating_add)
    }
}

fn congestion(scope: &str, layout: &CircuitLayout) -> Diagnostic {
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        layout.region.span.clone(),
        format!(
            "{scope} the circuit does not fit its reservation (region {width}x{depth}, void={void})",
            width = layout.region.width,
            depth = layout.region.depth,
            void = layout.region.void,
        ),
        CONGESTION_FIX,
    )
}

/// `E_NO_CIRCUIT_REGION` for a scope with something to place and no `circuit` line the walk found.
fn missing_region_diagnostic(kind: ScopeKind, source: &EditionNetlistIr) -> Diagnostic {
    let span = source
        .cells
        .first()
        .map(|c| c.span.clone())
        .or_else(|| source.outputs.first().map(|o| o.span.clone()))
        .unwrap_or_default();
    let (primary, fix) = match kind {
        ScopeKind::Struct | ScopeKind::Def => (
            "this scope has redstone cells or actuators to build but no `circuit` line at its top level to reserve room for them",
            "Fix: add a `circuit region=<label> void=<N>` line at the top level of the scope, with a non-empty label naming the reservation (`region=floor`, `region=basement`) and an integer `void=` >= 3. It reserves the layers under the scope's base plane",
        ),
        ScopeKind::Site => (
            "this `site` has redstone cells or actuators to build, and a `site` has no base plane of its own to reserve room under",
            "Fix: move the redstone into a `struct` or `def` with a `size=WxH` header and a `circuit region=<label> void=<N>` line at its top level",
        ),
    };
    error_with_footer(
        DiagnosticCode::NoCircuitRegion,
        span,
        primary.to_owned(),
        fix,
    )
}

/// `E_NO_CIRCUIT_REGION` on the `circuit` line that reserves nothing, with core's reason clause.
fn rejected_region_diagnostic(rejected: &intent::RejectedCircuitRegion) -> Diagnostic {
    use intent::CircuitRegionDefect as Defect;
    let fix = match &rejected.defect {
        Defect::NestedUnderLevel { .. } => "Fix: move the `circuit` line out of the `level` to the scope's top level. That changes nothing else about it: a reservation lies under the scope's base plane, and no pass reads a `level`'s `y=` for a `circuit` line".to_owned(),
        Defect::NoSize { .. } => "Fix: give the enclosing scope a `size=WxH` header".to_owned(),
        Defect::RegionMissing { .. } => {
            "Fix: add `region=<label>` naming the reservation (`region=floor`, `region=basement`)"
                .to_owned()
        }
        Defect::RegionNotLabel { .. } => {
            "Fix: write `region=` as an identifier or a string (`region=floor`, `region=basement`)"
                .to_owned()
        }
        Defect::RegionEmpty { .. } => {
            "Fix: give `region=` a non-empty label (`region=floor`, `region=basement`)".to_owned()
        }
        Defect::VoidMissing { .. } => {
            "Fix: add `void=<N>`, the layers under the base plane, an integer >= 3".to_owned()
        }
        Defect::VoidNotInteger { .. } => "Fix: write `void=` as an integer >= 3".to_owned(),
        Defect::VoidBelowOne { .. } => "Fix: set `void=` to an integer >= 3".to_owned(),
        Defect::VoidTooLarge { .. } => format!(
            "Fix: set `void=` to an integer from 3 to {limit}",
            limit = u32::MAX,
        ),
        _ => "Fix: write the line as `circuit region=<label> void=<N>` at the top level of a `struct` or `def` with a `size=WxH` header, with a non-empty label and an integer `void=` >= 3".to_owned(),
    };
    error_with_footer(
        DiagnosticCode::NoCircuitRegion,
        rejected.span.clone(),
        format!(
            "this `circuit` line reserves no room for the scope's redstone cells or actuators: {reason}",
            reason = rejected.defect,
        ),
        fix,
    )
}

fn coord(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}
