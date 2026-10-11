//! Shared rectilinear-geometry helpers for the routing / delay /
//! crossing passes (stages 2, 3, and 4 of the pipeline `spec/redstone`
//! "Place-and-route" lays out): the pad coordinate convention, the
//! deterministic net order, and the one answer to "where does this net's
//! dust run". A change to the axis order, the search's tie-break, or the
//! [`NetRef`] sort key reaches every pass at once, and the byte-for-byte
//! JSON comparisons in the integration tests catch any drift.
//!
//! # The reservation is not empty space
//!
//! A reservation is a box `x ∈ [0,width) × y ∈ [0,void) × z ∈ [0,depth)`
//! some of whose coords already hold blocks: cell bodies, input pads,
//! actuator pads. Dust cannot occupy a block's coord, and a signal cannot
//! pass *through* one — a block either emits (the net's source) or
//! consumes (one of its sinks).
//!
//! [`Router`] holds that block set for one scope and grows a tree out of
//! each net's source, attaching the nearest sink still unconnected by
//! the cheapest block-free path until every sink is a leaf: the
//! shortest-path heuristic for a rectilinear Steiner tree, run inside the
//! obstacle set. [`Router::straight_run`] settles a pair with nothing
//! between them and [`Router::search`] a pair with something; both walk
//! x, then z, then y, so a net with a clear run occupies exactly the
//! L-shape the passes were built around.
//!
//! # The obstacle set grows as the nets are laid
//!
//! Blocks; every coord of dust an earlier net occupies; and every coord
//! [`beside`] that dust in its own plane, because two nets one step apart
//! are one strand carrying two signals. The second net goes round, or
//! climbs over — that pipeline's escape, falling out of the same search.
//! In its own plane only: whether dust at `y + 1` reads the dust below it
//! is the physical tile layer's question, so [`tile_layer_pairs`] lists
//! the strands a layer apart and [`tile_layer_clearance`] names them as
//! an advisory rather than refusing them.
//!
//! A tree is therefore a function of the order the nets were laid in.
//! [`net_trees`] lays them in [`net_order`] — fanout descending, then
//! [`net_ref_key`] — a total order all three passes walk, so the map is
//! one answer per layout. The escape happens before `wire_length` and
//! `local_delay_ticks` are read off the tree, so a net that climbed is
//! charged for the climb.
//!
//! Only dust is widened: a sink is a block, and two nets ending at one
//! cell body is the ordinary two-input cell, so [`Router::dust`] takes
//! the terminals back out of a tree before [`beside`] widens it.
//!
//! # Layers
//!
//! Dust that leaves the ground layer is stamped [`RouteLayer::Bridge`],
//! the same layer the crossing pass stamps on a repeater it lifts. One
//! rule — [`CellCoord::new`] — decides the layer from the height, so the
//! two cannot key past each other.
//!
//! # Two projections that cannot disagree
//!
//! [`NetTree`] is a tree of coords with a parent link on each, rooted at
//! the source. [`NetTree::wire_path`] lists them; [`NetTree::route_to`]
//! walks the parent links back from one sink, so a route is a subset of
//! the wire by construction. Stage 2 drains `wire_path` into its
//! occupancy set; stages 3 and 4 place buffer repeaters on the tree and
//! read the ones each `route_to` passes through, stage 3 to count them
//! and stage 4 to record their coords. The trees are
//! recomputed per stage rather than stored on the IR: they are a pure
//! function of what the IR already carries, and a stored copy would put
//! every coord of every net into every JSON dump.
//!
//! [`RouteLayer::Bridge`]: crate::placement_ir::RouteLayer::Bridge

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fmt::Write as _;

use cairn_lang_core::check::Severity;

use crate::delay::MAX_ATTENUATION_SEGMENT;
use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::netlist_ir::{CellPortDriver, NetRef};
use crate::placement::CELL_ROW;
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIrEntry,
};
use crate::saturating_index;

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

