//! Where a scope's implicit buffer repeaters stand.
//!
//! The delay pass charges ticks for these repeaters and the crossing pass
//! records their coords. Each builds its own [`RepeaterSites`] with
//! [`repeater_sites_of_scope`] from the trees it rebuilt; the function is
//! deterministic and nothing between the two stages moves a cell, so the
//! two passes describe one circuit.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::grid::{faces, net_ref_key};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{NetTree, net_label};
use crate::saturating_index;

/// Signal-attenuation ceiling (`spec/redstone` "Place-and-route" —
/// "the attenuation limit of 15 blocks"): the most blocks of dust a
/// signal crosses since the last block that restored its strength. A
/// fresh source starts at strength 15 and dust decays one unit per
/// block, so this many blocks of dust carry its signal, the last at
/// strength 1, which still drives the block after it: a sink, or a
/// repeater that restores the signal. A segment from a fresh source to
/// its sink is one step longer than its dust, so one of up to
/// `DUST_ATTENUATION_LIMIT + 1` steps needs no buffer repeater. From a
/// cell that passes on the strength it reads, the dust into the cell
/// counts too, and a shorter segment can need one.
pub const DUST_ATTENUATION_LIMIT: u32 = 15;

/// The node a segment feeds, as a panic about that segment names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fed {
    /// `ir.cells[i]`.
    Cell(usize),
    /// `ir.outputs[i]`'s pad.
    Output(usize),
}

impl Fed {
    /// Panic for a segment whose sink is not a terminal of the net that
    /// drives it: the driver list and the collected nets would then
    /// disagree, and the alternative is a node whose buffers
    /// under-count the dust it is fed through.
    pub(crate) fn is_not_a_terminal(self, net: NetRef, sink: CellCoord) -> ! {
        let (x, y, z) = (sink.x, sink.y, sink.z);
        match self {
            Self::Cell(index) => panic!(
                "cell #{index} at ({x},{y},{z}) is not a terminal of {net:?}, the net driving it — the driver list and the collected nets disagree"
            ),
            Self::Output(index) => panic!(
                "output #{index} pad at ({x},{y},{z}) is not a terminal of {net:?}, the net driving it — the driver list and the collected nets disagree"
            ),
        }
    }
}

/// Where every implicit buffer repeater of one scope stands, net by
/// net, beside the tree each net's sites were walked on.
///
/// Built once per scope by [`repeater_sites_of_scope`]; stage 3 counts
/// from the one it builds and stage 4 places from its own. Holding the
/// tree with its sites is what keeps [`Self::along`] from being asked
/// about one tree with the sites of another.
pub(crate) struct RepeaterSites<'t> {
    per_net: HashMap<NetRef, (&'t NetTree, HashSet<CellCoord>)>,
}

impl RepeaterSites<'_> {
    /// The repeaters the signal of `net` passes through on its way to
    /// `sink`, in the order it meets them: its route from the source.
    ///
    /// Two sinks of one net that share a stretch of the tree share the
    /// repeaters standing on it, because they are read off one set
    /// rather than placed per sink.
    ///
    /// `None` when `sink` is not a terminal of `net`, which the caller
    /// turns into a panic naming the node it asked for
    /// ([`Fed::is_not_a_terminal`]).
    pub(crate) fn along(&self, net: NetRef, sink: CellCoord) -> Option<Vec<CellCoord>> {
        let (tree, sites) = self.per_net.get(&net)?;
        let route = tree.route_to(sink)?;
        Some(
            route
                .into_iter()
                .filter(|coord| sites.contains(coord))
                .collect(),
        )
    }
}

