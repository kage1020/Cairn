//! Rectilinear geometry shared by the routing, delay and crossing passes (stages 2 to 4 of
//! `spec/redstone` "Place-and-route"): the deterministic net order, and the one answer to "where
//! does this net's dust run", on the coordinate conventions [`crate::grid`] fixes. A change to
//! the axis order, the search's tie-break or the [`NetRef`] sort key reaches every pass at once.
//!
//! # The reservation is not empty space
//!
//! A reservation is a box `x ∈ [0,width) × y ∈ [0,void) × z ∈ [0,depth)` some of whose coords
//! hold blocks: cell bodies, input pads, actuator pads. Dust cannot occupy a block's coord, and a
//! signal cannot pass *through* one: a block either emits (the net's source) or consumes (one of
//! its sinks).
//!
//! [`Router`] holds that block set for one scope and grows a tree out of each net's source,
//! attaching the nearest unconnected sink by the cheapest block-free path until every sink is a
//! leaf: the shortest-path heuristic for a rectilinear Steiner tree, run inside the obstacle set.
//! [`Router::straight_run`] settles a pair with nothing between them and [`Router::search`] a
//! pair with something; both walk x, then z, then y, so a net with a clear run is an L-shape.
//!
//! # The obstacle set grows as the nets are laid
//!
//! Blocks; every coord of dust an earlier net occupies; and every coord [`beside`] that dust in
//! its own plane, because two nets one step apart are one strand carrying two signals. The second
//! net goes round, or climbs over, out of the same search. In its own plane only: whether dust at
//! `y + 1` reads the dust below it is the physical tile layer's question, so [`tile_layer_pairs`]
//! lists the strands a layer apart and [`tile_layer_clearance`] names them as an advisory rather
//! than refusing them.
//!
//! A tree is therefore a function of the order the nets were laid in. [`net_trees`] lays them in
//! [`net_order`], a total order all three passes walk, so the map is one answer per layout. The
//! climb happens before `wire_length` and `local_delay_ticks` are read off the tree, so a net that
//! climbed is charged for it.
//!
//! Only dust is widened: a sink is a block, and two nets ending at one cell body is the ordinary
//! two-input cell, so [`Router::dust`] takes the terminals out of a tree before [`beside`] widens
//! it.
//!
//! # Layers
//!
//! Dust that leaves the ground layer is [`RouteLayer::Bridge`], the layer the crossing pass gives
//! a repeater it lifts. [`CellCoord::new`] decides the layer from the height, so the two agree.
//!
//! # Two projections that cannot disagree
//!
//! [`NetTree`] is a tree of coords with a parent link on each, rooted at the source.
//! [`NetTree::wire_path`] lists them; [`NetTree::route_to`] walks the parent links back from one
//! sink, so a route is a subset of the wire by construction. Stage 2 counts `wire_path` as
//! occupancy; stages 3 and 4 place buffer repeaters on the tree and read the ones each `route_to`
//! passes through. The trees are rebuilt per stage rather than stored on the IR: they are a pure
//! function of what the IR carries, and storing them would put every coord of every net into
//! every JSON dump.
//!
//! [`RouteLayer::Bridge`]: crate::placement_ir::RouteLayer::Bridge

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

/// Folds `charge` over the distinct nets driving one cell.
///
/// Two ports on one net are fed by one strand of dust, so adding both would count the dust, and
/// every repeater on it, twice. Shared by the routing pass (`wire_length`) and the delay pass
/// (`local_delay_ticks`), so the two agree on how many strands feed a cell.
///
/// The seen-list is a `Vec` because a cell carries at most one driver per port
/// ([`crate::netlist_ir::CellNode::drivers`]), so a scan is shorter than a hash. A list that
/// broke that would still get the right answer, only slower.
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

/// What kind of component stands on a reserved coord: named in a diagnostic, and separating the
/// coords a pass can be handed twice (pads, whose z saturates) from the one it cannot (a cell,
/// whose x comes from its topological index).
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