/// How a scope's edge columns lay their pads: whether they step over
/// the cell row.
///
/// The one place that decision is made. [`edge_pad`] reads it for the
/// coordinates and [`Self::rows`] for the rows those coordinates span,
/// so the placement pass's pad-row refusal, which reads the second,
/// cannot disagree with the pads it places from the first. Every pass
/// that derives a pad builds it from the IR it is working on with
/// [`Self::of`], or, in the placement pass, before that IR has cells,
/// from the netlist's cell count with [`Self::for_cell_count`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PadColumn {
    steps_over_cell_row: bool,
}

impl PadColumn {
    /// The column of a scope with `cells` cells.
    ///
    /// A scope with cells has a cell row at [`CELL_ROW`], and a pad on
    /// it would stand face to face with the end cell of the row
    /// whenever the row reaches the edge column, on a net that cell may
    /// have nothing to do with. A scope with no cells has no row to
    /// keep off, so its pads take every row from `z = 0`.
    pub(crate) fn for_cell_count(cells: usize) -> Self {
        Self {
            steps_over_cell_row: cells > 0,
        }
    }

    /// The column of the scope `ir` describes.
    pub(crate) fn of(ir: &PlacementIr) -> Self {
        Self::for_cell_count(ir.cells.len())
    }

    /// The row the `index`th pad stands in before the region clamps it:
    /// `z = index`, plus one from the cell row on when the column steps
    /// over it.
    fn z(self, index: u32) -> u32 {
        if self.steps_over_cell_row && index >= CELL_ROW {
            index.saturating_add(1)
        } else {
            index
        }
    }

    /// Rows a column of `pads` pads needs: the deepest pad's row, plus
    /// one. That is one per pad, and the cell row as well once a column
    /// that steps over it reaches it.
    ///
    /// Read by the placement pass's pad-row refusal, so the refusal and
    /// the coordinates it guards cannot disagree.
    pub(crate) fn rows(self, pads: usize) -> u32 {
        match saturating_index(pads) {
            0 => 0,
            pads => self.z(pads - 1).saturating_add(1),
        }
    }

    /// The depth rule [`Self::rows`] computes, as the fix line of every
    /// pad-row refusal states it, so the placement pass and the passes
    /// after it hand the author one sentence.
    pub(crate) fn depth_rule(self) -> &'static str {
        if self.steps_over_cell_row {
            "`depth >= max(inputs, outputs) + 1` once an edge carries two pads — one row per \
             sensor or actuator, and one for the cell row the pads step over"
        } else {
            "`depth >= max(inputs, outputs)` — one row per sensor or actuator, since a scope \
             with no cells has no cell row for the pads to step over"
        }
    }
}

/// v1 input-pad coordinate: left edge (`x=0`), first service layer
/// (`y=0`), z-axis increasing as the input index grows, laid by
/// `column` (see [`edge_pad`]). Saturates at `depth-1` when the input
/// count would push z past the region; the resulting overlap is what
/// [`collapsed_block`] finds, and every pass that lays nets surfaces it
/// as `E_ROUTE_CONGESTION` rather than a silent misroute.
pub(crate) fn input_pad(
    i: usize,
    column: PadColumn,
    region: &CircuitRegionReservation,
) -> CellCoord {
    edge_pad(i, 0, column, region)
}

/// v1 output-pad coordinate: right edge (`x=width-1`), same
/// saturating z-axis convention as [`input_pad`].
pub(crate) fn output_pad(
    k: usize,
    column: PadColumn,
    region: &CircuitRegionReservation,
) -> CellCoord {
    edge_pad(k, region.width.saturating_sub(1), column, region)
}

