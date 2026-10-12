//! Rectilinear geometry shared by stages 2 to 4 of `spec/redstone` "Place-and-route": the net
//! order, and the one answer to where each net's dust runs. A net goes round the blocks and round,
//! or over, the dust of every net laid before it, so its tree depends on [`net_order`].

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

use cairn_lang_core::check::Severity;

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::grid::{
    MAX_ATTENUATION_SEGMENT, PadColumn, STEPS, beside, coord_key, faces, input_pad, manhattan,
    net_ref_key, stepped, under,
};
use crate::netlist_ir::{CellPortDriver, NetRef};
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIrEntry,
};

/// Folds `charge` over the distinct nets driving one cell: two ports on one net share one strand.
pub(crate) fn sum_over_driving_nets<F>(drivers: &[CellPortDriver], mut charge: F) -> u32
where
    F: FnMut(NetRef) -> u32,
{
    let mut seen: Vec<NetRef> = Vec::with_capacity(drivers.len());
    let mut total = 0u32;
    for driver in drivers {
        if seen.contains(&driver.net) {
            continue;
        }
        seen.push(driver.net);
        total = total.saturating_add(charge(driver.net));
    }
    total
}

/// What kind of component stands on a reserved coord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockKind {
    /// A placed cell's body.
    Cell,
    /// A sensor's pad on the left edge.
    InputPad,
    /// An actuator's pad on the right edge.
    OutputPad,
}

impl BlockKind {
    /// The word a diagnostic uses for this kind of block.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Cell => "cell",
            Self::InputPad => "input",
            Self::OutputPad => "output",
        }
    }
}

/// One block inside a scope's reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockSite {
    /// The coord the block occupies.
    pub(crate) coord: CellCoord,
    /// What stands there.
    pub(crate) kind: BlockKind,
    /// Index among blocks of the same kind, for the diagnostic that names it.
    pub(crate) index: usize,
}

/// Every block in a scope: cells, then input pads, then actuator pads, the order
/// [`collapsed_block`] relies on.
pub(crate) fn block_sites(ir: &PlacementIr, region: &CircuitRegionReservation) -> Vec<BlockSite> {
    let mut sites = Vec::with_capacity(ir.cells.len() + ir.inputs.len() + ir.outputs.len());
    for (index, cell) in ir.cells.iter().enumerate() {
        sites.push(BlockSite {
            coord: keyed(cell.coord),
            kind: BlockKind::Cell,
            index,
        });
    }
    let column = PadColumn::of(ir);
    for index in 0..ir.inputs.len() {
        sites.push(BlockSite {
            coord: input_pad(index, column, region),
            kind: BlockKind::InputPad,
            index,
        });
    }
    for (index, output) in ir.outputs.iter().enumerate() {
        sites.push(BlockSite {
            coord: keyed(output.pad),
            kind: BlockKind::OutputPad,
            index,
        });
    }
    sites
}

/// The same voxel keyed as [`CellCoord::new`] keys it, which a struct literal may not be.
fn keyed(coord: CellCoord) -> CellCoord {
    CellCoord::new(coord.x, coord.y, coord.z)
}

/// One coord on the search frontier; [`BinaryHeap`]'s max is the one to expand next: least `f`,
/// then largest `g`, earliest step, smallest coord. `PartialEq` follows that key, not `layer`.
#[derive(Debug, Eq)]
struct Frontier {
    f: u32,
    g: u32,
    step: usize,
    coord: CellCoord,
}

impl Ord for Frontier {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f
            .cmp(&self.f)
            .then_with(|| self.g.cmp(&other.g))
            .then_with(|| other.step.cmp(&self.step))
            .then_with(|| coord_key(other.coord).cmp(&coord_key(self.coord)))
    }
}