/// One block inside a scope's reservation: where it stands, what it
/// is, and its index among its own kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockSite {
    /// The coord the block occupies.
    pub(crate) coord: CellCoord,
    /// What stands there.
    pub(crate) kind: BlockKind,
    /// Index among blocks of the same kind, for the diagnostic that names it.
    pub(crate) index: usize,
}

/// Every block in a scope, cells first, then input pads, then actuator pads: the one derivation of
/// what already stands in the reservation, from which [`crate::pass::lay_nets`] refuses a pad row
/// and builds the [`Router`].
///
/// The order matters to [`collapsed_block`], which asserts on the second of two cells sharing a
/// coord and reports the second of any other pair.
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

/// The same voxel, keyed the way the router keys it.
///
/// Every coord this module holds comes through here, so one `(x, y, z)` has one key whatever
/// layer the caller built it with. It is the identity on a coord built by [`CellCoord::new`]; it
/// catches one assembled as a struct literal, which the public fields allow.
fn keyed(coord: CellCoord) -> CellCoord {
    CellCoord::new(coord.x, coord.y, coord.z)
}

/// One coord on the search frontier, ordered so that [`BinaryHeap`]'s max pops the coord to
/// expand next.
///
/// The key is `(f, then the largest g, then the earliest step, then the smallest coord)`:
///
/// - `f = g + h` is A*'s estimate; popping the smallest finds the shortest path.
/// - Preferring the *largest* `g` among equal `f` keeps open stretches cheap: every coord on a
///   shortest path shares one `f`, so spreading over them would visit the whole box between the
///   ends. Diving walks one path and fans out only where a block stops it.
/// - The step index breaks the remaining ties by axis, so the way round a block prefers x, then
///   z, then y, as [`step_towards`] does.
/// - The coord is the last resort, so nothing is left to a hash order.
///
/// `PartialEq` is written rather than derived so it follows the key: a derived one would compare
/// `layer`, which the ordering ignores, making two entries `Ordering::Equal` and `!=` at once.
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

/// The blocks standing in one scope's reservation, and its extent. Built once per scope and
/// shared by every net, because the blocks belong to the layout rather than to a net.
pub(crate) struct Router {
    width: u32,
    height: u32,
    depth: u32,
    blocked: HashSet<CellCoord>,
}

impl Router {
    /// A router over `region` with `blocks` standing in it.
    ///
    /// The coords come through [`keyed`] again, though [`block_sites`] keys its list, so a block
    /// list assembled by hand cannot hand the router a key it never generates.
    pub(crate) fn new(region: &CircuitRegionReservation, blocks: &[BlockSite]) -> Self {
        Self {
            width: region.width,
            height: region.void,
            depth: region.depth,
            blocked: blocks.iter().map(|site| keyed(site.coord)).collect(),
        }
    }

    /// Whether `coord` is inside the reservation at all.
    fn inside(&self, coord: CellCoord) -> bool {
        coord.x < self.width && coord.y < self.height && coord.z < self.depth
    }

    /// Whether dust may be drawn on `coord` while `keep_out` stands. The blocks and `keep_out` are
    /// asked together so no caller can check one and forget the other.
    fn free(&self, coord: CellCoord, keep_out: &HashSet<CellCoord>) -> bool {
        self.inside(coord) && !self.blocked.contains(&coord) && !keep_out.contains(&coord)
    }

    /// The coord one [`STEPS`] step away, or `None` when the step leaves the reservation.
    fn step(&self, from: CellCoord, delta: (i64, i64, i64)) -> Option<CellCoord> {
        let next = stepped(from, delta)?;
        self.inside(next).then_some(next)
    }