/// [`repeater_sites`] for every net of one scope, or the
/// `E_ATTENUATION_LIMIT` that refuses the scope when a net has a run
/// of dust past its allowance with no coord close enough for a
/// repeater.
///
/// The attenuation limit runs from the last block that restored
/// strength, not from the start of each net. A cell whose
/// [`EditionCell::regenerates`] is `false` — a Bedrock OR, which is a
/// dust merge, or a Java comparator AND, which never outputs more than
/// its rear input carries — passes on the strength that reached it, so
/// the dust before it and the dust after it count as one run. Two walks
/// carry that across the cells:
///
/// - **Backwards**, cells last to first, each such cell gets a
///   *budget*: the most dust its inputs may already have spent for
///   the net it drives still to reach every one of its sinks, with a
///   repeater where one fits. Its sinks come after it in the list, so
///   their budgets are known when its own is worked out. The search
///   takes the largest `spent` in `1..=15` that [`repeater_sites`]
///   accepts. Acceptance is downward-closed in `spent`: a smaller
///   `spent` only lowers every coord's running total, so the first
///   coord past its allowance comes no earlier and has at least the
///   same coords behind it to walk back over. So the largest accepted
///   value is the budget, and every smaller one would be accepted too.
///   Zero is not tried: every cell is at least one block of dust from
///   its driver, so a budget of zero could never be met.
/// - **Forwards**, nets in [`net_ref_key`] order — the sensors, then
///   the cells in topological order — each net starts with the dust
///   already spent at its source: none at a sensor pad or a cell that
///   restores strength, and at a cell that does not, the most that any
///   one of its inputs arrives with. For a dust merge that is the
///   worst case, since any one input may be the only one on. For a
///   comparator it is only what the pass can say: it does not track
///   which input is the rear, so it takes the one that spent the most.
///   Its
///   repeaters go where the running total would pass the limit, or a
///   sink's budget. That point can be upstream of a cell, on the
///   segment into it.
///
/// The backward walk runs first and returns on the first cell it
/// meets whose own wire cannot be fed at any budget — the one with the
/// highest index — naming that cell's net. Only when every cell has a
/// budget does the forward walk run, and it names the first net, in
/// its order, that has nowhere left for a repeater. Either way the net
/// a refusal names does not depend on how any map iterates.
///
/// `trees` must be the ones [`lay_nets`] returned, which it only does
/// once every tree's [`NetTree::unreachable`] is empty: an unreachable
/// sink is parented straight to the source at whatever distance, and
/// [`repeater_sites`] counts every step as one block.
///
/// [`EditionCell::regenerates`]: crate::edition_netlist_ir::EditionCell::regenerates
/// [`lay_nets`]: crate::pass::lay_nets
pub(crate) fn repeater_sites_of_scope<'t>(
    ir: &PlacementIr,
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    region: &CircuitRegionReservation,
    trees: &'t HashMap<NetRef, NetTree>,
) -> Result<RepeaterSites<'t>, Diagnostic> {
    let refuse = |net: NetRef, tree: &NetTree, stretch: NoRepeaterSite| {
        no_repeater_site_diagnostic(ir, entry, netlist, region, net, tree, stretch)
    };
    let cell_net = |index: usize| {
        NetRef::Cell(
            u32::try_from(index).expect("a cell index the IR holds fits the u32 a NetRef carries"),
        )
    };

    let mut budgets: HashMap<CellCoord, u32> = HashMap::new();
    for (index, cell) in ir.cells.iter().enumerate().rev() {
        if cell.cell.regenerates() {
            continue;
        }
        // A cell that drives nothing has no wire out to spare any of the
        // limit for, so its budget is the whole limit. A budget counts
        // the cell's own coord as a block of dust, so that is what lets
        // a dust merge, which is one, stay lit; with no entry the cell
        // would get a sink's allowance, one block more. A comparator,
        // which reads the dust before it, could take that one more;
        // every comparator's budget is conservative the same way. No
        // `.crn` reaches this: a `logic` binding nothing consumes draws
        // `W_LOGIC_UNUSED_SIGNAL` and its cell is dropped.
        let Some(tree) = trees.get(&cell_net(index)) else {
            budgets.insert(cell.coord, DUST_ATTENUATION_LIMIT);
            continue;
        };
        // One block spent is tried last, so when no budget fits, the
        // refusal is the walk that spends the least, on the cell's own
        // net: the wire out of it is what has no room.
        let mut refusal = None;
        let budget = (1..=DUST_ATTENUATION_LIMIT).rev().find(|spent| {
            match repeater_sites(tree, *spent, &budgets) {
                Ok(_) => true,
                Err(stretch) => {
                    refusal = Some(stretch);
                    false
                }
            }
        });
        let Some(budget) = budget else {
            let stretch = refusal.expect("the range is not empty, so a walk was refused");
            return Err(refuse(cell_net(index), tree, stretch));
        };
        budgets.insert(cell.coord, budget);
    }

    let mut nets: Vec<(NetRef, &'t NetTree)> =
        trees.iter().map(|(net, tree)| (*net, tree)).collect();
    nets.sort_by_key(|(net, _)| net_ref_key(*net));
    let mut per_net: HashMap<NetRef, (&'t NetTree, HashSet<CellCoord>)> =
        HashMap::with_capacity(nets.len());
    let mut spent_at_source: HashMap<NetRef, u32> = HashMap::with_capacity(nets.len());
    for (net, tree) in nets {
        let spent = match net {
            NetRef::Input(_) => 0,
            NetRef::Cell(j) => {
                let cell = &ir.cells[j as usize];
                if cell.cell.regenerates() {
                    0
                } else {
                    // Every net driving this cell comes before it in
                    // the order, so its placement is already known.
                    const PLACED: &str =
                        "every net driving a cell comes before it in `net_ref_key` order";
                    cell.drivers
                        .iter()
                        .map(|driver| {
                            let (tree, sites) = per_net.get(&driver.net).expect(PLACED);
                            spent_on_arrival(
                                tree,
                                sites,
                                *spent_at_source.get(&driver.net).expect(PLACED),
                                cell.coord,
                            )
                        })
                        .max()
                        .unwrap_or(0)
                }
            }
        };
        let sites =
            repeater_sites(tree, spent, &budgets).map_err(|stretch| refuse(net, tree, stretch))?;
        per_net.insert(net, (tree, sites.into_iter().collect()));
        spent_at_source.insert(net, spent);
    }
    Ok(RepeaterSites { per_net })
}

/// The dust the signal of a net has spent by the time it reaches
/// `sink`: the steps since the last repeater on its route, or, with no
/// repeater on it, the whole route on top of what was `spent` at the
/// source.
fn spent_on_arrival(
    tree: &NetTree,
    sites: &HashSet<CellCoord>,
    spent: u32,
    sink: CellCoord,
) -> u32 {
    let route = tree
        .route_to(sink)
        .expect("every cell is a terminal of each net driving it");
    let steps = saturating_index(route.len().saturating_sub(1));
    match route.iter().rposition(|coord| sites.contains(coord)) {
        Some(at) => steps.saturating_sub(saturating_index(at)),
        None => spent.saturating_add(steps),
    }
}

/// A run of one net's dust the signal cannot cross, with no coord close
/// enough for a repeater.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NoRepeaterSite {
    /// The block the signal last left with its strength restored: a
    /// repeater placed before this run, or the net's source. At the
    /// source of a net whose driver passes strength on, that is less
    /// than full strength, by [`Self::spent`].
    from: CellCoord,
    /// Blocks of dust already spent on leaving `from`: zero at a
    /// repeater or at a source that restores strength.
    spent: u32,
    /// The coord nearest the source that is past its allowance — a block
    /// of dust past the limit, or a sink reached over more dust than it
    /// allows — and the first in [`NetTree::wire_path`] among those as
    /// near.
    unpowered: CellCoord,
    /// What `unpowered` may be reached over.
    allowance: Allowance,
    /// How many steps before `unpowered` a repeater may stand and still
    /// bring `unpowered` within its allowance, which is as far back as
    /// the walk for a repeater site may go: the allowance the walk held
    /// `unpowered` to. That is [`DUST_ATTENUATION_LIMIT`] for a block of
    /// dust, one more for a sink with no budget, and the budget for a
    /// sink with one. A refusal that ran out of coords close enough
    /// names this number.
    reach: u32,
    /// Whether the walk for a repeater site started on `unpowered`
    /// itself: a block of dust past the limit, which a repeater could
    /// stand in place of, reading the dust before it. The walk starts
    /// on the coord before `unpowered` instead when `unpowered` is a
    /// sink, which the repeater has to stand before, or when even the
    /// block before it is reached over more than the limit.
    starts_on_unpowered: bool,
    /// How far back the walk for a repeater site went before it gave
    /// up.
    blocked: Blocked,
    /// What kept every coord it passed over from holding one.
    refused: Refused,
}