impl PartialOrd for Frontier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Frontier {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

/// The blocks standing in one scope's reservation, and its extent.
pub(crate) struct Router {
    width: u32,
    height: u32,
    depth: u32,
    blocked: HashSet<CellCoord>,
}

impl Router {
    /// A router over `region` with `blocks` standing in it.
    pub(crate) fn new(region: &CircuitRegionReservation, blocks: &[BlockSite]) -> Self {
        Self {
            width: region.width,
            height: region.void,
            depth: region.depth,
            blocked: blocks.iter().map(|site| keyed(site.coord)).collect(),
        }
    }

    fn inside(&self, coord: CellCoord) -> bool {
        coord.x < self.width && coord.y < self.height && coord.z < self.depth
    }

    /// Whether dust may be drawn on `coord` while `keep_out` stands.
    fn free(&self, coord: CellCoord, keep_out: &HashSet<CellCoord>) -> bool {
        self.inside(coord) && !self.blocked.contains(&coord) && !keep_out.contains(&coord)
    }

    /// The coord one [`STEPS`] step away, or `None` when the step leaves the reservation.
    fn step(&self, from: CellCoord, delta: (i64, i64, i64)) -> Option<CellCoord> {
        let next = stepped(from, delta)?;
        self.inside(next).then_some(next)
    }

    /// The cheapest block-free path from any coord in `seeds` to the nearest in `targets`, as
    /// `(index into targets, path)`, both ends included.
    fn reach(
        &self,
        seeds: &[CellCoord],
        targets: &[CellCoord],
        keep_out: &HashSet<CellCoord>,
    ) -> Result<(usize, Vec<CellCoord>), GaveUp> {
        match self.straight_run(seeds, targets, keep_out) {
            Some(found) => Ok(found),
            None => self.search(seeds, targets, keep_out).0,
        }
    }

    /// The closest seed/target pair, when the straight line between them is clear.
    fn straight_run(
        &self,
        seeds: &[CellCoord],
        targets: &[CellCoord],
        keep_out: &HashSet<CellCoord>,
    ) -> Option<(usize, Vec<CellCoord>)> {
        let (index, seed) = seeds
            .iter()
            .flat_map(|seed| {
                targets
                    .iter()
                    .enumerate()
                    .map(move |(index, target)| (index, *seed, *target))
            })
            .filter(|(_, _, target)| self.inside(*target))
            .min_by_key(|(_, seed, target)| {
                (
                    manhattan(*seed, *target),
                    coord_key(*target),
                    coord_key(*seed),
                )
            })
            .map(|(index, seed, _)| (index, seed))?;
        let target = targets[index];
        let mut path = vec![seed];
        let mut current = seed;
        while current != target {
            current = step_towards(current, target);
            if current != target && !self.free(current, keep_out) {
                return None;
            }
            path.push(current);
        }
        Some((index, path))
    }

    /// A* from `seeds` to the nearest of `targets`, never through one, pruned past
    /// [`MAX_ATTENUATION_SEGMENT`]; also returns how many coords it settled.
    fn search(
        &self,
        seeds: &[CellCoord],
        targets: &[CellCoord],
        keep_out: &HashSet<CellCoord>,
    ) -> (Result<(usize, Vec<CellCoord>), GaveUp>, usize) {
        let arrivable = |target: &CellCoord| {
            faces(*target).any(|face| seeds.contains(&face) || self.free(face, keep_out))
        };
        if !targets.iter().any(arrivable) {
            return (Err(GaveUp::Exhausted), 0);
        }
        let heuristic = |coord: CellCoord| -> u32 {
            targets
                .iter()
                .map(|target| manhattan(coord, *target))
                .min()
                .unwrap_or(0)
        };

        let mut cheapest: HashMap<CellCoord, u32> = HashMap::new();
        let mut parent: HashMap<CellCoord, CellCoord> = HashMap::new();
        let mut frontier: BinaryHeap<Frontier> = BinaryHeap::new();
        let mut pruned = false;
        let mut settled = 0_usize;
        for seed in seeds {
            if cheapest.insert(*seed, 0).is_some() {
                continue;
            }
            frontier.push(Frontier {
                f: heuristic(*seed),
                g: 0,
                step: STEPS.len(),
                coord: *seed,
            });
        }

        while let Some(current) = frontier.pop() {
            if cheapest
                .get(&current.coord)
                .is_some_and(|best| *best < current.g)
            {
                // Stale: a cheaper way here was pushed later.
                continue;
            }
            settled += 1;
            if let Some(index) = targets.iter().position(|t| *t == current.coord) {
                return (Ok((index, walk_back(current.coord, &parent))), settled);
            }
            for (step, delta) in STEPS.iter().enumerate() {
                let Some(next) = self.step(current.coord, *delta) else {
                    continue;
                };
                // A target is asked first: a cell body is both a block and a terminal.
                let target = targets.contains(&next);
                if !target && !self.free(next, keep_out) {
                    continue;
                }
                let g = current.g.saturating_add(1);
                if cheapest.get(&next).is_some_and(|best| *best <= g) {
                    continue;
                }
                let f = g.saturating_add(heuristic(next));
                if f > MAX_ATTENUATION_SEGMENT {
                    pruned = true;
                    continue;
                }
                cheapest.insert(next, g);
                parent.insert(next, current.coord);
                frontier.push(Frontier {
                    f,
                    g,
                    step,
                    coord: next,
                });
            }
        }
        let gave_up = if pruned {
            GaveUp::Pruned
        } else {
            GaveUp::Exhausted
        };
        (Err(gave_up), settled)
    }

