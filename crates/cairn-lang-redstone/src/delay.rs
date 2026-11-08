//! Stage 3 of `spec/redstone` "Place-and-route": measures the strength each net's dust carries to
//! every input it feeds, from each part that drives it alone, and puts a repeater in the dust
//! wherever an input would read less than it needs.

use std::collections::{BinaryHeap, HashMap, HashSet};

use cairn_lang_core::block_array::Face;

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::geometry::{SIDES, Voxel};
use crate::grid::{Grid, Label, Slot};
use crate::part::Part;
use crate::placement_ir::{Buffer, CircuitLayout, LayoutStage, ScopedCircuitLayout, TileKind};
use crate::tile::Strength;

/// Tick delay of one buffer repeater: a repeater at its shortest delay.
pub const BUFFER_REPEATER_TICKS: u32 = 1;

/// The most blocks of dust a full-strength signal runs before it reads nothing.
pub const DUST_ATTENUATION_LIMIT: u32 = 15;

/// Output of a [`compile_delay`] run: the delayed circuits and the findings.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct DelayOutput {
    /// Every circuit delay insertion kept.
    pub scoped: ScopedCircuitLayout,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Puts the buffer repeaters every circuit of `routed` needs, leaving out one where some input
/// cannot be reached at the strength it needs.
#[must_use]
pub fn compile_delay(routed: &ScopedCircuitLayout) -> DelayOutput {
    let mut out = DelayOutput::default();
    for entry in &routed.scopes {
        let scope = format!("{}={}:", entry.kind.label(), entry.name);
        match buffer_layout(&scope, &entry.ir) {
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

fn buffer_layout(scope: &str, routed: &CircuitLayout) -> Result<CircuitLayout, Diagnostic> {
    let mut layout = routed.clone();
    let mut grid = Grid::of(&layout).map_err(|_| unreachable_input(scope, &layout, 0, 0))?;
    let mut ports: HashMap<(usize, usize), u8> = HashMap::new();
    for net in 0..layout.nets.len() {
        loop {
            let graph = NetGraph::of(&grid, &layout, net);
            let mut failing = None;
            let reaches: Vec<Reach> = sources(&layout, net, &ports)
                .iter()
                .map(|group| graph.reach(group))
                .collect();
            for (sink_index, sink) in layout.nets[net].sinks.iter().enumerate() {
                let tile = &layout.tiles[sink.tile];
                let Some(port) = tile.kind.template().port(sink.port).copied() else {
                    continue;
                };
                let at = tile.at(port.at);
                let mut weakest = u8::MAX;
                for (group, reach) in reaches.iter().enumerate() {
                    let level = reach.power.get(&at).copied().unwrap_or(0);
                    weakest = weakest.min(level);
                    if level < port.need && failing.is_none() {
                        failing = Some((sink_index, at, group));
                    }
                }
                ports.insert(
                    (sink.tile, sink_index_of(&layout, sink.tile, sink.port)),
                    weakest,
                );
            }
            let Some((sink_index, at, group)) = failing else {
                break;
            };
            let Some(buffer) = site(&grid, &graph, &reaches[group], at) else {
                return Err(unreachable_input(scope, &layout, net, sink_index));
            };
            grid.lay_buffer(buffer);
            layout.nets[net].wire.buffers.push(buffer);
        }
    }
    for net in &mut layout.nets {
        net.wire.buffers.sort_by_key(|b| b.at);
    }
    layout.stage = LayoutStage::Delayed;
    Ok(layout)
}

/// The index among a tile's inputs of the one taking `port`.
fn sink_index_of(layout: &CircuitLayout, tile: usize, port: crate::tile::TilePort) -> usize {
    layout.tiles[tile]
        .inputs
        .iter()
        .position(|input| input.port == port)
        .unwrap_or(0)
}

/// The dust each part driving `net` powers on its own, and the strength it holds there.
fn sources(
    layout: &CircuitLayout,
    net: usize,
    ports: &HashMap<(usize, usize), u8>,
) -> Vec<Vec<(Voxel, u8)>> {
    let driver = &layout.tiles[layout.nets[net].driver];
    let template = driver.kind.template();
    let level = match template.strength {
        Some(Strength::Rear(index)) => template
            .ports
            .get(index)
            .and_then(|port| {
                let input = driver.inputs.iter().position(|i| i.port == port.port)?;
                ports.get(&(layout.nets[net].driver, input)).copied()
            })
            .unwrap_or(0),
        _ => 15,
    };
    template
        .groups
        .iter()
        .map(|group| group.iter().map(|rel| (driver.at(*rel), level)).collect())
        .collect()
}

/// The voxel nearest `at`, on its way back to what last restored the signal, where a repeater
/// still reads power: the most one repeater can lift `at` by.
fn site(grid: &Grid, graph: &NetGraph, reach: &Reach, at: Voxel) -> Option<Buffer> {
    let mut path = vec![at];
    let mut cursor = at;
    while let Some(&from) = reach.came.get(&cursor) {
        path.push(from);
        cursor = from;
        if graph.restorers.contains(&from) {
            break;
        }
    }
    path.windows(3).find_map(|window| {
        let (next, here, prev) = (window[0], window[1], window[2]);
        let face = Grid::toward(prev, here)?;
        let straight = here.step(face) == next && prev.y == here.y && next.y == here.y;
        let fits = straight
            && graph.wire.contains(&here)
            && graph.links_of(here) == 2
            && reach.power.get(&prev).copied().unwrap_or(0) >= 1
            && !grid.side_driven(here, face);
        fits.then_some(Buffer {
            at: here,
            facing: face,
        })
    })
}

/// One net's dust as power runs along it.
pub(crate) struct NetGraph {
    /// Each dust voxel of the net and the dust it links to.
    links: HashMap<Voxel, Vec<Voxel>>,
    /// The net's repeaters, by the dust each reads: its voxel and the dust it drives.
    buffers: HashMap<Voxel, Vec<(Voxel, Voxel)>>,
    /// The voxels a repeater drives.
    restorers: HashSet<Voxel>,
    /// The wire's own dust, where a repeater may go.
    wire: HashSet<Voxel>,
}

/// How strong one source group leaves each voxel of a net, and the voxel each was reached from.
pub(crate) struct Reach {
    pub(crate) power: HashMap<Voxel, u8>,
    pub(crate) came: HashMap<Voxel, Voxel>,
}

impl NetGraph {
    /// The dust of `net` on `grid`.
    pub(crate) fn of(grid: &Grid, layout: &CircuitLayout, net: usize) -> Self {
        let mine = |at: Voxel| grid.get(at) == Some(Slot::Dust(Label::Net(net)));
        let mut dust: Vec<Voxel> = layout.nets[net].wire.dust.clone();
        for tile in &layout.tiles {
            let template = tile.kind.template();
            for (rel, part) in &template.parts {
                let at = tile.at(*rel);
                if *part == Part::Dust && mine(at) {
                    dust.push(at);
                }
            }
        }
        dust.sort_unstable();
        dust.dedup();
        let mut links: HashMap<Voxel, Vec<Voxel>> = HashMap::new();
        for &at in &dust {
            if !mine(at) {
                continue;
            }
            let mut joined = Vec::new();
            for face in SIDES {
                let beside = at.step(face);
                if mine(beside) {
                    joined.push(beside);
                }
                if mine(beside.up()) && grid.conducts(beside) && !grid.conducts(at.up()) {
                    joined.push(beside.up());
                }
                if mine(beside.down()) && !grid.conducts(beside) {
                    joined.push(beside.down());
                }
            }
            links.insert(at, joined);
        }
        let mut buffers: HashMap<Voxel, Vec<(Voxel, Voxel)>> = HashMap::new();
        let mut restorers = HashSet::new();
        for buffer in &layout.nets[net].wire.buffers {
            let rear = buffer.at.step(buffer.facing.opposite());
            let front = buffer.at.step(buffer.facing);
            buffers.entry(rear).or_default().push((buffer.at, front));
            restorers.insert(front);
        }
        let wire = layout.nets[net]
            .wire
            .dust
            .iter()
            .copied()
            .filter(|at| mine(*at))
            .collect();
        Self {
            links,
            buffers,
            restorers,
            wire,
        }
    }

    fn links_of(&self, at: Voxel) -> usize {
        self.links.get(&at).map_or(0, Vec::len)
    }

    /// Each repeater reading the dust at `at`, with the dust it drives.
    fn ahead(&self, at: Voxel) -> impl Iterator<Item = (Voxel, Voxel)> + '_ {
        self.buffers.get(&at).into_iter().flatten().copied()
    }

    /// How strong `sources` leave each voxel, every repeater whose dust reads on driving its own,
    /// and the way each voxel is reached in the fewest links, however weak.
    pub(crate) fn reach(&self, sources: &[(Voxel, u8)]) -> Reach {
        let mut power: HashMap<Voxel, u8> = HashMap::new();
        let came = self.paths(sources);
        let mut queue: BinaryHeap<(u8, Voxel)> = BinaryHeap::new();
        for &(at, level) in sources {
            let level = level.min(15);
            if self.links.contains_key(&at) && power.get(&at).is_none_or(|p| *p < level) {
                power.insert(at, level);
                queue.push((level, at));
            }
        }
        while let Some((level, at)) = queue.pop() {
            if power.get(&at).copied() != Some(level) {
                continue;
            }
            let mut reached: Vec<(Voxel, u8)> = self
                .links
                .get(&at)
                .into_iter()
                .flatten()
                .map(|next| (*next, level - 1))
                .collect();
            reached.extend(self.ahead(at).map(|(_, front)| (front, 15)));
            for (next, strength) in reached {
                if strength > 0 && power.get(&next).is_none_or(|p| *p < strength) {
                    power.insert(next, strength);
                    queue.push((strength, next));
                }
            }
        }
        Reach { power, came }
    }

    /// The voxel each voxel is first reached from, link by link out of `sources`, a repeater
    /// passed through from the dust it reads to the dust it drives.
    fn paths(&self, sources: &[(Voxel, u8)]) -> HashMap<Voxel, Voxel> {
        let mut came: HashMap<Voxel, Voxel> = HashMap::new();
        let mut seen: HashSet<Voxel> = sources.iter().map(|(at, _)| *at).collect();
        let mut queue: std::collections::VecDeque<Voxel> =
            sources.iter().map(|(at, _)| *at).collect();
        while let Some(at) = queue.pop_front() {
            let mut next: Vec<(Voxel, Option<Voxel>)> = self
                .links
                .get(&at)
                .into_iter()
                .flatten()
                .map(|n| (*n, None))
                .collect();
            next.extend(self.ahead(at).map(|(buffer, front)| (front, Some(buffer))));
            for (to, through) in next {
                if !seen.insert(to) {
                    continue;
                }
                match through {
                    Some(buffer) => {
                        came.insert(buffer, at);
                        came.insert(to, buffer);
                    }
                    None => {
                        came.insert(to, at);
                    }
                }
                queue.push_back(to);
            }
        }
        came
    }
}

impl Grid {
    /// Lays a buffer repeater over the dust at its voxel.
    pub(crate) fn lay_buffer(&mut self, buffer: Buffer) {
        self.replace(
            buffer.at,
            Slot::Part(Part::Repeater {
                facing: buffer.facing,
                delay: 1,
            }),
        );
    }

    /// Whether a diode drives into `at` from a side of a repeater driving `facing` there.
    fn side_driven(&self, at: Voxel, facing: Face) -> bool {
        SIDES
            .into_iter()
            .filter(|face| *face != facing && *face != facing.opposite())
            .any(|face| {
                let from = at.step(face);
                matches!(self.get(from), Some(Slot::Part(part)) if part.drives_into(from, at))
            })
    }
}

fn unreachable_input(scope: &str, layout: &CircuitLayout, net: usize, sink: usize) -> Diagnostic {
    let Some(circuit) = layout.nets.get(net) else {
        return error_with_footer(
            DiagnosticCode::AttenuationLimit,
            layout.region.span.clone(),
            format!("{scope} the circuit's dust cannot be laid as routed"),
            "Fix: widen the struct's `size=WxH` or increase `void`",
        );
    };
    let target = circuit
        .sinks
        .get(sink)
        .map_or(circuit.driver, |sink| sink.tile);
    let tile = &layout.tiles[target];
    let what = match tile.kind {
        TileKind::Cell { cell } => format!("this `{}` cell", cell.name()),
        TileKind::Pad { .. } => "this sensor's pad".to_owned(),
        TileKind::Riser => "the riser under this actuator".to_owned(),
    };
    error_with_footer(
        DiagnosticCode::AttenuationLimit,
        tile.span.clone(),
        format!(
            "{scope} {what} reads its signal too weak: the dust runs past {DUST_ATTENUATION_LIMIT} blocks with no straight stretch where a buffer repeater can restore it"
        ),
        "Fix: widen the struct's `size=WxH` or increase `void`, so the wire has room to run straight, or move what feeds it nearer",
    )
}

#[cfg(test)]
mod tests;
