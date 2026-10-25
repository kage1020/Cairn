//! Where a scope's implicit buffer repeaters stand. The delay and crossing passes each rebuild them
//! from their own trees, so this must stay deterministic.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::grid::{faces, net_ref_key};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{NetTree, net_label};
use crate::saturating_index;

/// The attenuation limit of `spec/redstone` "Place-and-route": the most blocks of dust a signal
/// crosses since its strength was last restored.
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
    /// Panics for a segment whose sink is not a terminal of its net.
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

/// Every implicit buffer repeater of one scope, net by net, kept with the tree it was walked on.
pub(crate) struct RepeaterSites<'t> {
    per_net: HashMap<NetRef, (&'t NetTree, HashSet<CellCoord>)>,
}

impl RepeaterSites<'_> {
    /// The repeaters `net`'s signal passes through on its way to `sink`, in order; `None` when
    /// `sink` is not a terminal of `net`.
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

/// [`repeater_sites`] for every net of one scope, or the `E_ATTENUATION_LIMIT` refusing it. The
/// dust into a port whose strength its cell passes on and the dust out of that cell count as one
/// run.
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

    // Every wire into a cell ends on its coord, the side's as well as the rear's, so a net sees
    // only the budgets of the cells it feeds on a port whose strength they pass on.
    let mut passed_on: HashMap<NetRef, Vec<CellCoord>> = HashMap::new();
    for cell in &ir.cells {
        for driver in &cell.drivers {
            if !cell.cell.port_regenerates(driver.port) {
                passed_on.entry(driver.net).or_default().push(cell.coord);
            }
        }
    }
    let held_to = |net: NetRef, budgets: &HashMap<CellCoord, u32>| -> HashMap<CellCoord, u32> {
        passed_on
            .get(&net)
            .into_iter()
            .flatten()
            .filter_map(|coord| budgets.get(coord).map(|budget| (*coord, *budget)))
            .collect()
    };

    let mut budgets: HashMap<CellCoord, u32> = HashMap::new();
    // Last to first: a cell's sinks follow it, so their budgets are already known.
    for (index, cell) in ir.cells.iter().enumerate().rev() {
        if cell.cell.regenerates() {
            continue;
        }
        let Some(tree) = trees.get(&cell_net(index)) else {
            budgets.insert(cell.coord, DUST_ATTENUATION_LIMIT);
            continue;
        };
        let held = held_to(cell_net(index), &budgets);
        // Acceptance is downward-closed in `spent`, so the first accepted from the top is the
        // budget, and the refusal kept is the one from the walk spending least.
        let mut refusal = None;
        let budget = (1..=DUST_ATTENUATION_LIMIT).rev().find(|spent| {
            match repeater_sites(tree, *spent, &held) {
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
                        .filter(|driver| !cell.cell.port_regenerates(driver.port))
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
        let sites = repeater_sites(tree, spent, &held_to(net, &budgets))
            .map_err(|stretch| refuse(net, tree, stretch))?;
        per_net.insert(net, (tree, sites.into_iter().collect()));
        spent_at_source.insert(net, spent);
    }
    Ok(RepeaterSites { per_net })
}

/// The dust a net's signal has spent on reaching `sink`.
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

/// A run of one net's dust with no coord close enough for a repeater.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NoRepeaterSite {
    /// The block the signal last left with its strength restored.
    from: CellCoord,
    /// Dust already spent on leaving `from`.
    spent: u32,
    /// The coord nearest the source that is past its allowance.
    unpowered: CellCoord,
    /// What `unpowered` may be reached over.
    allowance: Allowance,
    /// How many steps before `unpowered` a repeater may stand and still bring it within allowance.
    reach: u32,
    /// Whether the walk started on `unpowered` itself, a block of dust a repeater could replace.
    starts_on_unpowered: bool,
    /// How far back the walk went before it gave up.
    blocked: Blocked,
    /// What kept every coord it passed over from holding one.
    refused: Refused,
}

/// The most dust a coord may be reached over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Allowance {
    /// The [`DUST_ATTENUATION_LIMIT`].
    Limit,
    /// The budget of a cell that passes on the strength it reads on the port the wire feeds.
    Budget(u32),
}

/// How far back a [`NoRepeaterSite`]'s walk went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blocked {
    /// It walked back to `from` without finding a site.
    NoneBetween,
    /// It walked the whole allowance before `unpowered` without finding a site.
    NoneCloseEnough,
}