    /// Why no path within [`MAX_ATTENUATION_SEGMENT`] from `seeds` reaches `sink` after a pruned
    /// search, found by a flood out of `sink`, and how many coords the flood visited.
    fn unreached(
        &self,
        sink: CellCoord,
        seeds: &[CellCoord],
        keep_out: &HashSet<CellCoord>,
    ) -> (Unreached, usize) {
        if !self.inside(sink) {
            return (Unreached::WalledIn, 1);
        }
        let seeds: HashSet<CellCoord> = seeds.iter().copied().collect();
        let mut seen: HashSet<CellCoord> = HashSet::from([sink]);
        let mut queue: VecDeque<(CellCoord, u32)> = VecDeque::from([(sink, 0)]);
        let mut at_cap = false;
        while let Some((coord, distance)) = queue.pop_front() {
            for face in faces(coord) {
                if seen.contains(&face) {
                    continue;
                }
                let tree = seeds.contains(&face);
                if !tree && !self.free(face, keep_out) {
                    continue;
                }
                if distance >= MAX_ATTENUATION_SEGMENT {
                    at_cap = true;
                    continue;
                }
                debug_assert!(
                    !tree,
                    "the search gave up on a sink {} blocks from the tree, within the cap",
                    distance + 1,
                );
                if tree {
                    at_cap = true;
                    continue;
                }
                seen.insert(face);
                queue.push_back((face, distance + 1));
            }
        }
        let why = if at_cap {
            Unreached::BeyondCap
        } else {
            Unreached::WalledIn
        };
        (why, seen.len())
    }

    /// One net's routed tree: rooted at `source`, every distinct sink attached as a leaf, nearest
    /// first, round the blocks and `keep_out`.
    pub(crate) fn tree(
        &self,
        source: CellCoord,
        sinks: &[CellCoord],
        keep_out: &HashSet<CellCoord>,
    ) -> NetTree {
        debug_assert!(
            sinks
                .iter()
                .all(|sink| self.blocked.contains(&keyed(*sink))),
            "a sink must be a block of this router: dust ends at what consumes it",
        );
        let source = keyed(source);
        let mut tree = NetTree::rooted(source);
        let mut emitters = vec![source];
        let mut remaining: Vec<CellCoord> = Vec::new();
        for sink in sinks.iter().copied().map(keyed) {
            if sink != source && !remaining.contains(&sink) {
                remaining.push(sink);
            }
        }
        while !remaining.is_empty() {
            let (index, path) = match self.reach(&emitters, &remaining, keep_out) {
                Ok(found) => found,
                Err(gave_up) => {
                    for sink in remaining.drain(..) {
                        let why = match gave_up {
                            GaveUp::Exhausted => Unreached::WalledIn,
                            GaveUp::Pruned => self.unreached(sink, &emitters, keep_out).0,
                        };
                        tree.strand(sink, why);
                    }
                    break;
                }
            };
            remaining.remove(index);
            for pair in path.windows(2) {
                tree.attach(pair[1], pair[0]);
            }
            // Not the far end: the sink consumes the signal.
            emitters.extend(path[1..path.len().saturating_sub(1)].iter().copied());
        }
        tree
    }

