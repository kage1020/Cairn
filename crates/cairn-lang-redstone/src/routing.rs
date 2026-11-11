//! Stage 2 of `spec/redstone` "Place-and-route": runs each net's dust from the tile that drives it
//! to every input it feeds, along a layer and up and down staircases, apart from every other net's
//! dust.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::coupling::Column;
use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer, overlapping};
use crate::geometry::{SIDES, Voxel};
use crate::grid::{Collision, Grid, Label, Slot};
use crate::part::Part;
use crate::placement::{CONGESTION_FIX, MARGINS, relay};
use crate::placement_ir::{CircuitLayout, LayoutStage, ScopedCircuitLayout, TileKind, Wire};

/// How many times one wire is searched again around a voxel its last try could not take.
const BANS: usize = 8;

/// How many times the nets are routed again, the one that failed moved first.
const RESTARTS: usize = 3;

/// Output of a [`compile_routing`] run: the routed circuits and the findings.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct RoutingOutput {
    /// Every circuit routing kept.
    pub scoped: ScopedCircuitLayout,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Routes every net of every circuit of `placed`, laying a circuit's cells again at the other
/// margins when its nets do not all fit, and leaving out one that never fits.
///
/// # Panics
///
/// On a circuit placement has not produced.
#[must_use]
pub fn compile_routing(placed: &ScopedCircuitLayout) -> RoutingOutput {
    let mut out = RoutingOutput::default();
    for entry in &placed.scopes {
        let scope = format!("{}={}:", entry.kind.label(), entry.name);
        assert_eq!(
            entry.ir.stage,
            LayoutStage::Placed,
            "{scope} routing runs the nets of the circuit placement produces",
        );
        match route_layout(&scope, &entry.ir) {
            Ok(layout) => out.scoped.push(entry.kind, entry.name.clone(), layout),
            Err(diagnostic) => {
                if !out.diagnostics.contains(&diagnostic) {
                    out.diagnostics.push(diagnostic);
                }
            }
        }
    }
    out
}

fn route_layout(scope: &str, placed: &CircuitLayout) -> Result<CircuitLayout, Diagnostic> {
    let mut failed = match route_all(placed) {
        Ok(routed) => return Ok(routed),
        Err(Unrouted::Net(net)) => net,
        Err(Unrouted::Overlap(Collision { at })) => {
            return Err(overlapping(scope, placed.region.span.clone(), at));
        }
    };
    for margin in MARGINS
        .into_iter()
        .filter(|margin| *margin != placed.margin)
    {
        // A margin whose cells do not fit is passed over.
        let Ok(layout) = relay(scope, placed, margin) else {
            continue;
        };
        match route_all(&layout) {
            Ok(routed) => return Ok(routed),
            Err(Unrouted::Net(net)) => failed = net,
            Err(Unrouted::Overlap(Collision { at })) => {
                return Err(overlapping(scope, placed.region.span.clone(), at));
            }
        }
    }
    Err(unroutable(scope, placed, failed))
}

/// Why [`route_all`] laid no wires.
enum Unrouted {
    /// The net that found no way.
    Net(usize),
    /// Two tiles of the layout meet, so no grid holds it.
    Overlap(Collision),
}

/// `layout` with every net routed.
fn route_all(layout: &CircuitLayout) -> Result<CircuitLayout, Unrouted> {
    let mut order: Vec<usize> = (0..layout.nets.len()).collect();
    order.sort_by_key(|&net| (Reverse(layout.nets[net].sinks.len()), net));
    let mut last = 0;
    for _ in 0..=RESTARTS {
        let mut routed = layout.clone();
        for net in &mut routed.nets {
            net.wire = Wire::default();
        }
        let mut grid = Grid::of(&routed).map_err(Unrouted::Overlap)?;
        let mut stuck = None;
        for &net in &order {
            let Some(wire) = route_net(&grid, &routed, net) else {
                stuck = Some(net);
                break;
            };
            if grid.lay_wire(net, &wire).is_err() {
                stuck = Some(net);
                break;
            }
            routed.nets[net].wire = wire;
        }
        let Some(net) = stuck else {
            routed.stage = LayoutStage::Routed;
            return Ok(routed);
        };
        last = net;
        order.retain(|&n| n != net);
        order.insert(0, net);
    }
    Err(Unrouted::Net(last))
}

