//! Where a scope's implicit buffer repeaters stand.
//!
//! The delay pass charges ticks for these repeaters and the crossing pass records their coords.
//! Each builds its own [`RepeaterSites`] with [`repeater_sites_of_scope`] from the trees it
//! rebuilt; the function is deterministic and nothing between the two stages moves a cell, so the
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

/// Signal-attenuation ceiling, the attenuation limit of 15 blocks in `spec/redstone`
/// "Place-and-route": the most blocks of dust a signal crosses since the last block that restored
/// its strength.
///
/// A fresh source starts at strength 15 and dust decays one unit per block, so this many blocks
/// carry its signal, the last at strength 1, which still drives the block after it. A segment is
/// one step longer than its dust, so one of up to `DUST_ATTENUATION_LIMIT + 1` steps from a fresh
/// source needs no buffer repeater. From a cell that passes on the strength it reads, the dust
/// into the cell counts too, and a shorter segment can need one.
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
    /// Panics for a segment whose sink is not a terminal of the net that drives it, where the
    /// driver list and the collected nets disagree. Carrying on would under-count the node's
    /// buffers.
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

/// Where every implicit buffer repeater of one scope stands, net by net, beside the tree each
/// net's sites were walked on, so [`Self::along`] is never asked about one tree with the sites of
/// another.
pub(crate) struct RepeaterSites<'t> {
    per_net: HashMap<NetRef, (&'t NetTree, HashSet<CellCoord>)>,
}

impl RepeaterSites<'_> {
    /// The repeaters the signal of `net` passes through on its way to `sink`, in the order it
    /// meets them. Two sinks that share a stretch of the tree share its repeaters, because they
    /// are read off one set.
    ///
    /// `None` when `sink` is not a terminal of `net`, which the caller turns into
    /// [`Fed::is_not_a_terminal`].
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

/// [`repeater_sites`] for every net of one scope, or the `E_ATTENUATION_LIMIT` that refuses the
/// scope when a net has a run of dust past its allowance with no coord close enough for a
/// repeater.
///
/// The attenuation limit runs from the last block that restored strength, not from the start of
/// each net. A cell whose [`EditionCell::regenerates`] is `false` (a Bedrock OR, which is a dust
/// merge, or a Java comparator AND, which never outputs more than its rear input carries) passes
/// on the strength that reached it, so the dust before and after it count as one run. Two walks
/// carry that across the cells:
///
/// - **Backwards**, cells last to first, each such cell gets a *budget*: the most dust its inputs
///   may already have spent for the net it drives still to reach every one of its sinks. Its
///   sinks come after it, so their budgets are known. The budget is the largest `spent` in
///   `1..=15` that [`repeater_sites`] accepts; acceptance is downward-closed, since a smaller
///   `spent` lowers every running total. Zero is not tried: every cell is at least one block of
///   dust from its driver.
/// - **Forwards**, nets in [`net_ref_key`] order, each net starts with the dust already spent at
///   its source: none at a sensor pad or a cell that restores strength, otherwise the most any
///   one of its inputs arrives with. For a dust merge that is the worst case, since any one input
///   may be the only one on; for a comparator it is conservative, since the pass does not track
///   which input is the rear. Its repeaters go where the running total would pass the limit or a
///   sink's budget, which can be on the segment into a cell.
///
/// The backward walk refuses on the highest-index cell whose own wire cannot be fed at any budget;
/// otherwise the forward walk refuses on the first net in its order with nowhere left for a
/// repeater. Either way the net named does not depend on map iteration.
///
/// `trees` must be the ones [`lay_nets`] returned, whose [`NetTree::unreachable`] is always
/// empty: an unreachable sink is parented straight to the source at any distance, and
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
        // A cell that drives nothing spares the whole limit. A budget counts the cell's own coord
        // as dust, which keeps a dust merge lit; with no entry it would get a sink's allowance,
        // one block more. Only a hand-built IR reaches this: a `logic` binding nothing consumes
        // draws `W_LOGIC_UNUSED_SIGNAL` and its cell is dropped.
        let Some(tree) = trees.get(&cell_net(index)) else {
            budgets.insert(cell.coord, DUST_ATTENUATION_LIMIT);
            continue;
        };
        // One block spent is tried last, so when no budget fits, the refusal is the walk that
        // spends the least: the wire out of the cell is what has no room.
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

/// The dust a net's signal has spent on reaching `sink`: the steps since the last repeater on its
/// route, or, with none, the whole route on top of what was `spent` at the source.
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
    /// The block the signal last left with its strength restored: a repeater placed before this
    /// run, or the net's source, which is short of full strength by [`Self::spent`] when its
    /// driver passes strength on.
    from: CellCoord,
    /// Blocks of dust already spent on leaving `from`: zero at a repeater or at a source that
    /// restores strength.
    spent: u32,
    /// The coord nearest the source that is past its allowance (a block of dust past the limit,
    /// or a sink reached over more dust than it allows), the first in [`NetTree::wire_path`] on a
    /// tie.
    unpowered: CellCoord,
    /// What `unpowered` may be reached over.
    allowance: Allowance,
    /// How many steps before `unpowered` a repeater may stand and still bring it within its
    /// allowance, which bounds the walk for a site: [`DUST_ATTENUATION_LIMIT`] for a block of
    /// dust, one more for a sink with no budget, and the budget for a sink with one.
    reach: u32,
    /// Whether the walk started on `unpowered` itself, a block of dust a repeater could replace.
    /// It starts one coord back when `unpowered` is a sink, or when even the block before it is
    /// past the limit.
    starts_on_unpowered: bool,
    /// How far back the walk went before it gave up.
    blocked: Blocked,
    /// What kept every coord it passed over from holding one.
    refused: Refused,
}