    /// The coords of `tree` that are dust: all but its terminals, where another net may arrive.
    pub(crate) fn dust(&self, tree: &NetTree) -> Vec<CellCoord> {
        tree.wire_path()
            .into_iter()
            .filter(|coord| !self.blocked.contains(coord))
            .collect()
    }
}

/// One step from `from` towards `to`, along x, then z, then y, as [`STEPS`] orders the search.
fn step_towards(from: CellCoord, to: CellCoord) -> CellCoord {
    let mut next = from;
    if from.x != to.x {
        next.x = toward(from.x, to.x);
    } else if from.z != to.z {
        next.z = toward(from.z, to.z);
    } else if from.y != to.y {
        next.y = toward(from.y, to.y);
    }
    CellCoord::new(next.x, next.y, next.z)
}

/// `a` moved one towards `b`; `a` itself when they are equal.
fn toward(a: u32, b: u32) -> u32 {
    match a.cmp(&b) {
        Ordering::Less => a + 1,
        Ordering::Greater => a - 1,
        Ordering::Equal => a,
    }
}

/// The coords from a search root to `coord`, both ends included.
fn walk_back(coord: CellCoord, parent: &HashMap<CellCoord, CellCoord>) -> Vec<CellCoord> {
    let mut path = vec![coord];
    let mut current = coord;
    while let Some(previous) = parent.get(&current) {
        path.push(*previous);
        current = *previous;
    }
    path.reverse();
    path
}

/// What a search that reached none of its targets knows about all of them at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GaveUp {
    /// The cap pruned nothing, so every target is walled in.
    Exhausted,
    /// The cap pruned at least one coord, which says nothing about any one target.
    Pruned,
}

/// Why one sink was not reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unreached {
    /// No path of any length reaches the sink.
    WalledIn,
    /// No path within [`MAX_ATTENUATION_SEGMENT`] reaches it; a longer one is not looked for.
    BeyondCap,
}

/// One net's routed dust, each coord with the coord the signal reached it from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetTree {
    order: Vec<CellCoord>,
    parent: HashMap<CellCoord, CellCoord>,
    unreachable: Vec<CellCoord>,
    /// Why each sink of [`Self::unreachable`] is there.
    stranded_by: HashMap<CellCoord, Unreached>,
}

impl NetTree {
    /// A tree holding only its source.
    fn rooted(source: CellCoord) -> Self {
        Self {
            order: vec![source],
            parent: HashMap::new(),
            unreachable: Vec::new(),
            stranded_by: HashMap::new(),
        }
    }

    /// Records `coord` as reached from `from`.
    fn attach(&mut self, coord: CellCoord, from: CellCoord) {
        debug_assert!(
            !self.parent.contains_key(&coord) && coord != self.order[0],
            "the search returns a path of coords the tree does not hold yet",
        );
        debug_assert!(
            from == self.order[0] || self.parent.contains_key(&from),
            "a path grows out of a coord the tree already holds",
        );
        self.parent.insert(coord, from);
        self.order.push(coord);
    }

    /// Records a sink with no clear path, parented straight to the source so the tree stays total.
    fn strand(&mut self, sink: CellCoord, why: Unreached) {
        let source = self.order[0];
        self.parent.insert(sink, source);
        self.order.push(sink);
        self.unreachable.push(sink);
        self.stranded_by.insert(sink, why);
    }

    /// Every coord the net occupies, source first, then in laying order, each once.
    pub(crate) fn wire_path(&self) -> Vec<CellCoord> {
        self.order.clone()
    }

