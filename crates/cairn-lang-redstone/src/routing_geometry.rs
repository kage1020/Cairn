//! Shared rectilinear-geometry helpers for the routing / delay /
//! crossing passes.
//!
//! The three passes (`spec/redstone` §14.5 stages 2, 3, and 4) all
//! need the same pad-coordinate convention, deterministic net-order
//! key, and the same answer to "where does this net's dust run".
//! Keeping the primitives in one module guarantees that a future PR
//! touching the axis order, the search's tie-break, or the [`NetRef`]
//! sort key updates every downstream consumer in one place — the JSON
//! dumps compared byte-for-byte by
//! `crates/cairn-lang-redstone/tests/routing.rs` and the crossing /
//! delay integration tests catch any drift.
//!
//! # The reservation is not empty space
//!
//! A `circuit region=<label> void=<N>` reservation is a box
//! `x ∈ [0,width) × y ∈ [0,void) × z ∈ [0,depth)`, and some of its
//! coords already hold blocks: cell bodies, input pads, actuator pads.
//! Dust cannot occupy a block's coord, and a signal cannot pass
//! *through* one — a block either emits (it is the net's source) or
//! consumes (it is one of the net's sinks). A comparator on the way to
//! a further cell does not hand the signal on; what leaves it is its
//! own output.
//!
//! [`Router`] holds that block set for one scope and answers the
//! question the three passes ask: given a source and a set of sinks,
//! which coords does the net's dust occupy, and which of them does the
//! signal into *this* sink travel along. It grows a tree out of the
//! source, attaching the nearest sink still unconnected by the
//! cheapest block-free path, until every sink is a leaf of it. That is
//! the shortest-path heuristic for a rectilinear Steiner tree, run
//! inside the obstacle set rather than on an empty plane.
//!
//! The path itself is settled by [`Router::straight_run`] when the
//! closest pair has nothing between them and by [`Router::search`]
//! when it does — one question, and a shortcut that is allowed only
//! because no path is shorter than the straight line between its ends.
//! Both walk x, then z, then y, the axis order the passes were built
//! around, so a net with a clear run between its terminals occupies
//! exactly the L-shape it always did. Only a net that has something to
//! go around moves.
//!
//! # The obstacle set grows as the nets are laid
//!
//! Blocks; every coord of dust an earlier net already occupies; and
//! every coord beside that dust in its own plane. Two nets on one
//! coord is one strand carrying two signals, and so is two nets one
//! step apart — dust joins the dust next to it — so the second net to
//! be routed treats both the way it treats a cell body: something to
//! go round, or to climb over. `spec/redstone` §14.5 calls that
//! escape, and it falls out of the search that was already going round
//! blocks rather than out of a mechanism of its own.
//!
//! In its own plane, and no further. Whether dust at `y + 1` reads the
//! dust below it depends on what stands between them, which is a
//! question about the physical tile layer — §14.5 leaves the voxel
//! realisation to that layer, and this module holds itself to the
//! plane it can answer for.
//!
//! What it costs is that a tree is a function of the order the nets
//! were laid in. [`net_trees`] lays them in [`net_order`] — fanout
//! descending, then [`net_ref_key`] — which is a total order over the
//! nets of a scope and the same one all three passes walk, so the map
//! is one answer per layout rather than one per `HashMap` iteration.
//! What it buys is that the escape is measured: it happens before
//! `wire_length` and `delay_ticks` are read off the tree, so a net
//! that had to climb is charged for the climb.
//!
//! Only dust. A sink is a block, and two nets ending at one cell body
//! is the ordinary two-input cell — so [`Router::dust`] takes the
//! blocks back out of a tree before [`beside`] widens it, and what is
//! left is exactly what shorts. Two faces of one block are two steps
//! apart, so widening it costs a two-input gate nothing: neither
//! driver keeps the other out.
//!
//! # Layers
//!
//! Dust that has to leave the ground layer to get past a block, or
//! past another net, is stamped [`RouteLayer::Bridge`], the same layer
//! the crossing pass stamps on a repeater it lifts. One rule —
//! [`CellCoord::new`] — decides the layer from the height, so the two
//! cannot key past each other and a repeater cannot be lifted onto a
//! coord a wire already runs through.
//!
//! # Two projections that cannot disagree
//!
//! [`NetTree`] is a tree of coords with a parent link on each, rooted
//! at the source. [`NetTree::wire_path`] lists them; [`NetTree::route_to`]
//! walks the parent links back from one sink. The second is a subset of
//! the first by construction rather than by agreement between two
//! renderings — which is what used to fail: an L-shape is
//! direction-asymmetric, so drawing an edge `a → b` for the wire and
//! `b → a` for the route picked opposite elbows and put buffer
//! repeaters beside the dust they were meant to refresh.
//!
//! Stage 2 drains [`NetTree::wire_path`] into its occupancy set, stage
//! 3 measures [`NetTree::route_to`] to count buffer repeaters, and
//! stage 4 walks the same route to place them.
//!
//! The trees are recomputed per stage rather than stored on
//! [`crate::placement_ir::PlacementIr`]. They are a pure function of
//! the cells, outputs, and reservation the IR already carries, so a
//! stored copy would be a cache with a staleness mode this has not,
//! and it would put every coord of every net into the JSON dump each
//! stage emits.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt::Write as _;

use cairn_lang_core::check::Severity;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::netlist_ir::{CellPortDriver, NetRef};
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIrEntry,
};

/// Deterministic net-order key matching the routing pass's tie-break:
/// `Input(_)` sorts before `Cell(_)`, then by index ascending.
pub(crate) fn net_ref_key(net: NetRef) -> (u8, u32) {
    match net {
        NetRef::Input(i) => (0, i),
        NetRef::Cell(j) => (1, j),
    }
}

/// Deterministic coord order: x, then z, then y.
///
/// The same axis order [`STEPS`] gives the search and `step_towards`
/// walks a clear run in, so a list sorted by this reads in the order
/// the router laid it. Height comes last because it is the axis a
/// coord only leaves the ground for, and every pass that anchors a
/// message on "the lowest coord of these" means the one nearest the
/// origin of the plane rather than the one nearest the floor.
pub(crate) fn coord_key(coord: CellCoord) -> (u32, u32, u32) {
    (coord.x, coord.z, coord.y)
}

/// v1 input-pad coordinate: left edge (`x=0`), first service layer
/// (`y=0`), z-axis increasing as the input index grows. Saturates at
/// `depth-1` whenever the input count would push z past the region's
/// z-extent (`inputs.len() + 1 > depth`); the resulting overlap is
/// caught at seeding time and surfaces as `E_ROUTE_CONGESTION`
/// rather than a silent misroute. Pinning the coordinate here is a
/// v1 convention; once a consumer outside this crate needs the pad
/// coords, `input_pads` joins [`crate::placement_ir::PlacementIr`]
/// as a `#[non_exhaustive]`-safe field.
pub(crate) fn input_pad(i: usize, region: &CircuitRegionReservation) -> CellCoord {
    let raw = u32::try_from(i).unwrap_or(u32::MAX);
    let z = raw.min(region.depth.saturating_sub(1));
    CellCoord::new(0, 0, z)
}

/// v1 output-pad coordinate: right edge (`x=width-1`), same
/// saturating z-axis convention as [`input_pad`].
pub(crate) fn output_pad(k: usize, region: &CircuitRegionReservation) -> CellCoord {
    let raw = u32::try_from(k).unwrap_or(u32::MAX);
    let z = raw.min(region.depth.saturating_sub(1));
    let x = region.width.saturating_sub(1);
    CellCoord::new(x, 0, z)
}

/// Fold `charge` over the distinct nets driving one cell.
///
/// Two ports on one net are fed by one strand of dust. The routed
/// length into a cell is a function of the `(net, sink)` pair, so the
/// second port re-derives the first port's number: adding them
/// describes a layout with twice the dust, and a signal that passes
/// through every repeater on the way in twice.
///
/// Shared by the routing pass, which charges blocks of dust into
/// `wire_length`, and the delay pass, which charges ticks into
/// `delay_ticks`, so the two cannot disagree about how many strands
/// feed a cell. The seen-list is a `Vec` because a cell carries at
/// most one driver per port — a producer contract on
/// [`crate::netlist_ir::CellNode::drivers`], not something checked
/// here — so the scan is shorter than a hash. A driver list that
/// broke the contract would still get the right answer, only slower.
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

/// Manhattan (L¹) distance between two cell coordinates.
///
/// The search's heuristic: no clear path between two coords is shorter
/// than this, which is what makes the first sink the search reaches
/// the nearest one.
pub(crate) fn manhattan(a: CellCoord, b: CellCoord) -> u32 {
    let dx = a.x.max(b.x) - a.x.min(b.x);
    let dy = a.y.max(b.y) - a.y.min(b.y);
    let dz = a.z.max(b.z) - a.z.min(b.z);
    dx.saturating_add(dy).saturating_add(dz)
}

/// What kind of component stands on a reserved coord.
///
/// Names the thing in a diagnostic, and separates the two coords a
/// pass can be handed twice (pads, whose z saturates) from the one it
/// cannot (a cell, whose x is derived from its topological index and
/// so is a column of its own).
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
    /// Index among blocks of the same kind, for the diagnostic that
    /// names it.
    pub(crate) index: usize,
}