/// The `index`th pad down the edge column at `x`.
///
/// The pads step along `z` from `0`, over the cell row when `column`
/// says the scope has one, and saturate at `depth - 1`. In a scope
/// with cells the saturation can put a pad back on the cell row (at
/// `depth == 2` the second pad lands on `z = 1`), but only in a region
/// the placement pass refuses; no pad that pass emits stands on the
/// cell row.
fn edge_pad(
    index: usize,
    x: u32,
    column: PadColumn,
    region: &CircuitRegionReservation,
) -> CellCoord {
    let z = column.z(saturating_index(index));
    CellCoord::new(x, 0, z.min(region.depth.saturating_sub(1)))
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
/// `local_delay_ticks`, so the two cannot disagree about how many strands
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
/// [`crate::pass::lay_nets`] reads it to refuse a pad row that cannot
/// fit and then builds the [`Router`] from it, so all three passes get
/// one list and one router from it; [`Router::dust`] reads it back out
/// of a tree to tell a net's wire from its terminals. A second list
/// built anywhere else is a second thing to keep in step.
///
/// The order is load-bearing for [`collapsed_block`], which asserts on
/// the second of two cells sharing a coord and reports the second of
/// any other pair. `block_sites_lists_cells_then_input_pads_then_output_pads`
/// holds it.
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

/// The six coords one step from `coord`, every face of the block on
/// it: [`beside`] and the two straight above and below.
///
/// What a block standing on `coord` touches, as against the dust on it
/// joins, which is [`beside`]'s question.
pub(crate) fn faces(coord: CellCoord) -> impl Iterator<Item = CellCoord> {
    STEPS
        .into_iter()
        .filter_map(move |delta| stepped(coord, delta))
}

/// The coords a strand of dust on `coord` reaches: the four steps that
/// keep it in its own plane.
///
/// Dust joins the dust beside it, so a second net drawing on one of
/// these is one strand with the first as surely as a second net drawing
/// on `coord` itself. Up and down are not here: whether dust at `y + 1`
/// reads the dust below it depends on what stands between them, and
/// `spec/redstone` "Place-and-route" leaves that to the physical tile
/// layer.
pub(crate) fn beside(coord: CellCoord) -> impl Iterator<Item = CellCoord> {
    IN_PLANE
        .into_iter()
        .filter_map(move |delta| stepped(coord, delta))
}

/// The coords one layer under `coord` that are within one step of it:
/// the one directly under it, and the four diagonally under it.
///
/// Not an obstacle — [`beside`] is what the router routes by, and this
/// is what nothing routes by. Two strands a layer apart are separated
/// by the physical tile layer rather than by stage 2, so what this
/// names is the pair that layer is obliged to keep apart, and
/// [`tile_layer_pairs`] is where the obligation gets written down.
///
/// Derived from [`IN_PLANE`] rather than listed, so the rule the
/// router routes by and the rule the tile layer is handed cannot come
/// to mean different widths.
fn under(coord: CellCoord) -> impl Iterator<Item = CellCoord> {
    std::iter::once((0, -1, 0))
        .chain(IN_PLANE.map(|(dx, _, dz)| (dx, -1, dz)))
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
    /// [`GaveUp`] when no target is reached; why each one was not is
    /// [`Self::unreached`]'s question.
    ///
    /// Only the search tier is bounded by [`MAX_ATTENUATION_SEGMENT`].
    /// The straight tier has no cap check and can return a path longer
    /// than it. Through [`crate::pass::lay_nets`] it does not: the gate
    /// there refuses a sink further from its driver than the cap in a
    /// straight line before any tree is grown, the driver is always a
    /// seed, and the straight tier answers only for the closest
    /// seed/target pair — so its path is no longer than the driver's
    /// straight line to some target, which that gate held to the cap. A
    /// caller of [`Self::tree`] that skips the gate can get an over-cap
    /// path from this tier.
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
    ///
    /// # Bounded by the attenuation cap
    ///
    /// A coord is not queued when `g + h` — the length of the cheapest
    /// path through it from the tree to a target — is over
    /// [`MAX_ATTENUATION_SEGMENT`]. The segment the delay pass measures
    /// runs from the net's source through the seed a path leaves from, so
    /// it is at least the path, and a path over the cap is one that pass
    /// refuses. Without the bound a walled-in target was given up on only
    /// once the frontier emptied, which is every free coord of the
    /// reservation the tree can reach: work and memory set by
    /// `width × depth × void` rather than by the distance to the target.
    ///
    /// No accepted route changes. `h` is the Manhattan distance to the
    /// nearest target, which is consistent, so every coord on a cheapest
    /// path of length `L` has `g + h <= L`, and the coords the bound
    /// drops have `g + h` above the cap and so above `L` — the heap
    /// would have returned before popping any of them.
    ///
    /// When no target has a face a path could arrive through — a free
    /// coord, or a seed — there is nothing to search and the answer is
    /// [`GaveUp::Exhausted`] at once. A target that has none while
    /// another target does is not removed: it stays a goal and stays in
    /// `h`. That costs nothing in soundness, because `h` is then at most
    /// the distance to the nearest target that can be arrived at, and a
    /// smaller `h` prunes less rather than more.
    ///
    /// On failure the answer is [`GaveUp`], which is about the search
    /// and not about any one target; [`Router::tree`] turns it into a
    /// verdict per sink.
    ///
    /// The second value is how many coords the search settled — popped
    /// and not stale — the work the bound exists to limit, returned so a
    /// test can hold it to that.
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
                // A cheaper way to this coord was found after this
                // entry was pushed; the heap has no decrease-key.
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

    /// Why no path of at most [`MAX_ATTENUATION_SEGMENT`] blocks from
    /// `seeds` reaches `sink`, asked once a search the cap pruned has
    /// come back empty ([`GaveUp::Pruned`]), and how many coords
    /// answering it visited.
    ///
    /// A breadth-first flood out of `sink`, through its faces and on
    /// through free coords, no further than the cap from it. The search
    /// cannot answer this itself: what it learns is that nothing within
    /// the cap reaches *any* of its targets, and a coord it prunes says
    /// nothing about which target is walled in — a real detour is pruned
    /// too, once `h` has come down near the wall.
    ///
    /// - The flood runs out of coords before the cap: the free coords
    ///   `sink` opens onto are a pocket the tree is not in, so no route
    ///   of any length reaches it. [`Unreached::WalledIn`] is proved,
    ///   and a sink with no free face at all is the pocket of size zero.
    ///   So is a sink outside the reservation, which [`Self::step`]
    ///   never enters and so no path arrives at, whatever is beside it.
    /// - The flood is still going at the cap: [`Unreached::BeyondCap`],
    ///   which claims only what the search proved. A sink walled off in
    ///   a free region wider than the cap lands here too; the flood
    ///   cannot tell it from a long way round without leaving the cap's
    ///   neighbourhood, which is the work the bound exists to save.
    ///
    /// Reaching a seed within the cap would contradict the search — the
    /// bound keeps every coord on such a path — so it is asserted
    /// against in a debug build and answered as `BeyondCap` otherwise,
    /// the verdict that claims less.
    ///
    /// Bounded by the pocket or by the cap's neighbourhood of `sink`,
    /// whichever is smaller, so a refusal still costs nothing set by the
    /// size of the reservation. Only ever asked on the way to a refusal.
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
    ///
    /// When the search reaches none of the sinks still waiting, each of
    /// them is stranded with its own [`Unreached`]: all walled in when
    /// the search ran out of coords without the cap pruning any
    /// ([`GaveUp::Exhausted`]), and otherwise whatever
    /// [`Self::unreached`] proves of that sink alone.
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

/// What a search that reached none of its targets knows about all of
/// them at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GaveUp {
    /// Nothing was left to search, and the cap pruned nothing on the
    /// way: every free coord the tree can reach was settled, or no
    /// target had a face to arrive through. No path of any length
    /// reaches any target, so every one is [`Unreached::WalledIn`].
    Exhausted,
    /// The cap pruned at least one coord. That is a fact about the
    /// search, not about any one target — a coord can be pruned from
    /// an expensive predecessor and then queued from a cheap one, and a
    /// target walled in near the tree is pruned round just as a far one
    /// is — so [`Router::unreached`] asks each target on its own.
    Pruned,
}

/// Why one sink was not reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unreached {
    /// No path of any length reaches the sink: the free coords it opens
    /// onto, if any, are a pocket closed by blocks, by other nets' dust
    /// and the coords beside it, and by the edge of the reservation, and
    /// the tree is not in it.
    WalledIn,
    /// No path of at most [`MAX_ATTENUATION_SEGMENT`] blocks reaches it.
    /// One longer may or may not exist; nothing looks, because the delay
    /// pass would refuse it.
    BeyondCap,
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
    /// Why each sink of [`Self::unreachable`] is there, asked of each
    /// one on its own by [`Router::unreached`].
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

    /// Record `coord` as reached from `from`.
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

    /// Record a sink the reservation has no clear path to.
    ///
    /// Attached straight to the source so both projections stay total:
    /// the routing pass refuses such a scope before any consumer reads
    /// the number, and a hand-built IR handed to stage 3 or stage 4
    /// without stage 2 gets a deterministic answer rather than a
    /// panic. The number itself means nothing — [`Self::unreachable`]
    /// is what a caller asks.
    fn strand(&mut self, sink: CellCoord, why: Unreached) {
        let source = self.order[0];
        self.parent.insert(sink, source);
        self.order.push(sink);
        self.unreachable.push(sink);
        self.stranded_by.insert(sink, why);
    }

    /// Every coord the net occupies, source first and then in the
    /// order the search laid them. Always non-empty (the source occupies
    /// its own coord), and no coord appears twice.
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

    /// The coord the signal reaches `coord` from: `None` for the source,
    /// and for a coord that is not on this net.
    pub(crate) fn parent(&self, coord: CellCoord) -> Option<CellCoord> {
        self.parent.get(&keyed(coord)).copied()
    }

    /// A tree grown along `paths` by hand, each path starting on a coord
    /// the tree already holds — the first one at the source — and
    /// moving one block per step. Both are debug-asserted, so a bad
    /// fixture fails here rather than in whatever walks the tree.
    ///
    /// For the shapes a test wants pinned coord by coord without
    /// walling the router into them.
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

    /// The sinks the reservation has no clear path to, in the order
    /// they were asked for.
    ///
    /// Empty for every scope the routing pass lets through: it is
    /// stage 2 that refuses a layout the router cannot wire.
    pub(crate) fn unreachable(&self) -> &[CellCoord] {
        &self.unreachable
    }

    /// Why `sink` is one of [`Self::unreachable`], or `None` when it is
    /// not one.
    pub(crate) fn stranded_by(&self, sink: CellCoord) -> Option<Unreached> {
        self.stranded_by.get(&sink).copied()
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

/// The first pad standing on a coord another block already holds, or
/// `None` when every block has a coord of its own. Panics when two
/// cells share one.
///
/// What this finds is a pad the reservation is too small to hold,
/// stacked onto a cell or onto another pad. Too shallow: pad coords
/// step from `z = 0`, over the cell row in a scope that has one, and
/// saturate at `depth - 1`, so a pad column taller than the
/// reservation piles up on the last row. Too narrow: at `width == 1`
/// the sensor and actuator columns are one column, and pad #0 of each
/// stands at `(0,0,0)` whatever the depth. Either is a reservation an
/// author can enlarge, so it earns a diagnostic.
///
/// Two cells on one coord is not that. It is an IR no `size=` can
/// repair: a cell's coord comes from its topological index in the
/// placement pass, so a repeat means the caller assembled the list
/// itself and got it wrong. Every caller of this function has skipped
/// that pass by construction — that is the whole reason it is asked —
/// so "the placement pass would not do that" is not a guarantee any
/// of them hold, and the two blocks are as silent as the pad pair:
/// one net whose source and sink are the same coord, a wire length
/// under every cap, a dump that reads as a layout. It asserts, the way
/// [`collect_nets`]' callers do for a broken topological invariant.
///
/// So the block returned is always a pad. A repeated cell never comes
/// back from here, and [`block_sites`] lists cells before pads, so a
/// list carrying both faults asserts rather than reporting the pad.
///
/// Asked by all three passes, for the reason [`unroutable`] is: stage
/// 2 elides such a scope, so in a real run stages 3 and 4 never see
/// one, and a caller that assembled the IR itself hands them one
/// directly.
///
/// The coords come through [`keyed`], the way [`Router::new`] keys the
/// same list. Nothing [`block_sites`] builds needs it — every coord
/// there is already canonical — but the fields of [`CellCoord`] are
/// public, so a hand-built `(x, 1, z, Plane)` beside a
/// `(x, 1, z, Bridge)` is two coords to a raw comparison and one
/// obstacle to the router. Comparing them the router's way is what
/// keeps this function's answer and the router's world the same.
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

/// The refusal a scope earns when the reservation cannot wire one of
/// its sinks, or `None` when every sink has a clear path.
///
/// `E_ROUTE_CONGESTION` with its own primary, so it does not read as
/// the area arithmetic: the area can be ample and the one coord the
/// wire needs still be taken. For a sink [`Unreached::BeyondCap`]
/// stranded it is `E_ATTENUATION_LIMIT` from [`beyond_cap_diagnostic`]
/// instead. Either way it names the nets whose dust takes the sink's
/// faces — the cause the author can act on — and counts the rest of
/// the scope's stranded sinks, so sizing is one decision.
///
/// One sink anchors the message, and the code is that sink's. A
/// [`Unreached::WalledIn`] sink is preferred when there is one: its
/// wall is a cause the author can move, while "nothing within the cap"
/// may only be a symptom of it. Among those of one reason the first in
/// [`net_order`] and then in the net's own order is taken, so the
/// anchor is the same every run. The count of the rest covers sinks of
/// either reason, and says only that they cannot be reached.
///
/// `netlist` is the noun the caller lays nets from — `placed`,
/// `routed`, `delayed`. It is a parameter and not a constant because
/// all three passes route through [`crate::pass::lay_nets`], so a
/// hardcoded noun would name the wrong stage on two of the three paths.
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

/// The crowding clause and the count of the other stranded sinks, which
/// both of [`unroutable`]'s refusals end with.
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

/// The refusal for a sink [`Unreached::BeyondCap`] stranded: the free
/// coords round it run on past the cap, and no path within the cap
/// reaches it.
///
/// `E_ATTENUATION_LIMIT`, the code the delay pass gave such a sink when
/// the search still laid a detour out for it to measure. The sentence
/// says what is known and stops: no route within the cap. It does not
/// say a longer route exists, because nothing looked. The crowding
/// clause stays, since a sink can be out of reach for both reasons at
/// once and the nets on its faces are still something the author can
/// move.
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

/// Human-facing name for a net: the sensor's dotted `sig.<name>` when
/// the scope carries one at that index, `cell #j` for a synthesised
/// gate, which the Netlist IR has no source-level name for.
///
/// The `input pad #i` fall-back is for a hand-built IR whose input row
/// is shorter than the synthesis path implies; a diagnostic naming
/// nothing is worse than one naming an index.
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
        // goes in: consecutive coords of a strand are one step apart by
        // definition, so a strand runs beside itself the whole way, and
        // widening as it went would have the claim above trip on this
        // net's own turn.
        keep_out.extend(dust.into_iter().flat_map(beside));
        trees.insert(net, tree);
    }
    trees
}