    /// The dust from the source to `sink`, one block per step but for a stranded sink; `None` when
    /// `sink` is not on this net.
    pub(crate) fn route_to(&self, sink: CellCoord) -> Option<Vec<CellCoord>> {
        let sink = keyed(sink);
        if sink != self.order[0] && !self.parent.contains_key(&sink) {
            return None;
        }
        let mut route = vec![sink];
        let mut current = sink;
        while let Some(previous) = self.parent.get(&current) {
            // `attach`'s check is debug-only; this turns a parent cycle into a panic, not a hang.
            assert!(
                route.len() <= self.order.len(),
                "parent links cycle at ({x},{y},{z}) — a coord was attached twice",
                x = current.x,
                y = current.y,
                z = current.z,
            );
            route.push(*previous);
            current = *previous;
        }
        route.reverse();
        Some(route)
    }

    /// The coord the signal reaches `coord` from: `None` for the source or a coord off this net.
    pub(crate) fn parent(&self, coord: CellCoord) -> Option<CellCoord> {
        self.parent.get(&keyed(coord)).copied()
    }

    /// A tree grown along `paths` by hand, for a test that pins a shape coord by coord.
    #[cfg(test)]
    pub(crate) fn from_paths(paths: &[&[CellCoord]]) -> Self {
        let mut tree = Self::rooted(keyed(paths[0][0]));
        for path in paths {
            for pair in path.windows(2) {
                debug_assert_eq!(
                    manhattan(pair[0], pair[1]),
                    1,
                    "a hand-built path moves one block per step",
                );
                tree.attach(keyed(pair[1]), keyed(pair[0]));
            }
        }
        tree
    }

    /// The sinks with no clear path, in the order they were asked for.
    pub(crate) fn unreachable(&self) -> &[CellCoord] {
        &self.unreachable
    }

    /// Why `sink` is one of [`Self::unreachable`], or `None` when it is not one.
    pub(crate) fn stranded_by(&self, sink: CellCoord) -> Option<Unreached> {
        self.stranded_by.get(&sink).copied()
    }
}

/// Every net in a scope: driver → its distinct sinks, a cell body or an actuator pad; [`net_order`]
/// reads their count as fanout.
pub(crate) fn collect_nets(ir: &PlacementIr) -> HashMap<NetRef, Vec<CellCoord>> {
    let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    let mut sink = |net: NetRef, coord: CellCoord| {
        let sinks = nets.entry(net).or_default();
        if !sinks.contains(&coord) {
            sinks.push(coord);
        }
    };
    for cell in &ir.cells {
        for driver in &cell.drivers {
            sink(driver.net, cell.coord);
        }
    }
    for output in &ir.outputs {
        sink(output.driver, output.pad);
    }
    nets
}

/// The order the nets of a scope are laid in, which decides their trees: fanout descending, then
/// [`net_ref_key`].
pub(crate) fn net_order(nets: &HashMap<NetRef, Vec<CellCoord>>) -> Vec<NetRef> {
    let mut order: Vec<NetRef> = nets.keys().copied().collect();
    order.sort_by(|a, b| {
        nets[b]
            .len()
            .cmp(&nets[a].len())
            .then_with(|| net_ref_key(*a).cmp(&net_ref_key(*b)))
    });
    order
}

/// The first block on a coord another block already holds: a pad the reservation is too small for.
///
/// # Panics
///
/// When two cells share a coord, which only a wrongly assembled IR can do.
pub(crate) fn collapsed_block(blocks: &[BlockSite]) -> Option<&BlockSite> {
    let mut seen: HashSet<CellCoord> = HashSet::with_capacity(blocks.len());
    blocks.iter().find(|site| {
        if seen.insert(keyed(site.coord)) {
            return false;
        }
        assert!(
            site.kind != BlockKind::Cell,
            "two cells stand on ({x},{y},{z}) — a cell's coord is derived from its topological \
             index by the placement pass, so a repeat is an IR the caller assembled itself rather \
             than a reservation too small for its pad row. There is no `size=` that repairs it and \
             so no diagnostic to raise",
            x = site.coord.x,
            y = site.coord.y,
            z = site.coord.z,
        );
        true
    })
}