/// Which [`Misfit`]s kept the coords a walk passed over from holding a repeater.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Refused {
    /// Some coord turns, climbs or branches.
    bends: bool,
    /// The coord nearest `unpowered` refused for touching another block of its net.
    touching: Option<Touching>,
}

/// A coord refused for touching its own net, and the block of its net it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Touching {
    /// The coord a repeater was refused on.
    at: CellCoord,
    /// The first of its faces, in [`faces`] order, holding another coord of the net.
    touching: CellCoord,
}

impl Refused {
    /// Records a coord refused for `misfit`, keeping the first touching one, the nearest.
    fn add(&mut self, misfit: Misfit) {
        match misfit {
            Misfit::Bends => self.bends = true,
            Misfit::Touches(touching) => {
                self.touching.get_or_insert(touching);
            }
        }
    }
}

/// Where one net's repeaters stand, in [`NetTree::wire_path`] order, or the run with no coord for
/// one. Every step of `tree` must move one block, so its [`NetTree::unreachable`] must be empty.
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
    // A sink or a repeater only reads the dust before it, so it may be reached one block further.
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
        // The coord fixed first has the least depth, then `order` index, so the answer depends only
        // on the tree.
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
        // A repeater can replace dust past the limit, but not a sink.
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

/// The source or repeater the signal on `coord` last left at full strength.
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
    /// The coord turns, climbs or branches, or is the source.
    Bends,
    /// The wire runs straight through, but another block of the net is on one of its faces.
    Touches(Touching),
}

/// `Ok` when a repeater on `coord` carries the signal on: in line with one coord before and one
/// after at one height, with no other block of the net on any face, linked in the tree or not.
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

/// The refusal for a [`NoRepeaterSite`], naming the node the signal does not reach.
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
    let (reach, footer) = match allowance {
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
            (
                format!(
                    "runs out before {}, past the attenuation limit of {} of dust, and never reaches {dark}",
                    at(unpowered),
                    blocks(DUST_ATTENUATION_LIMIT),
                ),
                // A repeater reads the 15 blocks before it, so the wire needs a straight coord
                // every 16.
                format!(
                    "Fix: leave the wire room to run straight at least once in every {} — a larger `region=` is one way, moving what walls it in is another — or split the logic across several scopes, each with its own `circuit` line",
                    blocks(DUST_ATTENUATION_LIMIT + 1),
                ),
            )
        }
        Allowance::Budget(budget) => {
            let (cell, ports) = budget_ports(ir, unpowered, net);
            (
                format!(
                    "reaches {ports} of {cell} at {coord} over more than the {spare} of dust the cell can spare — {cell} passes on the strength it receives on {ports} rather than restoring it, and the wire past it needs the rest",
                    coord = at(unpowered),
                    spare = blocks(budget),
                ),
                format!(
                    "Fix: leave the wire into {ports} of {cell} room to run straight within {near} of it, or shorten the wire out of {cell} so it can spare more — a larger `region=` is one way to do either — or drive {ports} from a cell that restores strength, or split the logic across several scopes, each with its own `circuit` line",
                    near = blocks(near),
                ),
            )
        }
    };
    let why = why_no_site(&stretch);
    let primary = format!(
        "{netlist} netlist for {kind} `{name}` routes {net} so that the signal leaving {leaving} {reach}: {why}",
        kind = entry.kind.label(),
        name = entry.name,
        net = net_label(net, ir),
    );
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

/// The clause of a [`NoRepeaterSite`]'s refusal after the colon.
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

/// `cell #i`, for the cell on `coord` that an [`Allowance::Budget`] belongs to, and the ports of it
/// that `net` feeds and whose strength it passes on, such as port `a`.
fn budget_ports(ir: &PlacementIr, coord: CellCoord, net: NetRef) -> (String, String) {
    let (index, cell) = ir
        .cells
        .iter()
        .enumerate()
        .find(|(_, cell)| cell.coord == coord)
        .expect("a budget is only ever given to a cell's coord");
    let ports: Vec<String> = cell
        .drivers
        .iter()
        .filter(|driver| driver.net == net && !cell.cell.port_regenerates(driver.port))
        .map(|driver| format!("`{}`", driver.port.as_str()))
        .collect();
    let ports = match ports.as_slice() {
        [] => panic!("a net is held to a budget only through a port it feeds"),
        [only] => format!("port {only}"),
        [first, last] => format!("ports {first} and {last}"),
        [head @ .., last] => format!("ports {}, and {last}", head.join(", ")),
    };
    (format!("cell #{index}"), ports)
}

#[cfg(test)]
mod tests;