/// The most dust a coord may be reached over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Allowance {
    /// [`DUST_ATTENUATION_LIMIT`]: any coord but the one below. A block
    /// of dust may be at most the limit's last; a sink, or a repeater,
    /// may be reached over all of it.
    Limit,
    /// A cell that passes on the strength it reads: the wire out of it
    /// needs the rest of the limit, so it may be reached over only
    /// this much.
    Budget(u32),
}

/// How far back a [`NoRepeaterSite`]'s walk went. With where it
/// started, [`NoRepeaterSite::starts_on_unpowered`], that is which
/// coords [`NoRepeaterSite::refused`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blocked {
    /// It walked every coord after `from` and before `unpowered` — and
    /// `unpowered` itself, when it started there — and reached `from`
    /// without finding one.
    NoneBetween,
    /// It walked every coord within the allowance before `unpowered` —
    /// and `unpowered` itself, when it started there — without finding
    /// one; a repeater on any coord further back would leave the signal
    /// too weak to reach it.
    NoneCloseEnough,
}

/// What kept the coords a [`NoRepeaterSite`]'s walk passed over from
/// holding a repeater, each of them for one of the [`Misfit`]s. Both
/// empty when the walk passed over no coord at all: the first one it
/// asked about was already `from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Refused {
    /// Some coord turns, climbs or branches.
    bends: bool,
    /// Some coord runs straight through at one height, but touches
    /// another block of its own net on one of its faces: the one of
    /// those nearest `unpowered`.
    touching: Option<Touching>,
}