/// The wire of `net` from its driver to every input it feeds, on `grid` as the nets before it
/// left it.
fn route_net(grid: &Grid, layout: &CircuitLayout, net: usize) -> Option<Wire> {
    let circuit = &layout.nets[net];
    let driver = &layout.tiles[circuit.driver];
    let template = driver.kind.template();
    let mut tree: Vec<Voxel> = template.outs.iter().map(|rel| driver.at(*rel)).collect();
    let mut wire = Wire::default();
    let mut grid = grid.clone();
    let mut goals: Vec<Voxel> = circuit
        .sinks
        .iter()
        .filter_map(|sink| {
            let tile = &layout.tiles[sink.tile];
            tile.kind
                .template()
                .port(sink.port)
                .map(|port| tile.at(port.at))
        })
        .collect();
    let origin = tree[0];
    goals.sort_by_key(|goal| (goal.distance(origin), *goal));
    for goal in goals {
        if tree.contains(&goal) {
            continue;
        }
        let mut banned: HashSet<Voxel> = HashSet::new();
        let mut found = None;
        for _ in 0..=BANS {
            let steps = search(&grid, layout, net, &tree, goal, &banned)?;
            let piece = lay_steps(&grid, &tree, &steps);
            let mut trial = grid.clone();
            match trial.lay_wire(net, &piece) {
                Ok(()) => {
                    found = Some((trial, piece));
                    break;
                }
                Err(collision) => {
                    banned.insert(collision.at);
                }
            }
        }
        let (trial, piece) = found?;
        grid = trial;
        tree.extend(piece.dust.iter().copied());
        tree.push(goal);
        wire.dust.extend(piece.dust);
        wire.supports.extend(piece.supports);
        wire.air.extend(piece.air);
    }
    for list in [&mut wire.dust, &mut wire.supports, &mut wire.air] {
        list.sort_unstable();
        list.dedup();
    }
    Some(wire)
}

/// One step of a search: the voxel it reaches and the one it left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Step {
    from: Voxel,
    to: Voxel,
}

/// The cheapest chain of steps from any voxel of `tree` to `goal`, its last step ending on it.
fn search(
    grid: &Grid,
    layout: &CircuitLayout,
    net: usize,
    tree: &[Voxel],
    goal: Voxel,
    banned: &HashSet<Voxel>,
) -> Option<Vec<Step>> {
    let mut best: HashMap<Voxel, u32> = HashMap::new();
    let mut came: HashMap<Voxel, Voxel> = HashMap::new();
    let mut heap = BinaryHeap::new();
    let estimate = |at: Voxel| at.distance(goal);
    for &start in tree {
        best.insert(start, 0);
        heap.push(Reverse((estimate(start), 0u32, start)));
    }
    while let Some(Reverse((_, cost, at))) = heap.pop() {
        if at == goal {
            let mut steps = Vec::new();
            let mut to = goal;
            while let Some(&from) = came.get(&to) {
                steps.push(Step { from, to });
                to = from;
            }
            steps.reverse();
            return Some(steps);
        }
        if best.get(&at).is_some_and(|b| *b < cost) {
            continue;
        }
        for face in SIDES {
            for rise in [0, 1, -1] {
                let to = Voxel::new(at.x, at.y + rise, at.z).step(face);
                let step = Step { from: at, to };
                let lone = tree.contains(&at) && lone(grid, at);
                if !step_fits(grid, layout, net, step, goal, banned, lone) {
                    continue;
                }
                let spent = cost + if rise == 0 { 1 } else { 2 };
                if best.get(&to).is_none_or(|b| spent < *b) {
                    best.insert(to, spent);
                    came.insert(to, at);
                    heap.push(Reverse((spent + estimate(to), spent, to)));
                }
            }
        }
    }
    None
}

/// Whether the dust at `at` joins nothing yet, so a wire leaving it turns it into a line that
/// points on past it.
fn lone(grid: &Grid, at: Voxel) -> bool {
    !SIDES.into_iter().any(|face| {
        let beside = at.step(face);
        let joins_flat = match grid.get(beside) {
            Some(Slot::Dust(_) | Slot::Part(Part::Torch { .. } | Part::Comparator { .. })) => true,
            Some(Slot::Part(part @ Part::Repeater { .. })) => part
                .drives()
                .is_some_and(|f| f == face || f == face.opposite()),
            _ => false,
        };
        let joins_up =
            matches!(grid.get(beside.up()), Some(Slot::Dust(_))) && !grid.conducts(at.up());
        let joins_down =
            matches!(grid.get(beside.down()), Some(Slot::Dust(_))) && !grid.conducts(beside);
        joins_flat || joins_up || joins_down
    })
}