/// The refusal for a sink the reservation cannot wire, anchored on a walled-in one first, or `None`
/// when every sink is wired.
pub(crate) fn unroutable<F>(
    nets: &HashMap<NetRef, Vec<CellCoord>>,
    trees: &HashMap<NetRef, NetTree>,
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    region: &CircuitRegionReservation,
    source_of_net: F,
) -> Option<Diagnostic>
where
    F: Fn(NetRef) -> CellCoord,
{
    let order = net_order(nets);
    let stranded: Vec<(NetRef, CellCoord)> = order
        .iter()
        .flat_map(|net| trees[net].unreachable().iter().map(|sink| (*net, *sink)))
        .collect();
    let (net, sink) = stranded
        .iter()
        .find(|(net, sink)| trees[net].stranded_by(*sink) == Some(Unreached::WalledIn))
        .or_else(|| stranded.first())
        .copied()?;
    let source = source_of_net(net);
    let crowding = crowding_nets(nets, trees, &source_of_net, net, sink);
    if trees[&net].stranded_by(sink) == Some(Unreached::BeyondCap) {
        return Some(beyond_cap_diagnostic(
            entry,
            netlist,
            region,
            (source, sink),
            &crowding,
            stranded.len(),
        ));
    }
    let mut primary = format!(
        "{netlist} netlist for {kind} `{name}` cannot reach ({x},{y},{z}) from the driver at ({sx},{sy},{sz}) — every route between them is blocked by a cell body, an I/O pad, or another net's dust, on the coord or one step from it in the same plane, and a wire passes through none of the three",
        kind = entry.kind.label(),
        name = entry.name,
        x = sink.x,
        y = sink.y,
        z = sink.z,
        sx = source.x,
        sy = source.y,
        sz = source.z,
    );
    crowded_and_the_rest(&mut primary, entry, &crowding, stranded.len());
    Some(error_with_footer(
        DiagnosticCode::RouteCongestion,
        region.span.clone(),
        primary,
        format!(
            "Fix: raise `void` above {void} so the wire has a layer to climb onto, enlarge `size=WxH`, or split the logic across several scopes, each with its own `circuit` line",
            void = region.void,
        ),
    ))
}

/// The crowding clause and the count of other stranded sinks both of [`unroutable`]'s refusals end
/// with.
fn crowded_and_the_rest(
    primary: &mut String,
    entry: &ScopedPlacementIrEntry,
    crowding: &[NetRef],
    stranded: usize,
) {
    if !crowding.is_empty() {
        write!(
            primary,
            "; the faces it could arrive through are taken by {names}",
            names = crowding
                .iter()
                .map(|net| net_label(*net, &entry.ir))
                .collect::<Vec<_>>()
                .join(" and "),
        )
        .expect("writing to a String cannot fail");
    }
    if stranded > 1 {
        write!(
            primary,
            "; {rest} more of this scope's sinks cannot be reached either",
            rest = stranded - 1,
        )
        .expect("writing to a String cannot fail");
    }
}

/// The `E_ATTENUATION_LIMIT` refusal for a sink [`Unreached::BeyondCap`] stranded.
fn beyond_cap_diagnostic(
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    region: &CircuitRegionReservation,
    (source, sink): (CellCoord, CellCoord),
    crowding: &[NetRef],
    stranded: usize,
) -> Diagnostic {
    let mut primary = format!(
        "{netlist} netlist for {kind} `{name}` has no route from the driver at ({sx},{sy},{sz}) to ({x},{y},{z}) within the v1 attenuation limit of {cap} steps",
        kind = entry.kind.label(),
        name = entry.name,
        x = sink.x,
        y = sink.y,
        z = sink.z,
        sx = source.x,
        sy = source.y,
        sz = source.z,
        cap = MAX_ATTENUATION_SEGMENT,
    );
    crowded_and_the_rest(&mut primary, entry, crowding, stranded);
    error_with_footer(
        DiagnosticCode::AttenuationLimit,
        region.span.clone(),
        primary,
        format!(
            "Fix: give the wire a shorter way round — raise `void` above {void} so it has a layer to climb onto, or enlarge `region=` — or split the logic across several scopes, each with its own `circuit` line",
            void = region.void,
        ),
    )
}