/// A coord refused for touching its own net, and the block of its net
/// it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Touching {
    /// The coord a repeater was refused on.
    at: CellCoord,
    /// The first of its faces, in [`faces`] order, that holds another
    /// coord of the same net — one other than the coord before it and
    /// the coord after it on the path.
    touching: CellCoord,
}

impl Refused {
    /// Records that the walk has also refused a coord for `misfit`.
    /// The first coord refused for touching is the one kept, which is
    /// the nearest `unpowered`, since the walk runs back from it.
    fn add(&mut self, misfit: Misfit) {
        match misfit {
            Misfit::Bends => self.bends = true,
            Misfit::Touches(touching) => {
                self.touching.get_or_insert(touching);
            }
        }
    }
}

/// Where one net's implicit buffer repeaters stand, in the order the
/// tree was laid ([`NetTree::wire_path`]) — which is not any one sink's
/// route order; [`RepeaterSites::along`] gives that.
///
/// A repeater is directional: it reads the block behind it and drives
/// the block in front of it, at its own height. So it can only stand on
/// a coord the wire runs straight through, from the coord before it to
/// the one after, all three at the same height and on one line, and from
/// which nothing else branches — a repeater on a fork drives one branch
/// and starves the other, and one on a turn or a climb drives into
/// nothing.
///
/// Placed on the net's tree rather than per sink, so every sink past a
/// repeater is fed by that one block. The signal leaves the source with
/// `spent` blocks of dust already behind it. Since the last
/// full-strength block, a coord the wire runs on past is dust, and may
/// be at most the [`DUST_ATTENUATION_LIMIT`]th block of it; a sink, or a
/// repeater, only reads the dust before it, so it may be reached over
/// all [`DUST_ATTENUATION_LIMIT`] — or over its entry in `budgets`, for
/// a sink that passes its strength on. The walk finds the coord nearest
/// the source that is past its allowance, and puts a repeater on the
/// last coord, on the way back towards the source or the repeater
/// before, that can hold one and is close enough — on that coord
/// itself, when it is dust and the block before it, which a repeater
/// there reads, is within the limit; then walks again. On a straight
/// run from a fresh source that is every `DUST_ATTENUATION_LIMIT + 1`th
/// coord, the count [`buffer_count_for_segment`] gives. Where a coord
/// cannot hold one the repeater moves earlier, which only shortens the
/// dust behind it, and the run past it can need one repeater more than
/// its length alone implies. On a fork it moves onto the trunk, where
/// one block serves every branch past it.
///
/// `Err` when a coord past its allowance has no coord behind it, since
/// the last full-strength block, that can hold a repeater close enough.
///
/// Every step of `tree` must move one block, which holds for a tree
/// whose [`NetTree::unreachable`] is empty — the only kind
/// [`crate::pass::lay_nets`] hands on.
///
/// [`buffer_count_for_segment`]: crate::delay::buffer_count_for_segment
fn repeater_sites(
    tree: &NetTree,
    spent: u32,
    budgets: &HashMap<CellCoord, u32>,
) -> Result<Vec<CellCoord>, NoRepeaterSite> {
    let order = tree.wire_path();
    let source = order[0];
    let mut children: HashMap<CellCoord, Vec<CellCoord>> = HashMap::new();
    for coord in &order[1..] {
        let parent = tree
            .parent(*coord)
            .expect("every coord but the source was attached to one");
        children.entry(parent).or_default().push(*coord);
    }
    // A coord the wire runs on past is dust, and its own block counts
    // against the limit. A leaf is a sink, and a site a repeater: both
    // only read the dust before them, so they may be reached one block
    // further — over the limit's worth of dust, not the limit less one.
    let allowance = |coord: CellCoord, sites: &HashSet<CellCoord>| {
        budgets.get(&coord).copied().unwrap_or(
            if children.contains_key(&coord) && !sites.contains(&coord) {
                DUST_ATTENUATION_LIMIT
            } else {
                DUST_ATTENUATION_LIMIT + 1
            },
        )
    };
    let mut sites: HashSet<CellCoord> = HashSet::new();
    loop {
        // Dust spent since the last full-strength block, and depth from
        // the source. `order` lists a parent before its children, so
        // one pass fills both.
        //
        // Of the coords past their allowance (a filter rather than a
        // key), the one fixed first is the one of least depth, with its
        // `order` index breaking a depth tie so the answer does not
        // depend on anything but the tree. The walk back from it either
        // refuses or stops on a coord within that allowance of it, so a
        // repeater there brings it within reach — every pass round the
        // loop adds one site or returns `Err`. `first` holds its depth,
        // its `order` index, the coord and the dust it is reached over.
        let mut walked: HashMap<CellCoord, (u32, u32)> = HashMap::with_capacity(order.len());
        walked.insert(source, (spent, 0));
        let mut first: Option<(u32, usize, CellCoord, u32)> = None;
        for (index, coord) in order.iter().enumerate().skip(1) {
            let parent = tree
                .parent(*coord)
                .expect("every coord but the source was attached to one");
            let (behind, depth) = *walked
                .get(&parent)
                .expect("`order` lists a parent before its children");
            let behind = if sites.contains(&parent) { 0 } else { behind };
            let here = (behind.saturating_add(1), depth.saturating_add(1));
            walked.insert(*coord, here);
            if here.0 > allowance(*coord, &sites)
                && first.is_none_or(|(depth, at, _, _)| (here.1, index) < (depth, at))
            {
                first = Some((here.1, index, *coord, here.0));
            }
        }
        let Some((depth, _, unpowered, reached)) = first else {
            break;
        };
        let reach = allowance(unpowered, &sites);
        // Dust past the limit can be replaced by the repeater itself,
        // which reads the dust before it, as long as that dust is
        // within the limit. A sink cannot: the repeater has to stand
        // before it.
        let starts_on_unpowered =
            children.contains_key(&unpowered) && reached <= DUST_ATTENUATION_LIMIT + 1;
        let mut candidate = if starts_on_unpowered {
            unpowered
        } else {
            tree.parent(unpowered)
                .expect("a coord past its allowance is never the source")
        };
        let mut refused = Refused::default();
        loop {
            // `candidate` is `unpowered` or an ancestor of it, so it is
            // no deeper than it.
            let candidate_depth = walked
                .get(&candidate)
                .expect("every coord of the tree was walked")
                .1;
            debug_assert!(candidate_depth <= depth, "the walk back only climbs");
            let too_far = depth.saturating_sub(candidate_depth) > reach;
            let fresh = candidate == source || sites.contains(&candidate);
            if fresh || too_far {
                let from = last_full_strength(tree, &sites, candidate);
                return Err(NoRepeaterSite {
                    from,
                    spent: if from == source && !sites.contains(&from) {
                        spent
                    } else {
                        0
                    },
                    unpowered,
                    allowance: budgets
                        .get(&unpowered)
                        .map_or(Allowance::Limit, |budget| Allowance::Budget(*budget)),
                    reach,
                    starts_on_unpowered,
                    blocked: if fresh {
                        Blocked::NoneBetween
                    } else {
                        Blocked::NoneCloseEnough
                    },
                    refused,
                });
            }
            match holds_repeater(tree, &children, candidate) {
                Ok(()) => {
                    sites.insert(candidate);
                    break;
                }
                Err(misfit) => refused.add(misfit),
            }
            candidate = tree
                .parent(candidate)
                .expect("the walk back stops at the source");
        }
    }
    Ok(order
        .into_iter()
        .filter(|coord| sites.contains(coord))
        .collect())
}