/// Two strands of dust a layer apart and within one step of each
/// other: the pair `spec/redstone` "Place-and-route" hands to the
/// physical tile layer.
///
/// `over` is the upper coord and `under` the lower. A pair of coords
/// is one entry rather than two — the walk that finds them looks down
/// only — and reads in the direction the block between them would be
/// placed. Two strands running alongside each other for several coords
/// are several pairs, because each of those coords is its own place
/// for a tile to conduct or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TileLayerPair {
    /// The upper coord, and the net whose dust it is.
    pub(crate) over: (CellCoord, NetRef),
    /// The lower coord, and its net — never the same net as `over`:
    /// one strand climbing is the wire, not two signals.
    pub(crate) under: (CellCoord, NetRef),
}

impl TileLayerPair {
    /// `true` when the two sit on one column — dust directly over
    /// dust, rather than over and one step across.
    fn stacked(self) -> bool {
        self.over.0.x == self.under.0.x && self.over.0.z == self.under.0.z
    }
}

/// Every pair of strands the physical tile layer has to separate, in
/// [`coord_key`] order over the upper coord and then the lower.
///
/// The router lays no two nets within one step of each other in one
/// plane, so the pairs here are all made by the escape: a net that
/// climbs to clear another runs over it, or one step across from it a
/// layer up — the staircase dust climbs, which shorts by the same
/// mechanism an in-plane pair does.
///
/// Dust only, per [`Router::dust`]: a terminal is a block, and a block
/// under a strand is what a wire climbs over rather than something two
/// signals share — the ordinary way past a component. The obligation
/// `spec/redstone` "Edition differences" puts on the tile is wider than
/// this list, and does not need the list to enumerate it: a `bridge`
/// tile conducts to no coord of another net below it, whether dust or
/// block.
pub(crate) fn tile_layer_pairs(
    nets: &HashMap<NetRef, Vec<CellCoord>>,
    trees: &HashMap<NetRef, NetTree>,
    router: &Router,
) -> Vec<TileLayerPair> {
    // One owner per coord: `net_trees` asserts that no two nets claim
    // one, so the map loses nothing, and looking a coord up beats
    // walking every other net's dust for each strand.
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
    // `HashMap` iteration order never reaches a message: the walk above
    // is over a map, so the order the pairs were found in is not an
    // order at all.
    pairs.sort_by_key(|pair| (coord_key(pair.over.0), coord_key(pair.under.0)));
    pairs
}