/// Whether `step` may join the wire of `net`: the dust it lays fits, and the climb it makes has
/// its cells to itself.
fn step_fits(
    grid: &Grid,
    layout: &CircuitLayout,
    net: usize,
    step: Step,
    goal: Voxel,
    banned: &HashSet<Voxel>,
    lone: bool,
) -> bool {
    let Step { from, to } = step;
    if banned.contains(&to) {
        return false;
    }
    let flat = Voxel::new(to.x, from.y, to.z);
    let rise = to.y - from.y;
    // A climb keeps clear the cell the link passes: over the lower dust going up, beside the
    // upper going down.
    let passes = match rise {
        1 => Some(from.up()),
        -1 => Some(flat),
        _ => None,
    };
    if let Some(cell) = passes
        && (banned.contains(&cell) || !clearable(grid, cell))
    {
        return false;
    }
    if rise == 1
        && grid
            .get(flat)
            .is_some_and(|slot| matches!(slot, Slot::Dust(_)))
    {
        return false;
    }
    if lone {
        let face = Grid::toward(from, flat).unwrap_or(cairn_lang_core::block_array::Face::East);
        let behind = from.step(face.opposite());
        if grid.get(behind).is_some() && (banned.contains(&behind) || !clearable(grid, behind)) {
            return false;
        }
    }
    if to == goal {
        return grid.get(to) == Some(Slot::Dust(Label::Net(net))) && grid.conducts(to.down());
    }
    dust_fits(grid, layout, net, to, banned)
}

/// Whether `at` holds nothing a wire may not keep empty.
fn clearable(grid: &Grid, at: Voxel) -> bool {
    matches!(
        grid.get(at),
        None | Some(Slot::Open | Slot::NoDust | Slot::Air)
    )
}

/// Whether wire dust of `net` may stand at `at`.
fn dust_fits(
    grid: &Grid,
    layout: &CircuitLayout,
    net: usize,
    at: Voxel,
    banned: &HashSet<Voxel>,
) -> bool {
    let top = layout.region.top();
    if grid.get(at) != Some(Slot::Open) || at.y < 1 || at.y > top {
        return false;
    }
    let driver = &layout.tiles[layout.nets[net].driver];
    let own =
        matches!(driver.kind, TileKind::Pad { .. }).then_some((driver.origin.x, driver.origin.z));
    let quiet = |column: Column| own.map_or(column.quiet, |own| column.quiet_but_for(own));
    if at.y == top && !layout.surface.column(at.x, at.z).is_some_and(quiet) {
        return false;
    }
    let under = at.down();
    let held = grid.conducts(under)
        || (under.y >= 1
            && !banned.contains(&under)
            && matches!(grid.get(under), Some(Slot::Open | Slot::NoDust))
            && !grid.torch(under.down())
            && !grid.driven(under));
    if !held || grid.torch(at.up()) {
        return false;
    }
    let foreign = |slot: Option<Slot>| match slot {
        Some(Slot::Dust(Label::Net(other))) => other != net,
        Some(Slot::Dust(Label::Internal(_))) => true,
        _ => false,
    };
    for face in SIDES {
        let beside = at.step(face);
        if foreign(grid.get(beside)) {
            return false;
        }
        // Dust a step down links unless a block over it cuts the link; dust a step up links
        // unless a block over this dust does.
        if foreign(grid.get(beside.down())) && !grid.conducts(beside) {
            return false;
        }
        if foreign(grid.get(beside.up())) && !grid.conducts(at.up()) {
            return false;
        }
    }
    true
}

/// The wire a chain of steps lays: its dust, the blocks under it and the cells its climbs pass.
fn lay_steps(grid: &Grid, tree: &[Voxel], steps: &[Step]) -> Wire {
    let mut wire = Wire::default();
    for (index, step) in steps.iter().enumerate() {
        let Step { from, to } = *step;
        let flat = Voxel::new(to.x, from.y, to.z);
        match to.y - from.y {
            1 => wire.air.push(from.up()),
            -1 => wire.air.push(flat),
            _ => {}
        }
        if index == 0 && tree.contains(&from) && lone(grid, from) {
            let face = Grid::toward(from, flat).unwrap_or(cairn_lang_core::block_array::Face::East);
            let behind = from.step(face.opposite());
            if grid.get(behind).is_some() {
                wire.air.push(behind);
            }
        }
        if index + 1 < steps.len() {
            wire.dust.push(to);
            if !grid.conducts(to.down()) && to.down().y >= 1 {
                wire.supports.push(to.down());
            }
        }
    }
    wire
}

fn unroutable(scope: &str, layout: &CircuitLayout, net: usize) -> Diagnostic {
    let circuit = &layout.nets[net.min(layout.nets.len().saturating_sub(1))];
    let driver = &layout.tiles[circuit.driver];
    let what = match driver.kind {
        TileKind::Cell { cell } => format!("the `{}` cell's output", cell.name()),
        TileKind::Pad { .. } => "the sensor's signal".to_owned(),
        TileKind::Riser => "the riser's signal".to_owned(),
    };
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        driver.span.clone(),
        format!(
            "{scope} {what} finds no way to every input it feeds through the reservation (region {width}x{depth}, void={void}) without running beside another signal's dust",
            width = layout.region.width,
            depth = layout.region.depth,
            void = layout.region.void,
        ),
        CONGESTION_FIX,
    )
}

#[cfg(test)]
mod tests;