/// `coord` itself when it is the source or a repeater, otherwise the
/// nearest of those behind it: the block the signal on `coord` last
/// left at full strength.
fn last_full_strength(tree: &NetTree, sites: &HashSet<CellCoord>, coord: CellCoord) -> CellCoord {
    let mut at = coord;
    while !sites.contains(&at) {
        match tree.parent(at) {
            Some(parent) => at = parent,
            None => break,
        }
    }
    at
}

/// Why a repeater on a coord would not carry the signal on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Misfit {
    /// The coord does not have one coord before it and exactly one
    /// after, all three at the same height and on one line: it turns,
    /// climbs or branches. (The source, which has no coord before it,
    /// lands here too, but [`repeater_sites`] stops at the source
    /// before asking, so that arm is defensive.)
    Bends,
    /// The wire runs straight through it at one height, but another
    /// block of the same net is on one of its faces.
    Touches(Touching),
}

/// `Ok` when a repeater on `coord` would carry the signal on: one coord
/// before it and exactly one after, all three at the same height and
/// on one line, and no other coord of the net touching it. `Err` says
/// which of those failed; the shape of the wire is asked about first,
/// so a coord that fails both is a [`Misfit::Bends`].
///
/// The last is asked of the grid, not of the parent links. Dust joins
/// the dust beside it whether or not the tree runs between them, and a
/// repeater joins only the blocks at its back and front — so any other
/// block of the same net beside it, dust the tree reaches some other
/// way or one of the net's cells or pads, would be severed from the
/// dust the repeater replaces. The check takes all six faces, above
/// and below too, as `spec/redstone` "Place-and-route" does.
fn holds_repeater(
    tree: &NetTree,
    children: &HashMap<CellCoord, Vec<CellCoord>>,
    coord: CellCoord,
) -> Result<(), Misfit> {
    let Some(before) = tree.parent(coord) else {
        return Err(Misfit::Bends);
    };
    let [after] = children.get(&coord).map_or(&[][..], Vec::as_slice) else {
        return Err(Misfit::Bends);
    };
    let step = |from: CellCoord, to: CellCoord| {
        (
            i64::from(to.x) - i64::from(from.x),
            i64::from(to.y) - i64::from(from.y),
            i64::from(to.z) - i64::from(from.z),
        )
    };
    let (dx, dy, dz) = step(before, coord);
    if dy != 0 || (dx, dy, dz) != step(coord, *after) {
        return Err(Misfit::Bends);
    }
    // `route_to` answers for the source too, which has no parent.
    let on_net = |face: CellCoord| tree.parent(face).is_some() || tree.route_to(face).is_some();
    match faces(coord).find(|face| *face != before && *face != *after && on_net(*face)) {
        Some(touching) => Err(Misfit::Touches(Touching {
            at: coord,
            touching,
        })),
        None => Ok(()),
    }
}