/// How many pairs *of each shape* the finding names one by one.
///
/// A dump reader wants somewhere to start reading and the shape of the
/// obligation, not a census — the count in the primary is the census.
/// Per shape rather than over the whole list because the two are
/// separated by different tiles — the one under a strand and the ones
/// diagonally under it — and a layout is lopsided towards the diagonal
/// often enough for a flat cap to name one shape only. A count of the
/// corpus's split belongs in a test, where it fails when it drifts;
/// here it would be a number nothing checks.
const NAMED_PER_SHAPE: usize = 2;

/// The advisory a scope earns for the strands its escape left a layer
/// apart, or `None` when no two nets run within one step of each other
/// across layers.
///
/// `W_ROUTE_CROSS_LAYER_CLEARANCE`, and a warning rather than a
/// refusal: the pairs are not a fault in the layout. `spec/redstone`
/// "Place-and-route" keeps two nets one step apart in one plane and
/// makes separating two a layer apart the physical tile layer's
/// obligation, because whether the upper strand reads the lower one
/// depends on what is standing between them and the pseudo-2.5D model
/// carries no answer. A rule invented here would refuse layouts for a
/// reason nothing can check.
///
/// What is left is that the obligation was owed by nobody a reader
/// could see. This names the pairs that carry it, so an author reading
/// a dump — and whoever writes the tile catalogue — can find them.
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