/// The other nets whose dust on or [`beside`] a face of `sink` keeps a wire out, in [`net_order`].
fn crowding_nets<F>(
    nets: &HashMap<NetRef, Vec<CellCoord>>,
    trees: &HashMap<NetRef, NetTree>,
    source_of_net: &F,
    stranded: NetRef,
    sink: CellCoord,
) -> Vec<NetRef>
where
    F: Fn(NetRef) -> CellCoord,
{
    let mut terminals: HashSet<CellCoord> = HashSet::new();
    for (net, sinks) in nets {
        terminals.insert(keyed(source_of_net(*net)));
        terminals.extend(sinks.iter().copied().map(keyed));
    }
    let taken: HashSet<CellCoord> = faces(sink)
        .flat_map(|face| std::iter::once(face).chain(beside(face)))
        .collect();
    net_order(nets)
        .into_iter()
        .filter(|net| *net != stranded)
        .filter(|net| {
            trees[net]
                .wire_path()
                .iter()
                .any(|coord| taken.contains(coord) && !terminals.contains(coord))
        })
        .collect()
}

/// A net's name in a message: the sensor's `sig.<name>`, or `cell #j`.
pub(crate) fn net_label(net: NetRef, ir: &PlacementIr) -> String {
    match net {
        NetRef::Input(i) => ir
            .inputs
            .get(i as usize)
            .map_or_else(|| format!("input pad #{i}"), |input| input.name.to_string()),
        NetRef::Cell(j) => format!("cell #{j}"),
    }
}

/// The routed tree of every net, laid in [`net_order`] round the dust of the ones before it and the
/// coords [`beside`] it.
pub(crate) fn net_trees<F>(
    nets: &HashMap<NetRef, Vec<CellCoord>>,
    router: &Router,
    source_of_net: F,
) -> HashMap<NetRef, NetTree>
where
    F: Fn(NetRef) -> CellCoord,
{
    let mut keep_out: HashSet<CellCoord> = HashSet::new();
    let mut trees: HashMap<NetRef, NetTree> = HashMap::with_capacity(nets.len());
    for net in net_order(nets) {
        let tree = router.tree(source_of_net(net), &nets[&net], &keep_out);
        let dust = router.dust(&tree);
        for coord in &dust {
            assert!(
                keep_out.insert(*coord),
                "{net:?} lays dust on ({x},{y},{z}), which another net of this \
                 scope already owns or runs beside — the router is asked to go \
                 round what is already laid and what it reaches, so this is the \
                 routing pass disagreeing with itself rather than anything the \
                 source can express",
                x = coord.x,
                y = coord.y,
                z = coord.z,
            );
        }
        // Widened only once the net is claimed: a strand runs beside itself.
        keep_out.extend(dust.into_iter().flat_map(beside));
        trees.insert(net, tree);
    }
    trees
}

/// Two strands of dust a layer apart and within one step, which the physical tile layer separates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TileLayerPair {
    /// The upper coord, and the net whose dust it is.
    pub(crate) over: (CellCoord, NetRef),
    /// The lower coord, and its net, never `over`'s.
    pub(crate) under: (CellCoord, NetRef),
}

impl TileLayerPair {
    /// Whether the two sit on one column, rather than one step across.
    fn stacked(self) -> bool {
        self.over.0.x == self.under.0.x && self.over.0.z == self.under.0.z
    }
}