/// The refusal for a [`NoRepeaterSite`]: a run of `net`'s dust past
/// its allowance with no coord close enough for a repeater.
///
/// It names the node that is left without the signal: past the limit,
/// the first cell, then the first actuator pad, whose route runs
/// through `unpowered`; within a budget, the cell standing on
/// `unpowered`, which the signal reaches too weak to pass on.
fn no_repeater_site_diagnostic(
    ir: &PlacementIr,
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    reservation: &CircuitRegionReservation,
    net: NetRef,
    tree: &NetTree,
    stretch: NoRepeaterSite,
) -> Diagnostic {
    let NoRepeaterSite {
        from,
        spent,
        unpowered,
        allowance,
        reach: near,
        ..
    } = stretch;
    let leaving = if spent == 0 {
        at(from)
    } else {
        format!(
            "{}, with {} of dust already spent,",
            at(from),
            blocks(spent)
        )
    };
    let reach = match allowance {
        Allowance::Limit => {
            let passes = |sink: CellCoord| {
                tree.route_to(sink)
                    .is_some_and(|route| route.contains(&unpowered))
            };
            let dark = ir
                .cells
                .iter()
                .position(|cell| cell.drivers.iter().any(|d| d.net == net) && passes(cell.coord))
                .map(|index| format!("cell #{index}"))
                .or_else(|| {
                    ir.outputs
                        .iter()
                        .position(|output| output.driver == net && passes(output.pad))
                        .map(|index| format!("output pad #{index}"))
                })
                .expect("every coord of a net's tree is on the route to one of its sinks");
            format!(
                "runs out before {}, past the attenuation limit of {} of dust, and never reaches {dark}",
                at(unpowered),
                blocks(DUST_ATTENUATION_LIMIT),
            )
        }
        Allowance::Budget(budget) => format!(
            "reaches {cell} at {coord} over more than the {spare} of dust it can spare — {cell} passes on the strength it receives rather than restoring it, and the wire past it needs the rest",
            cell = budget_cell(ir, unpowered),
            coord = at(unpowered),
            spare = blocks(budget),
        ),
    };
    let why = why_no_site(&stretch);
    let primary = format!(
        "{netlist} netlist for {kind} `{name}` routes {net} so that the signal leaving {leaving} {reach}: {why}",
        kind = entry.kind.label(),
        name = entry.name,
        net = net_label(net, ir),
    );
    let footer = match allowance {
        // A repeater stands in place of the block of dust past the
        // limit at the latest, reading the 15 before it, so the wire
        // needs a straight coord among every 16 blocks it runs on.
        Allowance::Limit => format!(
            "Fix: leave the wire room to run straight at least once in every {} — a larger `region=` is one way, moving what walls it in is another — or split the logic across several scopes, each with its own `circuit` line",
            blocks(DUST_ATTENUATION_LIMIT + 1),
        ),
        Allowance::Budget(_) => format!(
            "Fix: leave the wire into {cell} room to run straight within {near} of it, or shorten the wire out of it so it can spare more — a larger `region=` is one way to do either — or drive {cell} from a cell that restores strength, or split the logic across several scopes, each with its own `circuit` line",
            cell = budget_cell(ir, unpowered),
            near = blocks(near),
        ),
    };
    error_with_footer(
        DiagnosticCode::AttenuationLimit,
        reservation.span.clone(),
        primary,
        footer,
    )
}