    /// The cheapest block-free path from any coord in `seeds` to the nearest coord in `targets`,
    /// as `(index into targets, path)`, both ends included.
    ///
    /// [`Self::straight_run`] settles a closest pair with nothing between them, and
    /// [`Self::search`] the rest. Both return a shortest path, but where two targets tie, the tier
    /// decides which attaches and so the shape of the trunk; the shortcut always runs first.
    /// [`GaveUp`] when no target is reached; [`Self::unreached`] says why for each.
    ///
    /// Only the search is bounded by [`MAX_ATTENUATION_SEGMENT`]. Through
    /// [`crate::pass::lay_nets`] the straight tier stays within it too: that gate refuses any sink
    /// over the cap from its driver in a straight line, the driver is always a seed, and the
    /// straight tier answers only for the closest pair. A caller of [`Self::tree`] that skips the
    /// gate can get an over-cap path.
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
    ///
    /// No path is shorter than the straight line, so a clear line between the *closest* pair is a
    /// shortest path and [`Self::search`] cannot better it. Answering here keeps an unobstructed
    /// run linear in its length, where a frontier search would remember every coord it passed in a
    /// reservation as wide as its `size=WxH`.
    ///
    /// Only the closest pair: a closer pair might have a shorter way round, so a blocked line hands
    /// the question on rather than looking for the next clear one.
    ///
    /// A target outside the reservation is not a pair. The far end is exempt from the free check,
    /// so such a sink would otherwise be walked to and answered as wired; [`Self::search`] refuses
    /// it through [`Self::step`].
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

    /// The cheapest block-free path when something is in the way.
    ///
    /// A* over the reservation with the smallest remaining Manhattan distance as the heuristic,
    /// admissible because no clear path is shorter than the straight line, so the first target
    /// reached is the nearest.
    ///
    /// A target is somewhere the search may *arrive*, never pass through: the loop returns the
    /// moment one is popped. That makes every sink a leaf of the tree.
    ///
    /// # Bounded by the attenuation cap
    ///
    /// A coord is not queued when `g + h`, the cheapest path through it from the tree to a target,
    /// is over [`MAX_ATTENUATION_SEGMENT`]: the segment the delay pass measures is at least the
    /// path, so it would refuse. Unbounded, a walled-in target is given up on only once every free
    /// coord the tree can reach is settled, which costs `width × depth × void` rather than the
    /// distance to the target.
    ///
    /// No accepted route changes: `h` is consistent, so every coord on a cheapest path of length
    /// `L` has `g + h <= L`, and the heap returns before popping anything the bound drops.
    ///
    /// When no target has a face a path could arrive through (a free coord or a seed), the answer
    /// is [`GaveUp::Exhausted`] at once. A target with none while another has one stays a goal and
    /// stays in `h`, which only lowers `h` and so prunes less.
    ///
    /// [`GaveUp`] is about the search, not any one target; [`Router::tree`] turns it into a
    /// verdict per sink. The second value is how many coords were settled (popped and not stale),
    /// so a test can hold the bound to its purpose.
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
                // A cheaper way here was found after this entry was pushed; the heap has no
                // decrease-key.
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
                // A target is asked before the obstacle set because a cell body two nets sink at
                // is a block on both their paths and a terminal of both.
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

    /// Why no path of at most [`MAX_ATTENUATION_SEGMENT`] blocks from `seeds` reaches `sink`, asked
    /// after a pruned search ([`GaveUp::Pruned`]), and how many coords answering it visited.
    ///
    /// A breadth-first flood out of `sink` through free coords, no further than the cap. The search
    /// cannot answer this: it learns only that nothing within the cap reaches *any* target, and a
    /// pruned coord says nothing about which target is walled in.
    ///
    /// - The flood runs out before the cap: `sink` opens onto a pocket the tree is not in, so
    ///   [`Unreached::WalledIn`] is proved. A sink with no free face, or outside the reservation,
    ///   is a pocket of size zero.
    /// - The flood is still going at the cap: [`Unreached::BeyondCap`], which claims only what the
    ///   search proved. A sink walled off in a free region wider than the cap lands here too,
    ///   since telling the two apart would cost what the bound saves.
    ///
    /// Reaching a seed within the cap would contradict the search, so it is a debug assertion and
    /// otherwise answered as `BeyondCap`, the verdict that claims less. The cost is bounded by the
    /// pocket or the cap's neighbourhood of `sink`, never by the reservation.
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