/// The most dust a coord may be reached over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Allowance {
    /// [`DUST_ATTENUATION_LIMIT`]: a block of dust may be at most the limit's last; a sink or a
    /// repeater may be reached over all of it.
    Limit,
    /// A cell that passes on the strength it reads: the wire out of it needs the rest of the
    /// limit.
    Budget(u32),
}

/// How far back a [`NoRepeaterSite`]'s walk went. With
/// [`NoRepeaterSite::starts_on_unpowered`], that is which coords [`NoRepeaterSite::refused`]
/// describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blocked {
    /// It walked back to `from` without finding a site.
    NoneBetween,
    /// It walked the whole allowance before `unpowered` without finding a site; a repeater
    /// further back would leave the signal too weak to reach it.
    NoneCloseEnough,
}

/// Which [`Misfit`]s kept the coords a [`NoRepeaterSite`]'s walk passed over from holding a
/// repeater. Both empty when the first coord it asked about was already `from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Refused {
    /// Some coord turns, climbs or branches.
    bends: bool,
    /// The coord nearest `unpowered` that runs straight at one height but touches another block
    /// of its own net.
    touching: Option<Touching>,
}

/// A coord refused for touching its own net, and the block of its net it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Touching {
    /// The coord a repeater was refused on.
    at: CellCoord,
    /// The first of its faces, in [`faces`] order, that holds a coord of the same net other than
    /// the ones before and after it on the path.
    touching: CellCoord,
}

impl Refused {
    /// Records a coord refused for `misfit`. The first coord refused for touching is kept: the
    /// walk runs back from `unpowered`, so it is the nearest.
    fn add(&mut self, misfit: Misfit) {
        match misfit {
            Misfit::Bends => self.bends = true,
            Misfit::Touches(touching) => {
                self.touching.get_or_insert(touching);
            }
        }
    }
}

/// Where one net's implicit buffer repeaters stand, in [`NetTree::wire_path`] order, which is not
/// any one sink's route order ([`RepeaterSites::along`] gives that).
///
/// A repeater reads the block behind it and drives the block in front, at its own height, so it
/// can only stand where the wire runs straight through at one height with nothing branching off:
/// on a fork it would starve one branch, and on a turn or climb it drives into nothing.
///
/// Placed on the net's tree rather than per sink, so every sink past a repeater is fed by that one
/// block. The signal leaves the source with `spent` blocks of dust behind it. A coord the wire
/// runs on past is dust, and may be at most the [`DUST_ATTENUATION_LIMIT`]th block since the
/// last full-strength one; a sink or a repeater only reads the dust before it, so it may be
/// reached over all of it, or over its entry in `budgets`. The walk finds the coord nearest the
/// source that is past its allowance and puts a repeater on the last coord back from it that can
/// hold one and is close enough (on that coord itself when it is dust and the block before it is
/// within the limit), then walks again.
///
/// On a straight run from a fresh source that is every `DUST_ATTENUATION_LIMIT + 1`th coord, the
/// count [`buffer_count_for_segment`] gives. Where a coord cannot hold one the repeater moves
/// earlier, so the run past it can need one more than its length implies; on a fork it moves onto
/// the trunk, where one block serves every branch.
///
/// `Err` when a coord past its allowance has no coord close enough behind it, since the last
/// full-strength block, that can hold a repeater.
///
/// Every step of `tree` must move one block, which holds for the trees [`crate::pass::lay_nets`]
/// hands on, whose [`NetTree::unreachable`] is empty.
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
    // Dust counts its own block against the limit. A leaf (a sink) and a site (a repeater) only
    // read the dust before them, so they may be reached one block further.
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
        // Dust spent since the last full-strength block, and depth from the source; `order` lists
        // a parent before its children, so one pass fills both.
        //
        // Of the coords past their allowance, the one fixed first has the least depth, then the
        // least `order` index, so the answer depends only on the tree. The walk back from it
        // either refuses or stops within its allowance, so every round adds one site or returns
        // `Err`. `first` holds the depth, the `order` index, the coord and the dust it is reached
        // over.
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
        // A repeater can replace dust past the limit if the dust before it is within the limit. A
        // sink cannot be replaced: the repeater has to stand before it.
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

/// The block the signal on `coord` last left at full strength: `coord` itself when it is the
/// source or a repeater, otherwise the nearest of those behind it.
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
    /// The coord turns, climbs or branches. The source, with no coord before it, lands here too,
    /// though [`repeater_sites`] stops before asking about it.
    Bends,
    /// The wire runs straight through it at one height, but another
    /// block of the same net is on one of its faces.
    Touches(Touching),
}

/// `Ok` when a repeater on `coord` would carry the signal on: one coord before it and exactly one
/// after, all three at the same height and on one line, and no other coord of the net touching
/// it. A coord that fails both is a [`Misfit::Bends`].
///
/// Touching is asked of the grid, not the parent links: dust joins the dust beside it whether or
/// not the tree runs between them, and a repeater joins only the blocks at its back and front, so
/// any other block of the net beside it would be cut off. The check takes all six faces, as
/// `spec/redstone` "Place-and-route" does.
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

/// The refusal for a [`NoRepeaterSite`]. It names the node left without the signal: past the
/// limit, the first cell, then the first actuator pad, whose route runs through `unpowered`;
/// within a budget, the cell on `unpowered`, which the signal reaches too weak to pass on.
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
        // A repeater stands on the block past the limit at the latest, reading the 15 before it,
        // so the wire needs a straight coord in every 16 blocks.
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

/// Why a [`NoRepeaterSite`] had nowhere for a repeater: the clause of its refusal after the colon,
/// naming exactly the coords the walk asked about.
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

/// `cell #i`, for the cell on `coord` that an [`Allowance::Budget`] belongs to.
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