/// `(x,y,z)`, as a refusal names a coord.
fn at(coord: CellCoord) -> String {
    format!("({},{},{})", coord.x, coord.y, coord.z)
}

/// `n block` or `n blocks`.
fn blocks(n: u32) -> String {
    format!("{n} {}", if n == 1 { "block" } else { "blocks" })
}

/// Why a [`NoRepeaterSite`] had nowhere for a repeater: the clause of
/// its refusal after the colon. It names exactly the coords the walk
/// asked about, `unpowered` among them when the walk started there.
fn why_no_site(stretch: &NoRepeaterSite) -> String {
    const STRAIGHT: &str = "a buffer repeater carries a signal only where the wire runs straight through it at one height";
    const JOINS: &str = "joins only the blocks at its back and front";
    let NoRepeaterSite {
        from,
        unpowered,
        reach,
        starts_on_unpowered,
        blocked,
        refused,
        ..
    } = *stretch;
    let span = match blocked {
        Blocked::NoneBetween if starts_on_unpowered => {
            format!("after {} up to and including {}", at(from), at(unpowered))
        }
        Blocked::NoneBetween => "between the two".to_owned(),
        Blocked::NoneCloseEnough if starts_on_unpowered => {
            format!("from it back to {} before it", blocks(reach))
        }
        Blocked::NoneCloseEnough => format!("within {} before it", blocks(reach)),
    };
    let touches = |touching: Touching| {
        format!(
            "runs straight but touches another block of its own net on one of its faces, as {} touches {}",
            at(touching.at),
            at(touching.touching),
        )
    };
    let reason = match refused {
        Refused {
            bends: false,
            touching: None,
        } => format!("no coord stands {span}"),
        Refused {
            bends: true,
            touching: None,
        } => format!("every coord {span} turns, climbs or branches — {STRAIGHT}"),
        Refused {
            bends: false,
            touching: Some(touching),
        } => format!(
            "every coord {span} {} — a buffer repeater, which {JOINS}, would cut that block off from the dust it replaces",
            touches(touching),
        ),
        Refused {
            bends: true,
            touching: Some(touching),
        } => format!(
            "every coord {span} turns, climbs or branches, or {} — {STRAIGHT} and {JOINS}",
            touches(touching),
        ),
    };
    let nothing = refused == Refused::default();
    match blocked {
        Blocked::NoneBetween if nothing => {
            format!("{reason}, so there is nowhere for a buffer repeater")
        }
        Blocked::NoneBetween => format!("{reason}, so none can stand there"),
        Blocked::NoneCloseEnough => {
            format!("{reason}, and one further back would leave too little strength to get there")
        }
    }
}

/// `cell #i`, for the cell on `coord` that an [`Allowance::Budget`]
/// belongs to: budgets are only ever given to cells, by their coord.
fn budget_cell(ir: &PlacementIr, coord: CellCoord) -> String {
    let index = ir
        .cells
        .iter()
        .position(|cell| cell.coord == coord)
        .expect("a budget is only ever given to a cell's coord");
    format!("cell #{index}")
}

#[cfg(test)]
mod tests;