    /// One net's routed tree: rooted at `source`, with every distinct sink attached as a leaf,
    /// going round the blocks and `keep_out`.
    ///
    /// Grown one sink at a time, nearest first, each hanging off the wire already laid, so a
    /// fanout shares a trunk. Everything on a path emits except its far end, the sink.
    ///
    /// `keep_out` is what earlier nets have taken: their dust and the coords [`beside`] it (see
    /// [`net_trees`]).
    ///
    /// When the search reaches none of the remaining sinks, each is stranded with its own
    /// [`Unreached`]: walled in after [`GaveUp::Exhausted`], otherwise whatever
    /// [`Self::unreached`] proves of it.
    pub(crate) fn tree(
        &self,
        source: CellCoord,
        sinks: &[CellCoord],
        keep_out: &HashSet<CellCoord>,
    ) -> NetTree {
        // `search` keeps a sink a leaf itself, but `straight_run` only asks whether a coord is
        // free, so a sink missing from the block set would be walked through. `collect_nets` and
        // `block_sites` keep the two in step.
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

    /// The coords of `tree` that are dust: everything but its terminals, which are exactly the
    /// blocks on it, since a path runs through no block and ends at one.
    ///
    /// This, widened by [`beside`], is what the next net goes round. Two drivers reaching one cell
    /// is a two-input gate, so keeping the terminals in would leave the second nowhere to arrive.
    pub(crate) fn dust(&self, tree: &NetTree) -> Vec<CellCoord> {
        tree.wire_path()
            .into_iter()
            .filter(|coord| !self.blocked.contains(coord))
            .collect()
    }
}

/// One step from `from` towards `to`, along x, then z, then y: the axis order [`STEPS`] gives
/// the search.
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
    /// Nothing was left to search and the cap pruned nothing, so every target is
    /// [`Unreached::WalledIn`].
    Exhausted,
    /// The cap pruned at least one coord. That says nothing about any one target (a target walled
    /// in near the tree is pruned round too), so [`Router::unreached`] asks each on its own.
    Pruned,
}

/// Why one sink was not reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unreached {
    /// No path of any length reaches the sink: it opens onto a pocket, closed by blocks, other
    /// nets' dust and the reservation's edge, that the tree is not in.
    WalledIn,
    /// No path of at most [`MAX_ATTENUATION_SEGMENT`] blocks reaches it. A longer one is not
    /// looked for, since the delay pass would refuse it.
    BeyondCap,
}

/// One net's routed dust: every coord it occupies, each with the coord the signal reached it
/// from. A rooted tree rather than a list so its two projections agree (see the module doc).
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

    /// Records a sink the reservation has no clear path to, attached straight to the source so
    /// both projections stay total. Every pass refuses such a scope before reading the number;
    /// [`Self::unreachable`] is what a caller asks.
    fn strand(&mut self, sink: CellCoord, why: Unreached) {
        let source = self.order[0];
        self.parent.insert(sink, source);
        self.order.push(sink);
        self.unreachable.push(sink);
        self.stranded_by.insert(sink, why);
    }

    /// Every coord the net occupies, source first, then in the order they were laid. Never empty,
    /// and no coord appears twice.
    pub(crate) fn wire_path(&self) -> Vec<CellCoord> {
        self.order.clone()
    }

    /// The dust the signal travels from the source to `sink`, a walk in which every step moves one
    /// block, except for a sink [`Self::unreachable`] lists, which [`Self::strand`] parents straight
    /// to the source. `route_to(source)` is the single-coord path.
    ///
    /// This is what a buffer repeater sits on, and it is not in general the straight line: the tree
    /// reaches a far sink through the trunk laid for a nearer one, and goes round what is in the
    /// way.
    ///
    /// `None` when `sink` is not a coord of this net.
    pub(crate) fn route_to(&self, sink: CellCoord) -> Option<Vec<CellCoord>> {
        let sink = keyed(sink);
        if sink != self.order[0] && !self.parent.contains_key(&sink) {
            return None;
        }
        let mut route = vec![sink];
        let mut current = sink;
        while let Some(previous) = self.parent.get(&current) {
            // A cycle would run forever. `attach`'s assertion against attaching a coord twice is
            // compiled out of a release build, so a walk longer than the tree turns the hang into a
            // named coord.
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

    /// The coord the signal reaches `coord` from: `None` for the source and for a coord not on
    /// this net.
    pub(crate) fn parent(&self, coord: CellCoord) -> Option<CellCoord> {
        self.parent.get(&keyed(coord)).copied()
    }

    /// A tree grown along `paths` by hand, for a test that pins a shape coord by coord. Each path
    /// starts on a coord the tree holds (the first at the source) and moves one block per step;
    /// both are debug-asserted, so a bad fixture fails here.
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

    /// The sinks the reservation has no clear path to, in the order they were asked for. Empty for
    /// every scope the routing pass lets through.
    pub(crate) fn unreachable(&self) -> &[CellCoord] {
        &self.unreachable
    }

    /// Why `sink` is one of [`Self::unreachable`], or `None` when it is not one.
    pub(crate) fn stranded_by(&self, sink: CellCoord) -> Option<Unreached> {
        self.stranded_by.get(&sink).copied()
    }
}

/// Every net in a scope: driver → the sinks it feeds, the cell body for a cell driver and the
/// actuator's pad for an output driver. Shared by all three passes so they agree on what a net
/// *is*.
///
/// A coord appears once per net however many of its drivers end at it: two ports of one cell
/// reading one signal are one strand into one block, and [`net_order`] reads the list's length
/// as fanout, which decides where every net's dust runs.
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
        // The pad the placement pass assigned, so the `x = width - 1` rule lives in one place.
        sink(output.driver, output.pad);
    }
    nets
}