/// Every pair of strands the physical tile layer has to separate, sorted by [`coord_key`] of the
/// upper coord, then the lower.
pub(crate) fn tile_layer_pairs(
    nets: &HashMap<NetRef, Vec<CellCoord>>,
    trees: &HashMap<NetRef, NetTree>,
    router: &Router,
) -> Vec<TileLayerPair> {
    let mut owner: HashMap<CellCoord, NetRef> = HashMap::new();
    for net in net_order(nets) {
        for coord in router.dust(&trees[&net]) {
            owner.insert(coord, net);
        }
    }
    let mut pairs: Vec<TileLayerPair> = Vec::new();
    for (over, over_net) in &owner {
        for below in under(*over) {
            match owner.get(&below) {
                Some(under_net) if under_net != over_net => pairs.push(TileLayerPair {
                    over: (*over, *over_net),
                    under: (below, *under_net),
                }),
                _ => {}
            }
        }
    }
    // The walk above is over a map, so its order must not reach a message.
    pairs.sort_by_key(|pair| (coord_key(pair.over.0), coord_key(pair.under.0)));
    pairs
}

/// How many pairs of each shape the finding names one by one.
const NAMED_PER_SHAPE: usize = 2;

/// The `W_ROUTE_CROSS_LAYER_CLEARANCE` advisory naming strands a layer apart, or `None` when there
/// are none.
pub(crate) fn tile_layer_clearance(
    nets: &HashMap<NetRef, Vec<CellCoord>>,
    trees: &HashMap<NetRef, NetTree>,
    router: &Router,
    entry: &ScopedPlacementIrEntry,
    region: &CircuitRegionReservation,
) -> Option<Diagnostic> {
    let pairs = tile_layer_pairs(nets, trees, router);
    if pairs.is_empty() {
        return None;
    }
    let stacked = pairs.iter().filter(|pair| pair.stacked()).count();
    let primary = format!(
        "routed netlist for {kind} `{name}` leaves {total} {pair} of dust within one step of each other across layers ({stacked} stacked, {staircase} staircase) — the router keeps two nets apart in one plane, and whether a strand reads the one a layer below it depends on what is standing between them",
        kind = entry.kind.label(),
        name = entry.name,
        total = pairs.len(),
        pair = if pairs.len() == 1 { "pair" } else { "pairs" },
        staircase = pairs.len() - stacked,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::RouteCrossLayerClearance,
        region.span.clone(),
        primary,
    );
    let named = pairs
        .iter()
        .filter(|pair| pair.stacked())
        .take(NAMED_PER_SHAPE)
        .chain(
            pairs
                .iter()
                .filter(|pair| !pair.stacked())
                .take(NAMED_PER_SHAPE),
        );
    let mut named_count = 0usize;
    for pair in named {
        named_count += 1;
        let (over, over_net) = pair.over;
        let (below, under_net) = pair.under;
        diag = diag.with_footer(format!(
            "({x},{y},{z}) on {over_name} stands {relation} ({ux},{uy},{uz}) on {under_name}",
            x = over.x,
            y = over.y,
            z = over.z,
            over_name = net_label(over_net, &entry.ir),
            relation = if pair.stacked() {
                "directly over"
            } else {
                "a layer over, and one step across from,"
            },
            ux = below.x,
            uy = below.y,
            uz = below.z,
            under_name = net_label(under_net, &entry.ir),
        ));
    }
    if pairs.len() > named_count {
        diag = diag.with_footer(format!(
            "and {rest} more of the same two shapes",
            rest = pairs.len() - named_count,
        ));
    }
    diag = diag.with_footer(
        "`spec/redstone` \"Place-and-route\" makes separating them the physical tile \
         layer's obligation, and the same chapter's \"Edition differences\" states it: a \
         `bridge` coord renders as a tile that conducts to neither another net's coord \
         under it nor another net's coords diagonally under it — its own net's coord \
         under it is the climb, and has to conduct",
    );
    diag = diag.with_footer(
        "Fix: nothing in the source is wrong — the pairs are what the escape costs, and enlarging the region is not a remedy: where a net has to climb at its own doorstep, more room only lengthens the run it then makes on the upper layer",
    );
    debug_assert_eq!(diag.severity(), Severity::Warning);
    Some(diag)
}

#[cfg(test)]
mod tests;