/// Every block in a scope, cells first, then input pads, then actuator
/// pads.
///
/// The one derivation of "what is already standing in the reservation".
/// The routing pass reads it to refuse a pad row that cannot fit, all
/// three passes hand it to [`Router::new`], and [`Router::dust`] reads
/// it back out of a tree to tell a net's wire from its terminals. A
/// second list built anywhere else is a second thing to keep in step.
pub(crate) fn block_sites(ir: &PlacementIr, region: &CircuitRegionReservation) -> Vec<BlockSite> {
    let mut sites = Vec::with_capacity(ir.cells.len() + ir.inputs.len() + ir.outputs.len());
    for (index, cell) in ir.cells.iter().enumerate() {
        sites.push(BlockSite {
            coord: keyed(cell.coord),
            kind: BlockKind::Cell,
            index,
        });
    }
    for index in 0..ir.inputs.len() {
        sites.push(BlockSite {
            coord: input_pad(index, region),
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
/// Everything this module holds — the block set, a tree's coords, the
/// sink a caller asks a route for — comes through here, so one
/// `(x, y, z)` has one key rather than one per layer the caller
/// happened to build it with. [`CellCoord::new`] already derives the
/// layer from the height, so this is the identity on anything built
/// through it; what it catches is a coord assembled as a struct
/// literal, which the public fields allow.
fn keyed(coord: CellCoord) -> CellCoord {
    CellCoord::new(coord.x, coord.y, coord.z)
}

/// The six rectilinear steps, in the axis order the passes were built
/// around: x before z before y.
///
/// Both directions of an axis sit next to each other, so the order
/// expresses an axis preference rather than a direction preference —
/// which is what makes the search lay the same L-shaped wire wherever
/// nothing is in the way, whichever way the net runs.
const STEPS: [(i64, i64, i64); 6] = [
    (-1, 0, 0),
    (1, 0, 0),
    (0, 0, -1),
    (0, 0, 1),
    (0, -1, 0),
    (0, 1, 0),
];

/// The four of [`STEPS`] that stay in the coord's own plane.
const IN_PLANE: [(i64, i64, i64); 4] = [(-1, 0, 0), (1, 0, 0), (0, 0, -1), (0, 0, 1)];

/// The coord one step from `from`, or `None` when the step runs off the
/// `0` edge, which the unsigned coords cannot represent.
///
/// Says nothing about the reservation: [`Router::step`] adds that, and
/// [`beside`] deliberately does not — a coord outside the region is
/// still a coord no dust of this scope stands on.
fn stepped(from: CellCoord, (dx, dy, dz): (i64, i64, i64)) -> Option<CellCoord> {
    let axis = |value: u32, delta: i64| -> Option<u32> {
        u32::try_from(i64::from(value).checked_add(delta)?).ok()
    };
    Some(CellCoord::new(
        axis(from.x, dx)?,
        axis(from.y, dy)?,
        axis(from.z, dz)?,
    ))
}

/// The coords a strand of dust on `coord` reaches: the four steps that
/// keep it in its own plane.
///
/// Dust joins the dust beside it, so a second net drawing on one of
/// these is one strand with the first as surely as a second net drawing
/// on `coord` itself. Up and down are not here: whether dust at `y + 1`
/// reads the dust below it depends on what stands between them, and
/// `spec/redstone` §14.5 leaves that to the physical tile layer.
pub(crate) fn beside(coord: CellCoord) -> impl Iterator<Item = CellCoord> {
    IN_PLANE
        .into_iter()
        .filter_map(move |delta| stepped(coord, delta))
}

/// One coord on the search frontier, ordered so that
/// [`BinaryHeap`]'s max pops the coord to expand next.
///
/// The key is `(f, then the largest g, then the earliest step, then
/// the smallest coord)`:
///
/// - `f = g + h` is A*'s estimate, and popping the smallest is what
///   makes the path found the shortest one.
/// - Preferring the *largest* `g` among equal `f` is what keeps the
///   open stretches of a search cheap: every coord on a shortest path
///   shares one `f`, so a search that spread over them would visit the
///   whole box between the two ends. Diving instead walks one of those
///   paths and only fans out where a block stops it.
/// - The step index breaks the remaining ties by axis, so the way round
///   a block prefers x, then z, then y — the order [`step_towards`]
///   walks for the run that needs no search.
/// - The coord is the last resort, so nothing is left to a hash order.
///
/// `PartialEq` is written rather than derived so it cannot drift from
/// the key: `layer` is not part of the ordering, and a derived `Eq`
/// compares it, which would make two entries `Ordering::Equal` and
/// `!=` at once. `CellCoord::new` makes the layer a function of the
/// height, so no pair like that reaches the heap today — but the
/// contract would then hold by an invariant living on another type.
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

/// The blocks standing in one scope's reservation, and the extent they
/// stand in.
///
/// Built once per scope and shared by every net, because the obstacle
/// set is a property of the layout rather than of the net being
/// routed.
pub(crate) struct Router {
    width: u32,
    height: u32,
    depth: u32,
    blocked: HashSet<CellCoord>,
}

impl Router {
    /// A router over `region` with `blocks` standing in it.
    ///
    /// The coords come through [`keyed`] again even though
    /// [`block_sites`] already keyed the list it builds: the two do it
    /// for different consumers — the crossing pass compares that list
    /// against wire coords — and a caller assembling a block list by
    /// hand would otherwise hand the router a key it never generates.
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

    /// Whether dust may be drawn on `coord` while `keep_out` stands.
    ///
    /// The two halves of the obstacle set are asked together so no
    /// caller can consult one and forget the other: a straight run that
    /// checked only the blocks would lay this net's dust over the last
    /// net's — or one step from it, which is the same short.
    fn free(&self, coord: CellCoord, keep_out: &HashSet<CellCoord>) -> bool {
        self.inside(coord) && !self.blocked.contains(&coord) && !keep_out.contains(&coord)
    }

    /// The coord one [`STEPS`] step away, or `None` when the step
    /// leaves the reservation (including off the `0` edge, which the
    /// unsigned coords cannot represent).
    fn step(&self, from: CellCoord, delta: (i64, i64, i64)) -> Option<CellCoord> {
        let next = stepped(from, delta)?;
        self.inside(next).then_some(next)
    }

    /// The cheapest block-free path from any coord in `seeds` to the
    /// nearest coord in `targets`, as `(index into targets, path)`.
    /// The path includes both ends; its first coord is the seed it
    /// left from.
    ///
    /// Two tiers, one cost. [`Self::straight_run`] settles the case
    /// where the closest pair has nothing between them; only when
    /// something does is [`Self::search`] asked to find the way round.
    /// Both return a shortest path to a nearest target, so the length
    /// is the same either way — but where two targets tie, which one
    /// attaches is the tier's to decide, and that decides the shape of
    /// the trunk. The shortcut runs first, so the answer is one
    /// answer.
    ///
    /// `None` when no target can be reached: every route out of the
    /// tree is walled in by blocks or by the edge of the reservation.
    fn reach(
        &self,
        seeds: &[CellCoord],
        targets: &[CellCoord],
        keep_out: &HashSet<CellCoord>,
    ) -> Option<(usize, Vec<CellCoord>)> {
        self.straight_run(seeds, targets, keep_out)
            .or_else(|| self.search(seeds, targets, keep_out))
    }

    /// The closest seed/target pair, when the straight line between
    /// them is clear.
    ///
    /// No path is shorter than the straight line between its ends, so a
    /// clear straight line between the *closest* pair is a shortest
    /// path to a nearest target and [`Self::search`] cannot better it.
    /// Answering here is what keeps an unobstructed run linear in its
    /// own length: a frontier search over open space still has to
    /// remember every coord it walked past, and a reservation is as
    /// wide as the `size=WxH` it was cut out of.
    ///
    /// Only the closest pair. A clear line to a pair that is *not* the
    /// closest says nothing — the closer pair might have a way round
    /// that is shorter still — so a blocked line hands the question on
    /// rather than looking for the next clear one.
    ///
    /// A target outside the reservation is not a pair at all. The
    /// interior check below asks whether a coord is free and the far
    /// end is exempt from it, so without this a sink the reservation
    /// does not contain would be walked straight to — and answered as
    /// wired. [`Self::search`] refuses it already, through
    /// [`Self::step`]; this is the same answer from the tier that does
    /// not search.
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
    /// A* over the reservation with the smallest remaining Manhattan
    /// distance as the heuristic — admissible because no clear path is
    /// shorter than the straight line, which is what makes the target
    /// it reaches first the nearest one.
    ///
    /// A target is somewhere the search may *arrive*, never somewhere
    /// it may pass through: the loop returns the moment one is popped,
    /// so no sink is ever expanded past. That is what makes every sink
    /// a leaf of the tree — the property a comparator has and a coil of
    /// dust does not.
    fn search(
        &self,
        seeds: &[CellCoord],
        targets: &[CellCoord],
        keep_out: &HashSet<CellCoord>,
    ) -> Option<(usize, Vec<CellCoord>)> {
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
                // A cheaper way to this coord was found after this
                // entry was pushed; the heap has no decrease-key.
                continue;
            }
            if let Some(index) = targets.iter().position(|t| *t == current.coord) {
                return Some((index, walk_back(current.coord, &parent)));
            }
            for (step, delta) in STEPS.iter().enumerate() {
                let Some(next) = self.step(current.coord, *delta) else {
                    continue;
                };
                // A target may be arrived at and never passed
                // through, which is what keeps a sink a leaf — and it
                // is why the obstacle set is asked second: a cell body
                // two nets both sink at is a block on both of their
                // paths and a terminal of both.
                let target = targets.contains(&next);
                if !target && !self.free(next, keep_out) {
                    continue;
                }
                let g = current.g.saturating_add(1);
                if cheapest.get(&next).is_some_and(|best| *best <= g) {
                    continue;
                }
                cheapest.insert(next, g);
                parent.insert(next, current.coord);
                frontier.push(Frontier {
                    f: g.saturating_add(heuristic(next)),
                    g,
                    step,
                    coord: next,
                });
            }
        }
        None
    }

    /// One net's routed tree: rooted at `source`, with every distinct
    /// sink attached as a leaf, going round the blocks and round
    /// `keep_out`.
    ///
    /// Grown one sink at a time, nearest first, because a sink can only
    /// be reached from wire that already exists: the dust laid for the
    /// first sink is what the second hangs off, which is how a fanout
    /// gets a shared trunk instead of one strand per sink. Everything
    /// on a path emits except its far end — the sink is a block, and a
    /// block ends the dust that arrives at it.
    ///
    /// `keep_out` is what the nets routed before this one have taken:
    /// their dust, and the coords [`beside`] it — see [`Self::dust`]
    /// and [`net_trees`]. Empty for the first net of a scope, and for a
    /// caller routing one net against nothing.
    pub(crate) fn tree(
        &self,
        source: CellCoord,
        sinks: &[CellCoord],
        keep_out: &HashSet<CellCoord>,
    ) -> NetTree {
        // Every sink is a block of this router. `search` enforces the
        // leaf property itself — it returns the moment a target is
        // popped, so no target is expanded past — but `straight_run`
        // has no equivalent guard: its interior check is "is this coord
        // free", so a sink that is not in the block set would be walked
        // straight through and stop being a leaf. The correspondence
        // that makes it true is between `collect_nets` and
        // `block_sites`, two functions away from here, which is exactly
        // the kind of precondition worth saying out loud.
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
            let Some((index, path)) = self.reach(&emitters, &remaining, keep_out) else {
                for sink in remaining.drain(..) {
                    tree.strand(sink);
                }
                break;
            };
            remaining.remove(index);
            for pair in path.windows(2) {
                tree.attach(pair[1], pair[0]);
            }
            // Every coord the path laid but its far end: the sink
            // consumes the signal, so nothing continues out of it.
            emitters.extend(path[1..path.len().saturating_sub(1)].iter().copied());
        }
        tree
    }

    /// The coords of `tree` that are dust: everything but its
    /// terminals.
    ///
    /// A path this router laid runs through no block and ends at one,
    /// so the blocks on a tree are exactly its source and its sinks —
    /// which makes this a derivation rather than a second list to keep
    /// in step with [`NetTree::wire_path`].
    ///
    /// This, widened by [`beside`], is what the next net has to go
    /// round. A cell body is a terminal of every net that drives it,
    /// and two drivers reaching one cell is a two-input gate rather
    /// than a short; keeping the terminals in would leave the second
    /// driver of every such cell with nowhere to arrive, and widening
    /// them would take the faces beside their own.
    pub(crate) fn dust(&self, tree: &NetTree) -> Vec<CellCoord> {
        tree.wire_path()
            .into_iter()
            .filter(|coord| !self.blocked.contains(coord))
            .collect()
    }
}

/// One step from `from` towards `to`, along x, then z, then y — the
/// axis order [`STEPS`] gives the search, spelled once for the run that
/// needs no search.
fn step_towards(from: CellCoord, to: CellCoord) -> CellCoord {
    let mut next = from;
    if from.x != to.x {
        next.x = if from.x < to.x {
            from.x + 1
        } else {
            from.x - 1
        };
    } else if from.z != to.z {
        next.z = if from.z < to.z {
            from.z + 1
        } else {
            from.z - 1
        };
    } else if from.y != to.y {
        next.y = if from.y < to.y {
            from.y + 1
        } else {
            from.y - 1
        };
    }
    CellCoord::new(next.x, next.y, next.z)
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

/// One net's routed dust: every coord it occupies, each with the coord
/// the signal reached it from.
///
/// Held as a rooted tree rather than as a flat list because the tree
/// has to answer two different questions and give answers that agree —
/// see the module doc. `route_to` reads the parent links the wire was
/// laid with, so a coord on a route is a coord of the wire by
/// construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetTree {
    order: Vec<CellCoord>,
    parent: HashMap<CellCoord, CellCoord>,
    unreachable: Vec<CellCoord>,
}

impl NetTree {
    /// A tree holding only its source.
    fn rooted(source: CellCoord) -> Self {
        Self {
            order: vec![source],
            parent: HashMap::new(),
            unreachable: Vec::new(),
        }
    }

    /// Record `coord` as reached from `from`.
    fn attach(&mut self, coord: CellCoord, from: CellCoord) {
        debug_assert!(
            !self.parent.contains_key(&coord) && coord != self.order[0],
            "the search returns a path of coords the tree does not hold yet",
        );
        self.parent.insert(coord, from);
        self.order.push(coord);
    }

    /// Record a sink the reservation has no clear path to.
    ///
    /// Attached straight to the source so both projections stay total:
    /// the routing pass refuses such a scope before any consumer reads
    /// the number, and a hand-built IR handed to stage 3 or stage 4
    /// without stage 2 gets a deterministic answer rather than a
    /// panic. The number itself means nothing — [`Self::unreachable`]
    /// is what a caller asks.
    fn strand(&mut self, sink: CellCoord) {
        let source = self.order[0];
        self.parent.insert(sink, source);
        self.order.push(sink);
        self.unreachable.push(sink);
    }

    /// Every coord the net occupies, source first and then in the
    /// order the search laid them.
    ///
    /// Always non-empty — a caller that discarded an empty return would
    /// drop the degenerate (no sinks, or the only sink is the source)
    /// case where the source still occupies its own coord.
    ///
    /// No coord appears twice, so a caller folding this into a
    /// per-coord map does not have to ask whether it has seen the pair
    /// before. Three things hold that up, one per way a coord enters
    /// the list:
    ///
    /// - The source is pushed once, by [`Self::rooted`].
    /// - A sink is pushed by [`Self::strand`], which takes each
    ///   remaining terminal once because [`Router::tree`] dedups the
    ///   terminal list against itself and the source and drains it.
    /// - Everything else is pushed by [`Self::attach`], off a path the
    ///   search returned. A coord already on the list is either a
    ///   terminal — and terminals are blocks, which no path walks
    ///   through — or a coord the search was seeded from, which it
    ///   does not expand back onto. `attach`'s own check of this is a
    ///   `debug_assert!`, so it states the invariant rather than
    ///   enforcing it in release.
    pub(crate) fn wire_path(&self) -> Vec<CellCoord> {
        self.order.clone()
    }

    /// The dust the signal travels from the source to `sink`: the
    /// parent links back from it, reversed, so the result is a walk in
    /// which every step moves one block.
    ///
    /// The exception is a sink [`Self::unreachable`] lists: [`Self::strand`]
    /// parents it straight to the source, so its route is one step of
    /// whatever distance separates them. Every pass refuses such a
    /// scope before measuring it.
    ///
    /// This is what a buffer repeater has to sit on, and it is not in
    /// general the straight line between the two: the tree reaches a
    /// far sink through the trunk laid for a nearer one, and it goes
    /// around whatever stands in the way. `route_to(source)` is the
    /// single-coord path.
    ///
    /// `None` when `sink` is not a coord of this net — a caller asking
    /// about a sink on a different net.
    pub(crate) fn route_to(&self, sink: CellCoord) -> Option<Vec<CellCoord>> {
        let sink = keyed(sink);
        if sink != self.order[0] && !self.parent.contains_key(&sink) {
            return None;
        }
        let mut route = vec![sink];
        let mut current = sink;
        while let Some(previous) = self.parent.get(&current) {
            // A cycle here would run forever and grow `route` until the
            // allocator gave up. `attach` debug-asserts that no coord
            // is attached twice, which is where the acyclicity comes
            // from — but that assertion is compiled out of a release
            // build, and `parent.insert` would silently overwrite. A
            // walk longer than the tree has coords is the cheap way to
            // turn the hang into a named coord.
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

    /// The sinks the reservation has no clear path to, in the order
    /// they were asked for.
    ///
    /// Empty for every scope the routing pass lets through: it is
    /// stage 2 that refuses a layout the router cannot wire.
    pub(crate) fn unreachable(&self) -> &[CellCoord] {
        &self.unreachable
    }
}

/// Every net in a scope: driver → the sinks it feeds. A cell driver
/// sinks at the cell body; an output driver sinks at the actuator's
/// pad.
///
/// Shared by the routing, delay, and crossing passes so the three
/// cannot disagree about what a net *is*. Each pass supplies its own
/// `NetRef → source coord` mapping (they differ only in how loud they
/// are about a hand-built IR that breaks the topological invariant),
/// but the sink side is this one function.
///
/// A coord appears once per net however many of that net's drivers end
/// at it. Two ports of one cell reading one signal are one strand of
/// dust arriving at one block, and the list's length is read as the
/// net's fanout by [`net_order`], which decides which net is laid
/// first and therefore where every net's dust runs. [`Router::tree`]
/// dedups its terminal list too, so the trees were already right;
/// what a duplicate moved was the order, and that now moves the wire.
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
        // The pad the placement pass assigned, not a re-derivation of
        // it: three passes call this function, and a second copy of the
        // `x = width - 1` rule is a second thing to keep in step.
        sink(output.driver, output.pad);
    }
    nets
}

/// The order the nets of a scope are laid in: fanout descending, ties
/// broken by [`net_ref_key`] ascending. `HashMap` iteration order never
/// reaches an output.
///
/// This is geometry, not presentation. [`net_trees`] lays each net
/// around the dust of the ones before it, so the order decides which
/// net keeps the direct run and which goes round — and being a total
/// order over the nets of a scope is what lets the routing, delay and
/// crossing passes rebuild the same trees. Fanout first because a net
/// with more sinks has less freedom in where its trunk can run.
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

/// The refusal a scope earns when the reservation cannot wire one of
/// its sinks, or `None` when every sink has a clear path.
///
/// `E_ROUTE_CONGESTION` because spec §14.5 states the code as "routing
/// is confined to the `circuit` region; if it does not fit, fail-loud",
/// and a reservation with no room for a clear path is a reservation the
/// layout does not fit in. Its own primary, so it does not read as the
/// area arithmetic: the area can be ample and the one coord the wire
/// needs still be a cell body — or dust an earlier net laid, or the
/// coord beside such dust, which is the common one, because a net going
/// round what is in its way is what keeps two signals off one strand.
///
/// Asked by all three passes rather than by stage 2 alone. Stage 2
/// elides the scope, so stages 3 and 4 never see one in a real run —
/// but they rebuild the trees from the IR, and an in-crate caller that
/// skipped stage 2 would hand them a stranded sink whose route is one
/// step: under every cap, worth no repeater, and indistinguishable in
/// the dump from a circuit that works. The delay pass already re-checks
/// the missing-region case on the same argument.
///
/// Reported in [`net_order`], so a scope with several unwireable sinks
/// names the same one every run, and the count comes with it: the
/// router strands every remaining sink of a net at once, so a fix-one-
/// recompile loop would be several rounds of the same sizing decision.
///
/// The three causes the primary lists are not equally likely and the
/// message cannot tell which one applied — the search reports that it
/// failed, not what it hit last. What it can do is name the nets whose
/// dust takes the faces of the sink it could not reach, which is the
/// cause the author can act on and the only one this pass created.
/// Blocks beside it are not named: a cell body or a pad beside a cell
/// body is the layout the author wrote, and saying so adds nothing.
pub(crate) fn unroutable<F>(
    nets: &HashMap<NetRef, Vec<CellCoord>>,
    trees: &HashMap<NetRef, NetTree>,
    entry: &ScopedPlacementIrEntry,
    region: &CircuitRegionReservation,
    source_of_net: F,
) -> Option<Diagnostic>
where
    F: Fn(NetRef) -> CellCoord,
{
    let order = net_order(nets);
    let stranded: usize = order.iter().map(|net| trees[net].unreachable().len()).sum();
    let (net, sink) = order
        .iter()
        .find_map(|net| trees[net].unreachable().first().map(|sink| (*net, *sink)))?;
    let source = source_of_net(net);
    let mut primary = format!(
        "routed netlist for {kind} `{name}` cannot reach ({x},{y},{z}) from the driver at ({sx},{sy},{sz}) — every route between them is blocked by a cell body, an I/O pad, or another net's dust, on the coord or one step from it in the same plane, and a wire passes through none of the three",
        kind = entry.kind.label(),
        name = entry.name,
        x = sink.x,
        y = sink.y,
        z = sink.z,
        sx = source.x,
        sy = source.y,
        sz = source.z,
    );
    let crowding = crowding_nets(nets, trees, &source_of_net, net, sink);
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
    let mut diag = Diagnostic::new(
        DiagnosticCode::RouteCongestion,
        region.span.clone(),
        primary,
    );
    diag = diag.with_footer(format!(
        "Fix: raise `void` above {void} so the wire has a layer to climb onto, enlarge `size=WxH`, or split into multiple `circuit` blocks",
        void = region.void,
    ));
    debug_assert_eq!(diag.severity(), Severity::Error);
    Some(diag)
}

/// The other nets whose dust keeps a wire out of the faces of `sink`,
/// in [`net_order`].
///
/// A face is one of the six rectilinear steps off the sink — the coords
/// a wire could have arrived through. A net takes one by drawing dust
/// on it, or by drawing dust [`beside`] it, which is the rule the
/// router routes by rather than a radius guessed to match it. A net's
/// own dust is not counted — it is not what stopped it — and neither is
/// anything on a terminal, because a terminal is a block and the
/// message already says blocks are in the way.
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
    let taken: HashSet<CellCoord> = STEPS
        .iter()
        .filter_map(|delta| stepped(sink, *delta))
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

/// Human-facing name for a net: the sensor's dotted `sig.<name>` when
/// the scope carries one at that index, `cell #j` for a synthesised
/// gate, which the Netlist IR has no source-level name for.
///
/// The `input pad #i` fall-back is for a hand-built IR whose input row
/// is shorter than the synthesis path implies; a diagnostic naming
/// nothing is worse than one naming an index.
fn net_label(net: NetRef, ir: &PlacementIr) -> String {
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
/// Laid in [`net_order`], each net going round the dust of the ones
/// before it and the coords [`beside`] that dust, so no two of the
/// trees run within one step of each other in one plane and no two
/// signals share a strand. The order is a total order over the nets of
/// a scope, so the map is one answer for one layout — it has to be,
/// because the routing, delay and crossing passes each call this and
/// have to be told the same thing.
///
/// A net with nowhere left to go is stranded rather than laid over the
/// dust in its way, or beside it; [`unroutable`] is what turns that
/// into a refusal.
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
            // Loud in release, and here rather than downstream. Two
            // nets one step apart in one plane are two signals on one
            // strand of dust, and there is no diagnostic left that
            // would name it: the codes that used to are gone, and the
            // routed paths do not reach the IR, so a consumer of a
            // wrong answer has nothing to inspect. `HashSet::extend`
            // throws the "was it already there" bool away; asking for
            // it is one branch per dust coord at the one place the
            // coord is claimed.
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
        // Widened after the whole net is claimed, not as each coord
        // goes in: a strand runs beside itself at every corner, so
        // widening on the way would have the claim above trip on this
        // net's own turn.
        keep_out.extend(dust.into_iter().flat_map(beside));
        trees.insert(net, tree);
    }
    trees
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use cairn_lang_core::error::Span;

    use super::*;
    use crate::netlist_ir::PortName;
    use crate::placement_ir::RouteLayer;

    fn reservation(width: u32, depth: u32) -> CircuitRegionReservation {
        CircuitRegionReservation {
            label: "floor".to_owned(),
            void: 1,
            width,
            depth,
            span: Span::default(),
        }
    }

    /// A reservation with an explicit service height.
    fn region(width: u32, depth: u32, void: u32) -> CircuitRegionReservation {
        CircuitRegionReservation {
            void,
            ..reservation(width, depth)
        }
    }

    /// A router over `region` with a cell body on each of `blocks`.
    ///
    /// What kind of block it is only matters to the diagnostic that
    /// names it, so the fixtures below say `Cell` and mean "something
    /// is standing here". Every fixture lists its own source and sinks
    /// among them, because the pipeline's are components: a sink that
    /// is not a block would be walked through rather than ended at, and
    /// [`Router::tree`] debug-asserts against exactly that.
    fn router(region: &CircuitRegionReservation, blocks: &[CellCoord]) -> Router {
        let sites: Vec<BlockSite> = blocks
            .iter()
            .enumerate()
            .map(|(index, coord)| BlockSite {
                coord: *coord,
                kind: BlockKind::Cell,
                index,
            })
            .collect();
        Router::new(region, &sites)
    }

    /// No net laid before this one.
    ///
    /// Spelled at every call rather than defaulted, so a test whose
    /// subject is the router against the blocks says that it is, and
    /// the tests whose subject is the dust of an earlier net read as
    /// the other case of the same question.
    fn nothing_in_the_way() -> HashSet<CellCoord> {
        HashSet::new()
    }

    /// The dust one net lays with nothing in its way.
    fn dust_of(router: &Router, source: CellCoord, sinks: &[CellCoord]) -> Vec<CellCoord> {
        router.dust(&router.tree(source, sinks, &nothing_in_the_way()))
    }

    /// What that dust keeps the next net out of: the dust, and the
    /// coords [`beside`] it — the obstacle set as [`net_trees`] builds
    /// it.
    fn keep_out(router: &Router, source: CellCoord, sinks: &[CellCoord]) -> HashSet<CellCoord> {
        let dust = dust_of(router, source, sinks);
        dust.iter()
            .copied()
            .chain(dust.iter().copied().flat_map(beside))
            .collect()
    }

    #[test]
    fn manhattan_sums_absolute_axis_deltas() {
        assert_eq!(
            manhattan(CellCoord::new(0, 0, 0), CellCoord::new(3, 4, 5)),
            12
        );
        assert_eq!(
            manhattan(CellCoord::new(5, 5, 5), CellCoord::new(5, 5, 5)),
            0
        );
        assert_eq!(
            manhattan(CellCoord::new(3, 4, 5), CellCoord::new(0, 0, 0)),
            12
        );
    }

    #[test]
    fn net_ref_key_orders_input_before_cell_then_by_index() {
        assert!(net_ref_key(NetRef::Input(u32::MAX)) < net_ref_key(NetRef::Cell(0)));
        assert!(net_ref_key(NetRef::Input(0)) < net_ref_key(NetRef::Input(1)));
        assert!(net_ref_key(NetRef::Cell(0)) < net_ref_key(NetRef::Cell(1)));
    }

    /// The fold charges each net once however far apart its ports sit
    /// in the driver list.
    ///
    /// The two passes that call this both reach it with the repeated
    /// net *adjacent* — a cell whose `a` and `b` read one signal — so
    /// a seen-list that only remembered the previous driver would
    /// satisfy every fixture in the crate while charging a Mux shaped
    /// `[Sel → net 0, A → net 1, B → net 0]` twice for one strand.
    /// Called directly because that shape has no producer to reach it
    /// through.
    #[test]
    fn each_driving_net_is_charged_once_however_the_ports_are_ordered() {
        let drivers = vec![
            CellPortDriver {
                port: PortName::Sel,
                net: NetRef::Input(0),
            },
            CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            },
            CellPortDriver {
                port: PortName::B,
                net: NetRef::Input(0),
            },
        ];
        let mut charged: Vec<NetRef> = Vec::new();
        let total = sum_over_driving_nets(&drivers, |net| {
            charged.push(net);
            match net {
                NetRef::Input(0) => 10,
                NetRef::Input(1) => 3,
                other => panic!("no other net drives this cell: {other:?}"),
            }
        });
        assert_eq!(
            charged,
            vec![NetRef::Input(0), NetRef::Input(1)],
            "one call per net, in first-appearance order",
        );
        assert_eq!(total, 13, "10 for the repeated net, once, plus 3");
    }

    #[test]
    fn input_pad_saturates_at_depth_minus_one() {
        let region = reservation(10, 3);
        assert_eq!(input_pad(0, &region), CellCoord::new(0, 0, 0));
        assert_eq!(input_pad(2, &region), CellCoord::new(0, 0, 2));
        // depth-1 = 2 ceilings anything past the third input.
        assert_eq!(input_pad(5, &region), CellCoord::new(0, 0, 2));
    }

    #[test]
    fn output_pad_sits_on_right_edge_and_saturates_z() {
        let region = reservation(4, 3);
        assert_eq!(output_pad(0, &region), CellCoord::new(3, 0, 0));
        assert_eq!(output_pad(2, &region), CellCoord::new(3, 0, 2));
        assert_eq!(output_pad(5, &region), CellCoord::new(3, 0, 2));
    }

    /// Where nothing is in the way the search lays the L-shape the
    /// passes were built around: x, then z, then y.
    ///
    /// The axis order is a regression fence — every pinned
    /// `wire_length` in the crate, and the coord each buffer repeater
    /// stands on, is measured along whatever shape this produces.
    #[test]
    fn a_clear_run_is_the_l_shape_the_passes_were_built_around() {
        let region = region(4, 3, 2);
        let source = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(2, 1, 1);
        let tree = router(&region, &[source, sink]).tree(source, &[sink], &nothing_in_the_way());
        assert_eq!(
            tree.route_to(CellCoord::new(2, 1, 1)),
            Some(vec![
                CellCoord::new(0, 0, 0),
                CellCoord::new(1, 0, 0),
                CellCoord::new(2, 0, 0),
                CellCoord::new(2, 0, 1),
                CellCoord::with_layer(2, 1, 1, RouteLayer::Bridge),
            ]),
        );
    }

    /// The axis order is a preference between axes, not between
    /// directions, so a net that runs the other way lays the mirror of
    /// the same shape rather than a different one.
    #[test]
    fn a_clear_run_backwards_still_walks_x_before_z() {
        let region = region(4, 3, 1);
        let source = CellCoord::new(3, 0, 2);
        let sink = CellCoord::new(1, 0, 0);
        let tree = router(&region, &[source, sink]).tree(source, &[sink], &nothing_in_the_way());
        assert_eq!(
            tree.route_to(CellCoord::new(1, 0, 0)),
            Some(vec![
                CellCoord::new(3, 0, 2),
                CellCoord::new(2, 0, 2),
                CellCoord::new(1, 0, 2),
                CellCoord::new(1, 0, 1),
                CellCoord::new(1, 0, 0),
            ]),
        );
    }

    /// The defect this module was rewritten for, in its smallest form:
    /// a block on the straight line between two terminals. The wire
    /// goes around it, and pays the two blocks that costs.
    #[test]
    fn the_wire_goes_around_a_block_rather_than_through_it() {
        let region = region(5, 3, 1);
        let source = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(2, 0, 0);
        let blocked = router(&region, &[source, sink, CellCoord::new(1, 0, 0)]);

        assert_eq!(
            blocked
                .tree(source, &[sink], &nothing_in_the_way())
                .route_to(sink),
            Some(vec![
                CellCoord::new(0, 0, 0),
                CellCoord::new(0, 0, 1),
                CellCoord::new(1, 0, 1),
                CellCoord::new(2, 0, 1),
                CellCoord::new(2, 0, 0),
            ]),
        );
        assert_eq!(
            router(&region, &[source, sink])
                .tree(source, &[sink], &nothing_in_the_way())
                .route_to(sink)
                .map(|route| route.len()),
            Some(3),
            "with nothing in the way the same pair is two blocks apart",
        );
    }

    /// With no room beside the block, the wire climbs over it — and
    /// what it climbs onto is a bridge coord, the same layer the
    /// crossing pass lifts a repeater to. One rule for the height, so
    /// the two cannot key past each other.
    #[test]
    fn a_wire_with_no_room_beside_a_block_climbs_over_it() {
        let region = region(3, 1, 2);
        let source = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(2, 0, 0);
        let tree = router(&region, &[source, sink, CellCoord::new(1, 0, 0)]).tree(
            source,
            &[sink],
            &nothing_in_the_way(),
        );
        assert_eq!(
            tree.route_to(sink),
            Some(vec![
                CellCoord::new(0, 0, 0),
                CellCoord::with_layer(0, 1, 0, RouteLayer::Bridge),
                CellCoord::with_layer(1, 1, 0, RouteLayer::Bridge),
                CellCoord::with_layer(2, 1, 0, RouteLayer::Bridge),
                CellCoord::new(2, 0, 0),
            ]),
        );
    }

    /// A sink is where the dust ends. The signal into the far cell of
    /// a row does not pass through the near ones — a comparator hands
    /// on its own output, not the wire that fed it — so every sink is
    /// a leaf and the trunk runs beside the row.
    ///
    /// This is the shape the router was rewritten for: the spanning
    /// tree it replaced reached the far cell *through* the near ones.
    #[test]
    fn a_row_of_sinks_is_fed_from_beside_it_and_never_through_itself() {
        let region = region(6, 3, 1);
        let source = CellCoord::new(0, 0, 2);
        let row: Vec<CellCoord> = (0..4).map(|x| CellCoord::new(x, 0, 0)).collect();
        let tree = router(&region, &row).tree(source, &row, &nothing_in_the_way());

        for sink in &row {
            let route = tree.route_to(*sink).expect("a sink of this net");
            let through: Vec<CellCoord> = route[1..route.len() - 1]
                .iter()
                .copied()
                .filter(|coord| row.contains(coord))
                .collect();
            assert!(
                through.is_empty(),
                "the route to {sink:?} passes through {through:?}",
            );
        }
        assert_eq!(
            tree.route_to(CellCoord::new(3, 0, 0)),
            Some(vec![
                CellCoord::new(0, 0, 2),
                CellCoord::new(0, 0, 1),
                CellCoord::new(1, 0, 1),
                CellCoord::new(2, 0, 1),
                CellCoord::new(3, 0, 1),
                CellCoord::new(3, 0, 0),
            ]),
            "the far cell is reached along the trunk the nearer ones hang off",
        );
    }

    /// A second sink hangs off the dust the first one laid rather than
    /// getting a strand of its own — which is what makes one repeater
    /// serve both.
    #[test]
    fn a_second_sink_hangs_off_the_trunk_the_first_one_laid() {
        let region = region(8, 2, 1);
        let source = CellCoord::new(0, 0, 0);
        let near = CellCoord::new(3, 0, 1);
        let far = CellCoord::new(6, 0, 1);
        let tree = router(&region, &[near, far]).tree(source, &[far, near], &nothing_in_the_way());

        let to_near = tree.route_to(near).expect("a sink of this net");
        let to_far = tree.route_to(far).expect("a sink of this net");
        assert_eq!(
            to_far[..to_near.len() - 1],
            to_near[..to_near.len() - 1],
            "the far route leaves the near one only at the cell itself",
        );
        assert_eq!(tree.wire_path().len(), 9, "one trunk, not two strands");
    }

    /// The tree lists each of its coords once.
    ///
    /// A consumer that folds `wire_path` into a per-coord map reads
    /// this as "one net is recorded at one coord once", and stage 4
    /// does exactly that to find the coords two nets share — a coord a
    /// net was listed at twice would read as that net crossing itself.
    /// The layout carries all three ways a coord enters the tree: a
    /// straight run, a search around a block, and a sink with no way
    /// in at all, which is attached to the source by a different path
    /// than the other two.
    #[test]
    fn a_tree_lists_each_of_its_coords_once() {
        let region = region(5, 3, 2);
        let source = CellCoord::new(0, 0, 1);
        let near = CellCoord::new(2, 0, 1);
        let walled = CellCoord::new(4, 0, 0);
        let blocks = [
            source,
            near,
            walled,
            CellCoord::new(3, 0, 0),
            CellCoord::new(4, 0, 1),
            CellCoord::new(4, 1, 0),
        ];
        let tree = router(&region, &blocks).tree(source, &[near, walled], &nothing_in_the_way());
        assert_eq!(
            tree.unreachable(),
            [walled],
            "the fixture only covers the stranded path while one sink is stranded",
        );

        let path = tree.wire_path();
        let mut seen: Vec<CellCoord> = path.clone();
        seen.sort_unstable_by_key(|coord| coord_key(*coord));
        seen.dedup();
        assert_eq!(seen.len(), path.len(), "a coord is listed once: {path:?}");
    }

    /// A sink walled in on every side is refused rather than wired
    /// through what walls it in. The tree still answers for it so both
    /// projections stay total; what the routing pass reads is
    /// `unreachable`.
    #[test]
    fn a_walled_in_sink_is_reported_rather_than_wired_through() {
        let region = region(3, 2, 1);
        let source = CellCoord::new(0, 0, 1);
        let sink = CellCoord::new(2, 0, 0);
        let walls = [
            sink,
            source,
            CellCoord::new(1, 0, 0),
            CellCoord::new(2, 0, 1),
        ];
        let tree = router(&region, &walls).tree(source, &[sink], &nothing_in_the_way());

        assert_eq!(tree.unreachable(), [sink]);
        assert_eq!(tree.route_to(sink), Some(vec![source, sink]));
        assert!(
            tree.wire_path().contains(&sink),
            "an unreachable sink is still a coord of the net",
        );
    }

    /// The same layout with one layer more has somewhere to go, so the
    /// refusal above is about the reservation rather than about the
    /// sink.
    #[test]
    fn the_same_sink_is_reachable_once_there_is_a_layer_to_climb_to() {
        let region = region(3, 2, 2);
        let source = CellCoord::new(0, 0, 1);
        let sink = CellCoord::new(2, 0, 0);
        let walls = [
            sink,
            source,
            CellCoord::new(1, 0, 0),
            CellCoord::new(2, 0, 1),
        ];
        let tree = router(&region, &walls).tree(source, &[sink], &nothing_in_the_way());

        assert!(tree.unreachable().is_empty());
        assert_eq!(
            tree.route_to(sink),
            Some(vec![
                source,
                CellCoord::new(0, 0, 0),
                CellCoord::with_layer(0, 1, 0, RouteLayer::Bridge),
                CellCoord::with_layer(1, 1, 0, RouteLayer::Bridge),
                CellCoord::with_layer(2, 1, 0, RouteLayer::Bridge),
                sink,
            ]),
        );
    }

    /// Adjacent terminals need nothing between them, and the router
    /// adds nothing.
    #[test]
    fn a_sink_beside_the_source_is_two_coords() {
        let region = region(4, 2, 1);
        let source = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(1, 0, 0);
        assert_eq!(
            router(&region, &[source, sink])
                .tree(source, &[sink], &nothing_in_the_way())
                .route_to(sink),
            Some(vec![source, sink]),
        );
    }

    #[test]
    fn a_net_with_no_sinks_still_occupies_its_source() {
        let region = region(4, 2, 1);
        let source = CellCoord::new(1, 0, 1);
        let tree = router(&region, &[]).tree(source, &[], &nothing_in_the_way());
        assert_eq!(tree.wire_path(), vec![source]);
        assert_eq!(tree.route_to(source), Some(vec![source]));
    }

    /// The source is its own sink at distance zero — the case a driver
    /// whose sink coincides with its source takes, and the one that
    /// must not report a buffer's worth of dust.
    #[test]
    fn a_sink_that_is_the_source_is_a_single_coord() {
        let region = region(4, 2, 1);
        let source = CellCoord::new(3, 0, 1);
        let other = CellCoord::new(0, 0, 1);
        let tree =
            router(&region, &[source, other]).tree(source, &[source, other], &nothing_in_the_way());
        assert_eq!(tree.route_to(source), Some(vec![source]));
    }

    /// A coord that is not part of this net gets no route rather than a
    /// plausible-looking one. The callers turn `None` into a panic
    /// naming the disagreement, which beats measuring a segment
    /// against a net that does not reach it.
    #[test]
    fn route_to_a_foreign_sink_is_none() {
        let region = region(6, 3, 1);
        let source = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(4, 0, 0);
        let tree = router(&region, &[source, sink]).tree(source, &[sink], &nothing_in_the_way());
        assert_eq!(tree.route_to(CellCoord::new(4, 0, 2)), None);
    }

    /// Two detours of equal length exist around this block — over the
    /// top and around the side. Which one the router takes is a fact
    /// every pinned coord downstream depends on, so it is pinned here
    /// rather than left to whichever the search reached first.
    #[test]
    fn equally_short_detours_are_decided_by_the_axis_order() {
        let region = region(3, 2, 2);
        let source = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(2, 0, 0);
        let tree = router(&region, &[source, sink, CellCoord::new(1, 0, 0)]).tree(
            source,
            &[sink],
            &nothing_in_the_way(),
        );
        let route = tree.route_to(sink).expect("a sink of this net");
        assert_eq!(route.len(), 5, "both ways round cost the same two blocks");
        assert_eq!(
            route[1],
            CellCoord::new(0, 0, 1),
            "z comes before y, so the wire goes round rather than over: {route:?}",
        );
    }

    /// A coord reached twice for the same cost keeps the parent that
    /// got there first.
    ///
    /// The search relaxes on a strictly cheaper `g`, so a second route
    /// arriving at the same price is dropped. Relaxing on a tie instead
    /// leaves the wire identical — the same coords in the same order —
    /// and moves the *routes* through it: on this layout the sink at
    /// `(4,0,1)` would hang off `(2,0,1)` and read six blocks where it
    /// reads eight here. `wire_length`, `delay_ticks`, and every buffer
    /// coord are measured along that path, so the tie is not free to
    /// drift.
    ///
    /// Neither answer is shorter wire and neither is wrong; what is
    /// wrong is the number changing without anyone saying so. Found by
    /// sweeping the two relaxations over random layouts and diffing the
    /// routes — 5 of 4000 differ, and no fixture in the crate was one
    /// of them.
    #[test]
    fn a_coord_reached_twice_for_one_price_keeps_the_first_parent() {
        let region = region(6, 4, 1);
        let source = CellCoord::new(3, 0, 2);
        let sinks = [
            CellCoord::new(4, 0, 1),
            CellCoord::new(1, 0, 0),
            CellCoord::new(3, 0, 1),
            CellCoord::new(4, 0, 2),
        ];
        let mut blocks = vec![source];
        blocks.extend(sinks);
        let tree = router(&region, &blocks).tree(source, &sinks, &nothing_in_the_way());
        assert_eq!(
            tree.route_to(CellCoord::new(4, 0, 1)),
            Some(vec![
                CellCoord::new(3, 0, 2),
                CellCoord::new(2, 0, 2),
                CellCoord::new(1, 0, 2),
                CellCoord::new(1, 0, 1),
                CellCoord::new(2, 0, 1),
                CellCoord::new(2, 0, 0),
                CellCoord::new(3, 0, 0),
                CellCoord::new(4, 0, 0),
                CellCoord::new(4, 0, 1),
            ]),
        );
    }

    /// The tree is a function of its terminals as a set. Asking for the
    /// same sinks in another order is the same wire, so two passes
    /// walking the same layout cannot disagree about it.
    #[test]
    fn the_order_the_sinks_are_asked_for_does_not_change_the_wire() {
        let region = region(8, 4, 2);
        let source = CellCoord::new(0, 0, 3);
        let sinks = [
            CellCoord::new(5, 0, 0),
            CellCoord::new(1, 0, 0),
            CellCoord::new(3, 0, 2),
        ];
        let router = router(&region, &sinks);
        let forward = router.tree(source, &sinks, &nothing_in_the_way());
        let mut backward_sinks = sinks;
        backward_sinks.reverse();
        assert_eq!(
            forward,
            router.tree(source, &backward_sinks, &nothing_in_the_way())
        );
    }

    /// `collect_nets` is the sink side every pass shares: a cell driver
    /// sinks at the cell body, an output driver at the actuator's pad.
    /// What a strand of dust reaches is its own plane.
    ///
    /// The four in-plane steps and no others: up and down are left to
    /// the physical tile layer, because whether dust at `y + 1` reads
    /// the dust below it depends on what is standing between them and
    /// the pseudo-2.5D model does not carry that. Spelled out here
    /// rather than derived from [`STEPS`], so widening the rule to six
    /// steps has to be written down twice.
    #[test]
    fn a_strand_reaches_the_four_coords_in_its_own_plane() {
        let mut reached: Vec<CellCoord> = beside(CellCoord::new(2, 1, 3)).collect();
        reached.sort_by_key(|coord| coord_key(*coord));
        assert_eq!(
            reached,
            vec![
                CellCoord::new(1, 1, 3),
                CellCoord::new(2, 1, 2),
                CellCoord::new(2, 1, 4),
                CellCoord::new(3, 1, 3),
            ],
        );
        assert!(
            reached.iter().all(|coord| coord.y == 1),
            "a strand does not reach the layer above or below it: {reached:?}",
        );
        assert_eq!(
            beside(CellCoord::new(0, 0, 0)).count(),
            2,
            "and the steps off the `0` edge are not coords at all",
        );
    }

    /// A net does not run in the lane beside another net's dust, even
    /// when that lane is empty.
    ///
    /// The coord-disjoint rule would let the second net take the row
    /// next door and call it wired; two strands one step apart are one
    /// strand carrying two signals. `void=1`, so the way round is on
    /// the plane or nowhere — this is about the width of the obstacle
    /// set, not about the escape.
    #[test]
    fn a_net_does_not_run_in_the_lane_beside_another_nets_dust() {
        let region = region(5, 4, 1);
        let first_source = CellCoord::new(0, 0, 0);
        let first_sink = CellCoord::new(4, 0, 0);
        let second_source = CellCoord::new(0, 0, 1);
        let second_sink = CellCoord::new(4, 0, 1);
        let router = router(
            &region,
            &[first_source, first_sink, second_source, second_sink],
        );

        let lane = CellCoord::new(2, 0, 1);
        let first = dust_of(&router, first_source, &[first_sink]);
        assert!(
            !first.contains(&lane),
            "the fixture needs the second net's own row clear of the first's \
             dust, so that only the one-step rule keeps it off: {first:?}",
        );

        let tree = router.tree(
            second_source,
            &[second_sink],
            &keep_out(&router, first_source, &[first_sink]),
        );
        let second = router.dust(&tree);
        assert!(
            tree.unreachable().is_empty(),
            "the second net still gets there: {:?}",
            tree.unreachable(),
        );
        assert!(
            !second.contains(&lane),
            "but not down the lane beside the first: {second:?}",
        );
        assert!(
            second
                .iter()
                .all(|coord| first.iter().all(|other| manhattan(*coord, *other) > 1)),
            "and no coord of one is a step from a coord of the other: {second:?}",
        );
    }

    /// A net is not kept out by its own dust.
    ///
    /// The rule is between nets, not inside one: consecutive coords of
    /// one strand are one step apart by definition, so a strand runs
    /// beside itself the whole way. That is why [`net_trees`] widens
    /// the obstacle set once a net is laid rather than as each coord
    /// goes into it — the second assertion is what widening on the way
    /// would do to every net there is.
    #[test]
    fn a_net_is_not_kept_out_by_its_own_dust() {
        // One row and one layer, so the keep-out set has nowhere to
        // send the net that the region still contains.
        let region = region(5, 1, 1);
        let source = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(4, 0, 0);
        let router = router(&region, &[source, sink]);

        let dust = dust_of(&router, source, &[sink]);
        assert!(
            dust.windows(2).any(|pair| manhattan(pair[0], pair[1]) == 1),
            "a strand runs beside itself at every step: {dust:?}",
        );
        let mine = keep_out(&router, source, &[sink]);
        assert_eq!(
            router.tree(source, &[sink], &mine).unreachable(),
            [sink],
            "so a net routed against its own keep-out set has nowhere to go",
        );
    }

    /// A clear straight line is not clear when another net is on it,
    /// or one step from it.
    ///
    /// The shortcut that answers an unobstructed run without searching
    /// has its own view of what is in the way, so it needs the same
    /// obstacle set the search gets. Without it a run whose ends are
    /// clear lays this net's dust straight over the last net's, and
    /// nothing downstream ever asks again.
    #[test]
    fn a_straight_run_does_not_cross_the_dust_already_laid() {
        // `void=1`, so the way round is on the plane or nowhere: this
        // test is about the shortcut's own view of the obstacle set,
        // not about the escape.
        let region = region(5, 3, 1);
        let source = CellCoord::new(2, 0, 0);
        let sink = CellCoord::new(2, 0, 2);
        let other_source = CellCoord::new(0, 0, 1);
        let other_sink = CellCoord::new(3, 0, 1);
        let router = router(&region, &[source, sink, other_source, other_sink]);

        assert_eq!(
            router
                .tree(source, &[sink], &nothing_in_the_way())
                .wire_path(),
            vec![source, CellCoord::new(2, 0, 1), sink],
            "with nothing in the way the run is the straight line",
        );

        assert!(
            dust_of(&router, other_source, &[other_sink]).contains(&CellCoord::new(2, 0, 1)),
            "the fixture needs the first net across the second's line",
        );
        let across = keep_out(&router, other_source, &[other_sink]);
        let tree = router.tree(source, &[sink], &across);
        assert!(
            tree.unreachable().is_empty(),
            "the second net still gets there: {:?}",
            tree.unreachable(),
        );
        assert!(
            router
                .dust(&tree)
                .iter()
                .all(|coord| !across.contains(coord)),
            "by going round rather than over, and keeping a coord clear \
             between the two: {:?}",
            router.dust(&tree),
        );
    }

    /// With nowhere to go round, the way out is up.
    ///
    /// `spec/redstone` §14.5 calls it an escape, and it is the search
    /// already climbing over a block, asked the same question about a
    /// signal. The layer is what makes the coord `Bridge`, so the
    /// assertion spells the layer rather than reading it off the
    /// height the way the constructor does.
    ///
    /// The run at `y=1` passes directly over the wall's dust at
    /// `(1,0,1)`, which is the per-plane half of the rule in the one
    /// place it decides an answer: were up and down in [`beside`],
    /// there would be no escape at all.
    #[test]
    fn a_net_with_no_way_round_climbs_over_the_dust_in_its_way() {
        let region = region(3, 3, 2);
        let source = CellCoord::new(1, 0, 0);
        let sink = CellCoord::new(1, 0, 2);
        let wall_source = CellCoord::new(0, 0, 1);
        let wall_sink = CellCoord::new(2, 0, 1);
        let router = router(&region, &[source, sink, wall_source, wall_sink]);

        assert_eq!(
            dust_of(&router, wall_source, &[wall_sink]),
            vec![CellCoord::new(1, 0, 1)],
            "the fixture needs the row between the two sealed",
        );
        let wall = keep_out(&router, wall_source, &[wall_sink]);
        let tree = router.tree(source, &[sink], &wall);
        assert_eq!(
            tree.route_to(sink).expect("a sink of this net"),
            vec![
                source,
                CellCoord::with_layer(1, 1, 0, RouteLayer::Bridge),
                CellCoord::with_layer(1, 1, 1, RouteLayer::Bridge),
                CellCoord::with_layer(1, 1, 2, RouteLayer::Bridge),
                sink,
            ],
            "the escape is a climb, a run along the layer above, and a drop",
        );
    }

    /// Two drivers of one cell both arrive.
    ///
    /// A cell body is a terminal of every net that drives it, and the
    /// obstacle set is dust — so the coord where the first net ends is
    /// not something the second has to go round. Keeping the terminals
    /// in would strand the second driver of every two-input gate there
    /// is, which is most of them.
    #[test]
    fn a_second_driver_of_one_cell_still_arrives_at_it() {
        let region = region(4, 3, 2);
        let cell = CellCoord::new(2, 0, 0);
        let first = CellCoord::new(0, 0, 0);
        let second = CellCoord::new(0, 0, 2);
        let router = router(&region, &[cell, first, second]);

        let dust = dust_of(&router, first, &[cell]);
        assert!(
            !dust.contains(&cell),
            "the cell body is a terminal, not dust: {dust:?}",
        );
        let tree = router.tree(second, &[cell], &keep_out(&router, first, &[cell]));
        assert!(
            tree.unreachable().is_empty(),
            "the second driver has to reach the cell it drives: {:?}",
            tree.unreachable(),
        );
    }

    /// A net with nowhere left to go is stranded rather than laid over
    /// what is in its way.
    ///
    /// This is the trade the pipeline makes: a layout whose last net
    /// cannot be wired is refused by `unroutable` at stage 2 instead of
    /// compiling into two signals on one strand of dust.
    #[test]
    fn a_net_with_nowhere_left_to_go_is_stranded_rather_than_shorted() {
        let region = region(3, 3, 1);
        let source = CellCoord::new(1, 0, 0);
        let sink = CellCoord::new(1, 0, 2);
        let wall_source = CellCoord::new(0, 0, 1);
        let wall_sink = CellCoord::new(2, 0, 1);
        let router = router(&region, &[source, sink, wall_source, wall_sink]);

        let wall = keep_out(&router, wall_source, &[wall_sink]);
        let tree = router.tree(source, &[sink], &wall);
        assert_eq!(
            tree.unreachable(),
            [sink],
            "with no layer above the plane there is nowhere to escape to",
        );
        assert!(
            router.dust(&tree).iter().all(|coord| !wall.contains(coord)),
            "and the answer is not to lay it over the wire in the way: {:?}",
            tree.wire_path(),
        );
    }

    /// [`net_trees`] lays the nets in [`net_order`], so the net with
    /// the wider fanout keeps the direct run and the other goes round.
    ///
    /// Reversing the order would be just as deterministic and would
    /// produce a different layout, which is why this pins which of the
    /// two it is: the three passes that call this each rebuild the
    /// trees, and a disagreement between them is a dump whose ticks
    /// and coords describe different circuits.
    #[test]
    fn the_wider_fanout_is_laid_first_and_keeps_its_run() {
        let region = region(5, 3, 2);
        let wide_source = CellCoord::new(0, 0, 1);
        let narrow_source = CellCoord::new(4, 0, 1);
        let near = CellCoord::new(2, 0, 0);
        let far = CellCoord::new(2, 0, 2);
        let blocks = [wide_source, narrow_source, near, far];
        let router = router(&region, &blocks);

        let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
        nets.insert(NetRef::Input(0), vec![near, far]);
        nets.insert(NetRef::Cell(0), vec![near]);
        let trees = net_trees(&nets, &router, |net| match net {
            NetRef::Input(0) => wide_source,
            _ => narrow_source,
        });

        let wide: HashSet<CellCoord> = router.dust(&trees[&NetRef::Input(0)]).into_iter().collect();
        let narrow: HashSet<CellCoord> =
            router.dust(&trees[&NetRef::Cell(0)]).into_iter().collect();
        assert!(
            wide.contains(&CellCoord::new(1, 0, 1)) && wide.contains(&CellCoord::new(2, 0, 1)),
            "the two-sink net is laid first and takes the middle row: {wide:?}",
        );
        assert!(
            wide.is_disjoint(&narrow),
            "and the one-sink net goes round it: {wide:?} vs {narrow:?}",
        );
    }

    /// [`Router::dust`] is the tree without its terminals.
    ///
    /// Derived rather than tracked: a path this router laid runs
    /// through no block and ends at one, so the blocks on a tree are
    /// exactly its source and its sinks.
    #[test]
    fn the_dust_of_a_tree_is_everything_but_its_terminals() {
        let region = region(4, 2, 1);
        let source = CellCoord::new(0, 0, 0);
        let sink = CellCoord::new(3, 0, 0);
        let router = router(&region, &[source, sink]);
        let tree = router.tree(source, &[sink], &nothing_in_the_way());
        assert_eq!(
            router.dust(&tree),
            vec![CellCoord::new(1, 0, 0), CellCoord::new(2, 0, 0)],
            "the two ends are blocks; what is between them is dust",
        );
    }

    #[test]
    fn collect_nets_maps_cell_drivers_to_bodies_and_outputs_to_pads() {
        let region = reservation(8, 4);
        let ir = one_cell_ir(&region);

        let nets = collect_nets(&ir);
        assert_eq!(nets[&NetRef::Input(0)], vec![CellCoord::new(2, 0, 1)]);
        assert_eq!(nets[&NetRef::Cell(0)], vec![output_pad(0, &region)]);
    }

    /// Two ports of one cell on one net are one sink.
    ///
    /// `sig.a and sig.a` reaches the placement IR as a cell with two
    /// drivers on one net, and one strand of dust arrives at one block.
    /// The list's length is the fanout [`net_order`] sorts on, so a
    /// duplicate here would lay this net before one that really does
    /// feed two sinks — and after this change the order is where every
    /// net's dust goes.
    #[test]
    fn two_ports_of_one_cell_on_one_net_are_one_sink() {
        let region = reservation(8, 4);
        let mut ir = one_cell_ir(&region);
        ir.cells[0].drivers.push(CellPortDriver {
            port: PortName::B,
            net: NetRef::Input(0),
        });

        let nets = collect_nets(&ir);
        assert_eq!(
            nets[&NetRef::Input(0)],
            vec![CellCoord::new(2, 0, 1)],
            "one cell body, however many of its ports read the net",
        );
        assert_eq!(
            net_order(&nets),
            vec![NetRef::Input(0), NetRef::Cell(0)],
            "and one apiece, so the tie is broken by the key rather than by a \
             fanout one of them does not have",
        );
    }

    /// A net is not reported as standing in its own way.
    ///
    /// For a sink inside the reservation the question cannot arise: a
    /// coord of this net's dust one step from the sink is a coord the
    /// router could have stepped off, so the sink would not be
    /// stranded. What is left is a sink outside the reservation, which
    /// a hand-built IR can carry and which `step` refuses to enter —
    /// and there the net's own dust really can run beside it.
    ///
    /// `sig.b` here reaches its first sink along the row and strands on
    /// a second one past the edge, with the coord it laid at `(3,0,0)`
    /// next to it. Naming itself would answer "why can `sig.b` not get
    /// there" with "`sig.b` is in the way".
    #[test]
    fn a_stranded_net_is_not_named_as_crowding_its_own_sink() {
        let region = region(4, 3, 1);
        let own_source = CellCoord::new(0, 0, 0);
        let own_sink = CellCoord::new(3, 0, 1);
        let past_the_edge = CellCoord::new(4, 0, 0);
        let other_source = CellCoord::new(0, 0, 2);
        let other_sink = CellCoord::new(2, 0, 2);
        let router = router(
            &region,
            &[
                own_source,
                own_sink,
                past_the_edge,
                other_source,
                other_sink,
            ],
        );

        let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
        nets.insert(NetRef::Input(0), vec![other_sink]);
        nets.insert(NetRef::Cell(0), vec![own_sink, past_the_edge]);
        let source_of_net = |net: NetRef| match net {
            NetRef::Input(_) => other_source,
            NetRef::Cell(_) => own_source,
        };
        let trees = net_trees(&nets, &router, source_of_net);

        assert_eq!(
            trees[&NetRef::Cell(0)].unreachable(),
            [past_the_edge],
            "the fixture needs the sink past the edge stranded",
        );
        assert!(
            router
                .dust(&trees[&NetRef::Cell(0)])
                .contains(&CellCoord::new(3, 0, 0)),
            "and its own dust beside that sink",
        );

        assert_eq!(
            crowding_nets(
                &nets,
                &trees,
                &source_of_net,
                NetRef::Cell(0),
                past_the_edge,
            ),
            Vec::<NetRef>::new(),
            "a net does not stand in its own way",
        );
    }

    /// `block_sites` is the other half of the same contract: the coords
    /// the router must not draw dust through, in one list so the three
    /// passes cannot disagree about what is standing where.
    #[test]
    fn block_sites_lists_cells_then_input_pads_then_output_pads() {
        let region = reservation(8, 4);
        let ir = one_cell_ir(&region);

        let sites = block_sites(&ir, &region);
        assert_eq!(
            sites
                .iter()
                .map(|site| (site.kind, site.index, site.coord))
                .collect::<Vec<_>>(),
            vec![
                (BlockKind::Cell, 0, CellCoord::new(2, 0, 1)),
                (BlockKind::InputPad, 0, CellCoord::new(0, 0, 0)),
                (BlockKind::OutputPad, 0, CellCoord::new(7, 0, 0)),
            ],
        );
    }

    /// One cell reading input 0, driving one actuator pad.
    fn one_cell_ir(region: &CircuitRegionReservation) -> PlacementIr {
        use cairn_lang_core::Edition;
        use cairn_lang_core::ast::DottedRef;

        use crate::edition_netlist_ir::EditionCell;
        use crate::netlist_ir::NetlistInput;
        use crate::placement_ir::{PlacedCellNode, PlacedOutputNode, PlacementPhase};

        let mut ir = PlacementIr::new(Edition::Java);
        ir.inputs.push(NetlistInput {
            name: DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers: vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            coord: CellCoord::new(2, 0, 1),
            phase: PlacementPhase::Unrouted,
            span: Span::default(),
        });
        ir.outputs.push(PlacedOutputNode::new(
            DottedRef::new("sig".into(), vec!["out".into()]),
            NetRef::Cell(0),
            output_pad(0, region),
            Span::default(),
        ));
        ir
    }

    /// Net order is fanout descending, ties by [`net_ref_key`]. Pinned
    /// because three passes walk it and a `HashMap`'s own order would
    /// make their outputs differ run to run.
    #[test]
    fn net_order_sorts_by_fanout_then_key() {
        let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
        nets.insert(NetRef::Cell(0), vec![CellCoord::new(1, 0, 0)]);
        nets.insert(
            NetRef::Input(1),
            vec![CellCoord::new(2, 0, 0), CellCoord::new(3, 0, 0)],
        );
        nets.insert(NetRef::Input(0), vec![CellCoord::new(4, 0, 0)]);
        assert_eq!(
            net_order(&nets),
            vec![NetRef::Input(1), NetRef::Input(0), NetRef::Cell(0)],
        );
    }

    mod route_invariants {
        //! Property coverage for the facts every consumer of
        //! [`NetTree`] leans on: a route is part of the wire, it is a
        //! walk, it ends where the dust ends, and it never crosses a
        //! block. The example fixtures above are all layouts a v1
        //! placement produces — a row of cells and a column of pads —
        //! and the router has to be right about the ones it does not.
        use proptest::prelude::*;

        use super::{
            BlockKind, BlockSite, CellCoord, CircuitRegionReservation, HashMap, HashSet, NetRef,
            Router, Span, beside, manhattan, net_trees, nothing_in_the_way,
        };

        const WIDTH: u32 = 9;
        const DEPTH: u32 = 4;

        /// A source, up to four sinks, and up to six loose blocks on
        /// the ground layer of a 9x2x4 box, which is where the
        /// placement pass puts every block there is. The box is small
        /// enough that they regularly wall something in, and the second
        /// layer is there for the router to climb to.
        fn layout() -> impl Strategy<Value = (u32, CellCoord, Vec<CellCoord>, Vec<CellCoord>)> {
            let coord = (0u32..WIDTH, 0u32..DEPTH).prop_map(|(x, z)| CellCoord::new(x, 0, z));
            (
                1u32..=2,
                coord.clone(),
                prop::collection::vec(coord.clone(), 1..=4),
                prop::collection::vec(coord, 0..=6),
            )
        }

        fn routed(
            void: u32,
            source: CellCoord,
            sinks: &[CellCoord],
            loose: &[CellCoord],
        ) -> (Router, HashSet<CellCoord>) {
            let region = CircuitRegionReservation {
                label: "floor".to_owned(),
                void,
                width: WIDTH,
                depth: DEPTH,
                span: Span::default(),
            };
            // Everything standing in the reservation: the source, the
            // sinks, and whatever else the case put there.
            let mut blocks: Vec<CellCoord> = vec![source];
            blocks.extend(sinks.iter().copied());
            blocks.extend(loose.iter().copied());
            let sites: Vec<BlockSite> = blocks
                .iter()
                .enumerate()
                .map(|(index, coord)| BlockSite {
                    coord: *coord,
                    kind: BlockKind::Cell,
                    index,
                })
                .collect();
            (
                Router::new(&region, &sites),
                blocks.iter().copied().collect(),
            )
        }

        /// The fewest steps from any seed to any target, over the
        /// same free space the router walks — a breadth-first sweep
        /// with nothing else in it.
        fn breadth_first(
            router: &Router,
            seeds: &[CellCoord],
            targets: &[CellCoord],
        ) -> Option<u32> {
            let mut seen: HashSet<CellCoord> = seeds.iter().copied().collect();
            let mut frontier: Vec<CellCoord> = seeds.to_vec();
            let mut distance = 0u32;
            while !frontier.is_empty() {
                let mut next: Vec<CellCoord> = Vec::new();
                for coord in frontier {
                    for delta in super::STEPS {
                        let Some(step) = router.step(coord, delta) else {
                            continue;
                        };
                        if targets.contains(&step) {
                            return Some(distance + 1);
                        }
                        if !router.free(step, &nothing_in_the_way()) || !seen.insert(step) {
                            continue;
                        }
                        next.push(step);
                    }
                }
                distance += 1;
                frontier = next;
            }
            None
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

            #[test]
            fn a_route_is_a_walk_along_the_wire_it_belongs_to(
                (void, source, sinks, loose) in layout()
            ) {
                let (router, _) = routed(void, source, &sinks, &loose);
                let tree = router.tree(source, &sinks, &nothing_in_the_way());
                let owned: HashSet<CellCoord> = tree.wire_path().into_iter().collect();
                for sink in &sinks {
                    let route = tree.route_to(*sink).expect("a sink of this net");
                    prop_assert_eq!(route.first().copied(), Some(source));
                    prop_assert_eq!(route.last().copied(), Some(*sink));
                    for coord in &route {
                        prop_assert!(
                            owned.contains(coord),
                            "route to {:?} visits {:?}, which the net does not own",
                            sink,
                            coord,
                        );
                    }
                    if tree.unreachable().contains(sink) {
                        continue;
                    }
                    for pair in route.windows(2) {
                        prop_assert_eq!(manhattan(pair[0], pair[1]), 1);
                    }
                }
            }

            /// Every attachment is as short as a plain breadth-first
            /// walk, whichever tier answered.
            ///
            /// `straight_run` is allowed to skip `search` only because
            /// a clear line between the closest pair is already a
            /// shortest path; `search` is hand-rolled A* with a
            /// non-standard tie-break and a stale-entry skip in place
            /// of decrease-key. Neither had anything to check it
            /// against — a tie-break edit that quietly added a block
            /// would move `wire_length`, the buffer counts, and every
            /// pinned buffer coord, and the only things that would
            /// notice are fixtures a well-meaning author would
            /// "fix" by updating the number.
            ///
            /// The walk below is the oracle: no heuristic, no ordering,
            /// no reuse of the router's own bookkeeping beyond what a
            /// coord *is*. Driven through [`Router::reach`] so the real
            /// tier choice is what gets measured, round after round,
            /// including every round the shortcut declines.
            #[test]
            fn every_attachment_is_as_short_as_a_breadth_first_walk(
                (void, source, sinks, loose) in layout()
            ) {
                let (router, _) = routed(void, source, &sinks, &loose);
                let mut seeds = vec![source];
                let mut targets: Vec<CellCoord> = Vec::new();
                for sink in &sinks {
                    if *sink != source && !targets.contains(sink) {
                        targets.push(*sink);
                    }
                }
                prop_assume!(!targets.is_empty());
                while !targets.is_empty() {
                    let reference = breadth_first(&router, &seeds, &targets);
                    let Some((index, path)) = router.reach(&seeds, &targets, &nothing_in_the_way()) else {
                        prop_assert_eq!(
                            reference,
                            None,
                            "the router gave up where a plain walk gets through",
                        );
                        break;
                    };
                    let walked = u32::try_from(path.len() - 1).expect("a path fits in u32");
                    prop_assert_eq!(Some(walked), reference, "path {:?}", path);
                    let searched = router
                        .search(&seeds, &targets, &nothing_in_the_way())
                        .expect("a path exists, so the search has to find one");
                    prop_assert_eq!(
                        searched.1.len(),
                        path.len(),
                        "search {:?} against the tier that answered {:?}",
                        searched.1,
                        path,
                    );
                    targets.remove(index);
                    seeds.extend(path[1..path.len() - 1].iter().copied());
                }
            }

            /// No two nets of one scope run within one step of
            /// each other in one plane.
            ///
            /// The whole change rests on this: dust joins the dust
            /// beside it, so two nets one step apart are one strand
            /// carrying two signals as surely as two nets on one
            /// coord, and the crossing pass no longer looks for either
            /// because [`net_trees`] no longer produces them. Nothing
            /// else checks it over arbitrary geometry, so it is
            /// checked here, over layouts the example corpus does not
            /// have.
            ///
            /// Up and down are not checked, because the router does
            /// not claim them: `spec/redstone` §14.5 leaves what a
            /// strand at `y + 1` reads to the physical tile layer.
            ///
            /// Three nets rather than two, because two only ever
            /// exercises "avoid the one before me". The third has to
            /// avoid the union of the two before it, which is the case
            /// where the accumulating obstacle set could go wrong and
            /// a per-net one would not.
            ///
            /// All three are drawn from the same block set, which is
            /// what makes the case realistic: a scope's nets end at
            /// each other's terminals, and a terminal is not dust.
            ///
            /// `prop_assume!` rather than an early return, so a case
            /// with too few blocks to build three nets from is counted
            /// as a reject rather than as a pass — the vacuity is in
            /// proptest's own rejection rate instead of hidden.
            #[test]
            fn no_two_nets_of_a_scope_run_within_one_step_in_one_plane(
                (void, source, sinks, loose) in layout()
            ) {
                prop_assume!(loose.len() >= 3);
                let (router, _) = routed(void, source, &sinks, &loose);
                let split = 2 + (loose.len() - 2) / 2;
                let mut nets: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
                nets.insert(NetRef::Input(0), sinks.clone());
                nets.insert(NetRef::Cell(0), loose[2..split].to_vec());
                nets.insert(NetRef::Cell(1), loose[split..].to_vec());
                let trees = net_trees(&nets, &router, |net| match net {
                    NetRef::Input(_) => source,
                    NetRef::Cell(0) => loose[0],
                    NetRef::Cell(_) => loose[1],
                });
                let mut seen: HashMap<CellCoord, NetRef> = HashMap::new();
                for (net, tree) in &trees {
                    for coord in router.dust(tree) {
                        for taken in std::iter::once(coord).chain(beside(coord)) {
                            if let Some(other) = seen.get(&taken)
                                && other != net
                            {
                                return Err(TestCaseError::fail(format!(
                                    "{net:?} lays dust on {coord:?}, which is \
                                     {taken:?} or beside it, and {other:?} \
                                     already has it",
                                )));
                            }
                        }
                    }
                    for coord in router.dust(tree) {
                        seen.insert(coord, *net);
                    }
                }
            }

            #[test]
            fn no_route_passes_through_a_block(
                (void, source, sinks, loose) in layout()
            ) {
                let (router, blocks) = routed(void, source, &sinks, &loose);
                let tree = router.tree(source, &sinks, &nothing_in_the_way());
                for sink in &sinks {
                    if tree.unreachable().contains(sink) {
                        continue;
                    }
                    let route = tree.route_to(*sink).expect("a sink of this net");
                    let interior = route.get(1..route.len().saturating_sub(1)).unwrap_or_default();
                    for coord in interior {
                        prop_assert!(
                            !blocks.contains(coord),
                            "the route to {:?} runs through {:?}",
                            sink,
                            coord,
                        );
                    }
                }
            }
        }
    }
}