/// The order the nets of a scope are laid in: fanout descending, then [`net_ref_key`] ascending.
///
/// This is geometry, not presentation: [`net_trees`] lays each net round the ones before it, so
/// the order decides which net keeps the direct run, and a total order lets all three passes
/// rebuild the same trees. Fanout first because a net with more sinks has less freedom in where
/// its trunk can run.
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

/// The first pad standing on a coord another block already holds, or `None` when every block
/// has a coord of its own.
///
/// That is a pad the reservation is too small to hold: too shallow, so a pad column taller than
/// it piles up on `depth - 1`, or too narrow, so at `width == 1` pad #0 of each edge stands at
/// `(0,0,0)`. An author can enlarge either, so it earns a diagnostic.
///
/// # Panics
///
/// Panics when two cells share a coord. A cell's coord comes from its topological index, so that
/// is an IR the caller assembled wrongly and no `size=` repairs, and it would otherwise read as a
/// layout. [`block_sites`] lists cells before pads, so a list with both faults panics.
///
/// The coords are compared through [`keyed`], as [`Router::new`] keys them, so this agrees with
/// the router about a hand-built coord whose layer does not match its height.
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

/// The refusal a scope earns when the reservation cannot wire one of its sinks, or `None` when
/// every sink has a clear path.
///
/// `E_ROUTE_CONGESTION` with its own primary, since the area can be ample and the one coord the
/// wire needs still be taken; for a sink [`Unreached::BeyondCap`] stranded, `E_ATTENUATION_LIMIT`
/// from [`beyond_cap_diagnostic`]. Either way it names the nets whose dust takes the sink's faces
/// and counts the scope's other stranded sinks, so sizing is one decision.
///
/// One sink anchors the message and gives the code: a [`Unreached::WalledIn`] one when there is
/// one, because its wall is a cause the author can move while "nothing within the cap" may be a
/// symptom of it; then the first in [`net_order`] and the net's own order. `netlist` is as for
/// [`crate::pass::lay_nets`].
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

/// The crowding clause and the count of the other stranded sinks, which both of
/// [`unroutable`]'s refusals end with.
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

/// The refusal for a sink [`Unreached::BeyondCap`] stranded, under `E_ATTENUATION_LIMIT`, the
/// delay pass's code for a route over the cap.
///
/// It says only that no route within the cap exists, since nothing looked for a longer one. The
/// crowding clause stays, since a sink can be out of reach for both reasons at once.
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

/// The other nets whose dust keeps a wire out of the faces of `sink`, in [`net_order`].
///
/// A net takes a face by drawing dust on it or [`beside`] it, the rule the router routes by. A
/// net's own dust is not counted, and neither is a terminal, since the message already says
/// blocks are in the way.
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

/// Human-facing name for a net: the sensor's dotted `sig.<name>`, or `cell #j` for a synthesised
/// gate, which has no source-level name. `input pad #i` covers a hand-built IR whose input row is
/// too short.
pub(crate) fn net_label(net: NetRef, ir: &PlacementIr) -> String {
    match net {
        NetRef::Input(i) => ir
            .inputs
            .get(i as usize)
            .map_or_else(|| format!("input pad #{i}"), |input| input.name.to_string()),
        NetRef::Cell(j) => format!("cell #{j}"),
    }
}

/// The routed tree of every net, keyed by driver.
///
/// Laid in [`net_order`], each net going round the dust of the ones before it and the coords
/// [`beside`] it, so no two trees run within one step of each other in one plane. The order is
/// total, so all three passes get the same map. A net with nowhere left to go is stranded rather
/// than laid over another's dust; [`unroutable`] turns that into a refusal.
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
            // Asserted in release, here: two nets one step apart in one plane share a strand, no
            // diagnostic names that, and the routed paths do not reach the IR for anyone to
            // inspect.
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
        // Widened after the whole net is claimed: a strand runs beside itself, so widening as it
        // went would trip the claim above on the net's own turn.
        keep_out.extend(dust.into_iter().flat_map(beside));
        trees.insert(net, tree);
    }
    trees
}

/// Two strands of dust a layer apart and within one step of each other: the pair
/// `spec/redstone` "Place-and-route" hands to the physical tile layer.
///
/// One entry per pair of coords, upper first. Two strands alongside each other for several coords
/// are several pairs, because each coord is its own place for a tile to conduct or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TileLayerPair {
    /// The upper coord, and the net whose dust it is.
    pub(crate) over: (CellCoord, NetRef),
    /// The lower coord, and its net, never `over`'s: one strand climbing is the wire, not two
    /// signals.
    pub(crate) under: (CellCoord, NetRef),
}

impl TileLayerPair {
    /// Whether the two sit on one column, rather than one step across.
    fn stacked(self) -> bool {
        self.over.0.x == self.under.0.x && self.over.0.z == self.under.0.z
    }
}

/// Every pair of strands the physical tile layer has to separate, in [`coord_key`] order over the
/// upper coord and then the lower.
///
/// No two nets run within one step in one plane, so every pair comes from a climb: a net that
/// climbs to clear another runs over it, or one step across a layer up (the staircase dust climbs,
/// which shorts the same way an in-plane pair does).
///
/// Dust only, per [`Router::dust`]: a block under a strand is what a wire climbs over. The
/// obligation `spec/redstone` "Edition differences" puts on a `bridge` tile is wider and covers
/// blocks without this list: it conducts to no coord of another net below it.
pub(crate) fn tile_layer_pairs(
    nets: &HashMap<NetRef, Vec<CellCoord>>,
    trees: &HashMap<NetRef, NetTree>,
    router: &Router,
) -> Vec<TileLayerPair> {
    // `net_trees` asserts that no two nets claim one coord, so one owner per coord loses nothing.
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

/// How many pairs *of each shape* the finding names one by one; the primary carries the count.
/// Per shape because the two are separated by different tiles, and a layout is lopsided towards
/// the diagonal often enough for a flat cap to name one shape only.
const NAMED_PER_SHAPE: usize = 2;

/// The advisory a scope earns for strands a layer apart, or `None` when no two nets run within
/// one step of each other across layers.
///
/// `W_ROUTE_CROSS_LAYER_CLEARANCE`, a warning rather than a refusal: `spec/redstone`
/// "Place-and-route" makes separating them the physical tile layer's obligation, because whether
/// the upper strand reads the lower depends on what stands between them, which the pseudo-2.5D
/// model does not know. This names the pairs so a reader of the dump can find them.
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
