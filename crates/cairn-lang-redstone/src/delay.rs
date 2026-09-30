//! Routed Placement IR → delayed Placement IR lowering (delay insertion).
//!
//! Stage 3 of the five-stage pipeline `spec/redstone` "Place-and-route"
//! lays out. Rebuilds every net's routed tree (the routing pass stores
//! only the summed `wire_length`; the trees are cheap to rebuild
//! and would bloat the JSON if stored) and rewrites every cell's
//! [`crate::placement_ir::PlacedCellNode::local_delay_ticks`] from `None`
//! to `Some(base delay + implicit buffer ticks)`:
//!
//! - **Base delay** is the cell's
//!   [`crate::edition_netlist_ir::EditionCell::base_delay_ticks`]; an
//!   actuator pad has none.
//! - **Implicit buffer repeaters** are counted per driving net, not per
//!   driver: two ports reading one signal are fed by one strand of dust
//!   and by the repeaters standing on it. Dust loses one unit of signal
//!   per block from strength 15, so `repeater_sites` stands a
//!   repeater, worth [`BUFFER_REPEATER_TICKS`], within every
//!   `DUST_ATTENUATION_LIMIT` blocks of dust since the last block that
//!   restored strength — on a straight run from a fresh source every
//!   15 blocks, `floor((s - 1) / 15)` of them over `s` blocks, and
//!   earlier where the coord there turns, climbs or forks, which a
//!   repeater cannot carry a signal through.
//!
//!   That last block is not always the start of the net. A cell that
//!   passes on the strength it reads (a Bedrock OR, a Java comparator
//!   AND) is not a strand of dust — its body stands between the wire
//!   into it and the wire out — but it restores nothing, so for the
//!   limit the two wires count as one run, and a repeater that run
//!   needs can land on either side of the cell.
//!
//! A segment is the *routed* path from the net's source to the sink
//! (`route_to`), not the Manhattan distance: the two differ whenever the
//! wire goes round something, and counting against the route is what
//! lets stage 4 put every buffer this stage paid for onto the dust it
//! refreshes. A cell is charged for the repeaters its segment passes
//! through, and stage 4 gives those same repeaters their coords. Each
//! pass builds its own `RepeaterSites` by running the same
//! deterministic function over its own rebuilt trees and the same
//! `ir.cells`, so the two agree as long as nothing between them moves
//! a cell; `crossing.rs`'s proptest checks that they do.
//!
//! **`local_delay_ticks` is a local wire cost, not an arrival time.** It
//! sums the buffers on every net feeding the cell, and it is the number
//! stage 4 is held to: `local_delay_ticks - base_delay_ticks` equals
//! [`BUFFER_REPEATER_TICKS`] per block in the cell's *deduplicated*
//! `buffer_coords` — the attribution list may name a coord twice, the
//! count may not. An arrival time would
//! take the max over those nets and add the arrival of each upstream
//! driver, so the two part company whenever more than one incoming net
//! carries a buffer or any driver is itself a cell. Nothing in this crate
//! computes an arrival time, and nothing should read this field as one —
//! `assert latency(...)` (`spec/redstone` "Verification") is a path
//! latency for the simulator pass to evaluate.
//!
//! `E_ATTENUATION_LIMIT` fires when a single segment exceeds the v1
//! sanity cap [`MAX_ATTENUATION_SEGMENT`]: it asks for a buffer chain
//! longer than v1 will build. It also fires when a run of dust would
//! pass the attenuation limit — counted from the last block that
//! restored strength, or within a cell's smaller budget — and no coord
//! on it can hold a repeater close enough. Failed scopes are elided so a partial `local_delay_ticks`
//! set never reaches a downstream reader.
//!
//! The pass is one `PlacementPhase::delay` transition per cell; no new
//! IR type. `--stage route` JSON keeps every key it had, gains
//! `local_delay_ticks` after `wire_length`, and its `stage` tag moves
//! from `route` to `delay`.

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::netlist_ir::NetRef;
use crate::pass::{
    OpenScope, Skipped, attribute_nodes, lay_nets, lower_scopes, missing_region_diagnostic,
    open_scope, source_of_net,
};
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use std::collections::{HashMap, HashSet};

use crate::routing_geometry::{NetTree, net_label, net_ref_key, sum_over_driving_nets};
use crate::saturating_index;

/// Signal-attenuation ceiling (`spec/redstone` "Place-and-route" —
/// "signal attenuation limit of 15"): the most blocks of dust a signal
/// crosses since the last block that restored its strength. A fresh
/// source starts at strength 15 and dust decays one unit per block, so
/// a segment of at most this many blocks from a fresh source reaches
/// its sink at strength ≥ 1 without a buffer repeater. From a cell
/// that passes on the strength it reads, the dust into the cell counts
/// too, and a shorter segment can need one.
pub const DUST_ATTENUATION_LIMIT: u32 = 15;

/// Tick delay added by one implicit buffer repeater. Matches the
/// Minecraft default repeater `delay=1` setting; a follow-up pass that
/// exposes per-buffer delay tuning (Tier 0 `repeater delay=<N>`) would
/// swap this constant for a per-buffer field on the routed IR.
pub const BUFFER_REPEATER_TICKS: u32 = 1;

/// Compile-time guard on [`BUFFER_REPEATER_TICKS`]. A default repeater
/// cannot delay by less than one tick — if this constant is ever set
/// to zero, `buffer_repeater_ticks_for_segment` would report zero
/// implicit-buffer contribution for any segment length, silently
/// under-reporting `local_delay_ticks` on every fixture that crosses the
/// 15-block attenuation limit. Assert forces the value ≥ 1 so a
/// future edit cannot slide past this without deliberate intent.
const _: () = assert!(
    BUFFER_REPEATER_TICKS >= 1,
    "BUFFER_REPEATER_TICKS must be at least 1 — a default repeater delays by one tick",
);

/// Compile-time guard on the sanity cap: it must sit above the
/// attenuation limit, otherwise the "beyond attenuation limit but
/// within cap" band that `buffer_repeater_ticks_for_segment` fills
/// with implicit buffers would be empty and every segment past 15
/// blocks would refuse instead of being absorbed by buffers.
const _: () = assert!(
    MAX_ATTENUATION_SEGMENT > DUST_ATTENUATION_LIMIT,
    "MAX_ATTENUATION_SEGMENT must exceed DUST_ATTENUATION_LIMIT so implicit buffers have a band to cover",
);

/// v1 sanity cap on a single driver segment. A segment longer than this
/// asks for a buffer chain longer than v1 will build, so the pass that
/// measures it refuses with `E_ATTENUATION_LIMIT` rather than count a
/// chain nothing materialises into `local_delay_ticks`.
///
/// Two passes measure against it, and they measure different things.
/// [`compile_delay`] applies it to the segment's *routed* length — the
/// dust the signal travels — which is the cap proper.
/// `crate::pass::lay_nets`, which all three place-and-route passes
/// call, applies it to the straight line between the segment's ends,
/// before a route is laid. The straight line is a floor on every route
/// between two coords — it is the router's own admissible heuristic —
/// so a pair further apart than this has no route any pass would
/// accept, and laying one first buys nothing but the wait. That gate
/// therefore refuses strictly less than this one does: everything it
/// turns away, the delay pass would have turned away after the work.
///
/// 256 blocks is 17 buffer repeaters from a fresh source
/// (`(256 - 1) / 15`); anything past
/// that in a single flat segment reads as a placement mistake rather
/// than a routing corner case in every fixture the crate ships today.
pub const MAX_ATTENUATION_SEGMENT: u32 = 256;

/// Output of a [`compile_delay`] run.
///
/// Mirrors [`crate::routing::RoutingOutput`]'s shape so callers see a
/// uniform result type across every stage of the place-and-route
/// pipeline. The delayed IR is a [`ScopedPlacementIr`] with every
/// non-failed scope's `local_delay_ticks` promoted from `None` to `Some(_)` —
/// no new IR type; the delay pass is one
/// [`crate::placement_ir::PlacementPhase::delay`] transition per cell,
/// per the producer↔variant table on that enum.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct DelayOutput {
    /// Placement IR for every scope whose delay insertion succeeded,
    /// with every cell's `local_delay_ticks` field populated.
    pub scoped: ScopedPlacementIr,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

impl DelayOutput {
    /// Empty output (no delayed scopes, no diagnostics).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lower a routed [`ScopedPlacementIr`] into a delayed
/// [`ScopedPlacementIr`].
///
/// Reads every cell's [`crate::edition_netlist_ir::EditionCell`] and
/// the scope's [`CircuitRegionReservation`] out of the input IR — the
/// Placement IR is self-describing by construction, so the delay pass
/// has no `IntentModule` dependency.
///
/// One entry per non-empty [`PlacementIr`] whose delay insertion
/// succeeded; scopes that raise an Error-severity diagnostic are
/// elided from the output so a partial `local_delay_ticks` set cannot
/// pollute a downstream reader. This pass raises `E_ATTENUATION_LIMIT`
/// itself, and carries the two `E_ROUTE_CONGESTION` refusals and the
/// `E_NO_CIRCUIT_REGION` that `crate::pass` asks on its behalf.
#[must_use]
pub fn compile_delay(routed: &ScopedPlacementIr) -> DelayOutput {
    let (scoped, diagnostics) = lower_scopes(routed, |entry| {
        delay_scope(entry).map(|ir| (ir, Vec::new()))
    });
    DelayOutput {
        scoped,
        diagnostics,
    }
}

/// Result of delaying one scope: the delayed IR on success, a single
/// Error-severity diagnostic on failure.
type ScopeDelay = Result<PlacementIr, Diagnostic>;

fn delay_scope(entry: &ScopedPlacementIrEntry) -> ScopeDelay {
    let source = &entry.ir;
    let OpenScope {
        mut ir,
        region,
        cell_coords,
        inputs,
        blocks,
    } = match open_scope(entry) {
        Err(Skipped::Empty) => return Ok(source.clone()),
        // Refused rather than passed through as routing does: this pass
        // writes `local_delay_ticks`, and the producer↔variant table on
        // `PlacementPhase` promises it after this stage.
        Err(Skipped::MissingRegion) => {
            return Err(missing_region_diagnostic(
                entry,
                "routed",
                "delay insertion",
            ));
        }
        Ok(scope) => scope,
    };

    let nets = lay_nets(
        &ir,
        &blocks,
        entry,
        "routed",
        &region,
        source_of_net(&region, &cell_coords, inputs),
    )?;

    // Refuse before writing `local_delay_ticks`, so a failed scope leaves
    // no partial attribution behind.
    for (cell_index, cell) in ir.cells.iter().enumerate() {
        for (driver_index, driver) in cell.drivers.iter().enumerate() {
            let segment = nets.segment(driver.net, cell.coord);
            if segment > MAX_ATTENUATION_SEGMENT {
                return Err(attenuation_diagnostic(
                    entry,
                    &region,
                    cell_index,
                    driver_index,
                    segment,
                ));
            }
        }
    }
    // An actuator wired straight to a sensor across a wide region hits
    // the same cap without touching a cell.
    for (output_index, output) in ir.outputs.iter().enumerate() {
        let segment = nets.segment(output.driver, output.pad);
        if segment > MAX_ATTENUATION_SEGMENT {
            return Err(attenuation_output_diagnostic(
                entry,
                &region,
                output_index,
                segment,
            ));
        }
    }

    let sites = repeater_sites_of_scope(&ir, entry, "routed", &region, &nets.trees)?;
    attribute_local_delay_ticks(&mut ir, entry, &|net, sink| {
        saturating_index(sites.along(&nets.trees, net, sink).len())
    });

    Ok(ir)
}

/// Fill every cell's `local_delay_ticks` with `base_delay(cell) + Σ buffer
/// ticks per driving net`, and every actuator pad's with the buffer
/// ticks on its own segment (a pad is not a cell, so no base delay).
///
/// `buffers_on(net, sink)` is how many repeaters the signal of `net`
/// passes through on its way to `sink` — [`RepeaterSites::along`] in
/// the pass.
///
/// Per net rather than per driver — see [`sum_over_driving_nets`] — and
/// summed rather than maxed, because the figure is a local wire cost
/// and not the tick the cell's output settles on (see the module doc).
fn attribute_local_delay_ticks<F>(
    ir: &mut PlacementIr,
    entry: &ScopedPlacementIrEntry,
    buffers_on: &F,
) where
    F: Fn(NetRef, CellCoord) -> u32,
{
    attribute_nodes(
        ir,
        entry,
        |cell| {
            let buffer_ticks = sum_over_driving_nets(&cell.drivers, |net| {
                buffers_on(net, cell.coord).saturating_mul(BUFFER_REPEATER_TICKS)
            });
            cell.cell.base_delay_ticks().saturating_add(buffer_ticks)
        },
        |output| buffers_on(output.driver, output.pad).saturating_mul(BUFFER_REPEATER_TICKS),
        |phase, ticks, identity| phase.delay_at(ticks, identity),
    );
}

/// Where every implicit buffer repeater of one scope stands, net by
/// net.
///
/// Built once per scope by [`repeater_sites_of_scope`], which stage 3
/// counts from and stage 4 places from, so the ticks one pass charges
/// and the blocks the other lays are one set read twice.
pub(crate) struct RepeaterSites {
    per_net: HashMap<NetRef, HashSet<CellCoord>>,
}

impl RepeaterSites {
    /// The repeaters the signal of `net` passes through on its way to
    /// `sink`, in the order it meets them.
    ///
    /// Two sinks of one net that share a stretch of the tree share the
    /// repeaters standing on it, because they are read off one set
    /// rather than placed per sink.
    ///
    /// Panics when `sink` is not a terminal of `net`: the driver list
    /// and the collected nets would then disagree, and the alternative
    /// is a node whose buffers under-count the dust it is fed through.
    pub(crate) fn along(
        &self,
        trees: &HashMap<NetRef, NetTree>,
        net: NetRef,
        sink: CellCoord,
    ) -> Vec<CellCoord> {
        let route = trees
            .get(&net)
            .and_then(|tree| tree.route_to(sink))
            .unwrap_or_else(|| {
                panic!(
                    "sink ({x},{y},{z}) is not a terminal of the net driving it — the driver list and the collected nets disagree",
                    x = sink.x,
                    y = sink.y,
                    z = sink.z,
                )
            });
        let Some(sites) = self.per_net.get(&net) else {
            return Vec::new();
        };
        route
            .into_iter()
            .filter(|coord| sites.contains(coord))
            .collect()
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
/// [`EditionCell::regenerates`]: crate::edition_netlist_ir::EditionCell::regenerates
pub(crate) fn repeater_sites_of_scope(
    ir: &PlacementIr,
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    region: &CircuitRegionReservation,
    trees: &HashMap<NetRef, NetTree>,
) -> Result<RepeaterSites, Diagnostic> {
    let refuse = |net: NetRef, stretch: NoRepeaterSite| {
        no_repeater_site_diagnostic(ir, entry, netlist, region, net, stretch)
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
        // A cell that drives nothing can be reached with any strength
        // at all, which is the limit the budget defaults to.
        let Some(tree) = trees.get(&cell_net(index)) else {
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
            return Err(refuse(cell_net(index), stretch));
        };
        budgets.insert(cell.coord, budget);
    }

    let mut nets: Vec<NetRef> = trees.keys().copied().collect();
    nets.sort_by_key(|net| net_ref_key(*net));
    let mut per_net: HashMap<NetRef, HashSet<CellCoord>> = HashMap::with_capacity(nets.len());
    let mut spent_at_source: HashMap<NetRef, u32> = HashMap::with_capacity(nets.len());
    for net in nets {
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
                            spent_on_arrival(
                                trees.get(&driver.net).expect(PLACED),
                                per_net.get(&driver.net).expect(PLACED),
                                *spent_at_source.get(&driver.net).expect(PLACED),
                                cell.coord,
                            )
                        })
                        .max()
                        .unwrap_or(0)
                }
            }
        };
        let sites = repeater_sites(&trees[&net], spent, &budgets)
            .map_err(|stretch| refuse(net, stretch))?;
        per_net.insert(net, sites.into_iter().collect());
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
pub(crate) struct NoRepeaterSite {
    /// The block the signal last left with its strength restored: a
    /// repeater placed before this run, or the net's source. At the
    /// source of a net whose driver passes strength on, that is less
    /// than full strength, by [`Self::spent`].
    pub(crate) from: CellCoord,
    /// Blocks of dust already spent on leaving `from`: zero at a
    /// repeater or at a source that restores strength.
    pub(crate) spent: u32,
    /// The coord nearest the source that the signal reaches over more
    /// dust than its allowance.
    pub(crate) unpowered: CellCoord,
    /// What `unpowered` may be reached over.
    pub(crate) allowance: Allowance,
    /// Why no repeater could stand before it.
    pub(crate) blocked: Blocked,
}

/// The most dust a coord may be reached over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Allowance {
    /// [`DUST_ATTENUATION_LIMIT`]: any coord but the one below.
    Limit,
    /// A cell that passes on the strength it reads: the wire out of it
    /// needs the rest of the limit, so it may be reached over only
    /// this much.
    Budget(u32),
}

impl Allowance {
    fn blocks(self) -> u32 {
        match self {
            Self::Limit => DUST_ATTENUATION_LIMIT,
            Self::Budget(budget) => budget,
        }
    }
}

/// Why a [`NoRepeaterSite`] has no repeater.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// Every coord between `from` and `unpowered` turns, climbs or
    /// branches.
    EveryCoordTurns,
    /// Every coord within the allowance before `unpowered` turns,
    /// climbs or branches, and the straight coords further back are
    /// too far from it to leave the signal enough.
    NoneCloseEnough,
}

/// Where one net's implicit buffer repeaters stand, in the order the
/// tree was laid.
///
/// A repeater is directional: it reads the block behind it and drives
/// the block in front of it, on its own layer. So it can only stand on
/// a coord the wire runs straight through, from the coord before it to
/// the one after, all three on one layer and on one line, and from
/// which nothing else branches — a repeater on a fork drives one branch
/// and starves the other, and one on a turn or a climb drives into
/// nothing.
///
/// Placed on the net's tree rather than per sink, so every sink past a
/// repeater is fed by that one block. The signal leaves the source with
/// `spent` blocks of dust already behind it, and every coord may be
/// reached over at most [`DUST_ATTENUATION_LIMIT`] blocks of dust since
/// the last full-strength block — or over its entry in `budgets`, for a
/// sink that passes its strength on. The walk finds the coord nearest
/// the source that is past its allowance, and puts a repeater on the
/// last coord before it, on the way back towards the source or the
/// repeater before, that can hold one and is close enough; then walks
/// again. On a straight run from a fresh source that is every
/// `DUST_ATTENUATION_LIMIT` coords, the count
/// [`buffer_count_for_segment`] gives. Where a coord cannot hold one
/// the repeater moves earlier, which only shortens the dust behind it,
/// and the run past it can need one repeater more than its length
/// alone implies. On a fork it moves onto the trunk, where one block
/// serves every branch past it.
///
/// `Err` when a coord past its allowance has no coord behind it, since
/// the last full-strength block, that can hold a repeater close enough.
fn repeater_sites(
    tree: &NetTree,
    spent: u32,
    budgets: &HashMap<CellCoord, u32>,
) -> Result<Vec<CellCoord>, NoRepeaterSite> {
    let allowance = |coord: CellCoord| {
        budgets
            .get(&coord)
            .copied()
            .unwrap_or(DUST_ATTENUATION_LIMIT)
    };
    let order = tree.wire_path();
    let source = order[0];
    let mut children: HashMap<CellCoord, Vec<CellCoord>> = HashMap::new();
    for coord in &order[1..] {
        let parent = tree
            .parent(*coord)
            .expect("every coord but the source was attached to one");
        children.entry(parent).or_default().push(*coord);
    }
    let mut sites: HashSet<CellCoord> = HashSet::new();
    loop {
        // Dust spent since the last full-strength block, and depth from
        // the source to break ties. `order` lists a parent before its
        // children, so one pass fills both.
        let mut walked: HashMap<CellCoord, (u32, u32)> = HashMap::with_capacity(order.len());
        walked.insert(source, (spent, 0));
        let mut first: Option<(u32, usize, CellCoord)> = None;
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
            if here.0 > allowance(*coord)
                && first.is_none_or(|(depth, at, _)| (here.1, index) < (depth, at))
            {
                first = Some((here.1, index, *coord));
            }
        }
        let Some((depth, _, unpowered)) = first else {
            break;
        };
        let budget = allowance(unpowered);
        let mut candidate = tree
            .parent(unpowered)
            .expect("a coord past its allowance is never the source");
        loop {
            // `candidate` is an ancestor of `unpowered`, so it is the
            // shallower of the two.
            let candidate_depth = walked
                .get(&candidate)
                .expect("every coord of the tree was walked")
                .1;
            debug_assert!(candidate_depth < depth, "the walk back only climbs");
            let too_far = depth.saturating_sub(candidate_depth) > budget;
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
                    blocked: if fresh {
                        Blocked::EveryCoordTurns
                    } else {
                        Blocked::NoneCloseEnough
                    },
                });
            }
            if carries_straight_through(tree, &children, candidate) {
                sites.insert(candidate);
                break;
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

/// Whether a repeater on `coord` would carry the signal on: one coord
/// before it and exactly one after, all three on one layer and one
/// line.
fn carries_straight_through(
    tree: &NetTree,
    children: &HashMap<CellCoord, Vec<CellCoord>>,
    coord: CellCoord,
) -> bool {
    let Some(before) = tree.parent(coord) else {
        return false;
    };
    let [after] = children.get(&coord).map_or(&[][..], Vec::as_slice) else {
        return false;
    };
    let step = |from: CellCoord, to: CellCoord| {
        (
            i64::from(to.x) - i64::from(from.x),
            i64::from(to.y) - i64::from(from.y),
            i64::from(to.z) - i64::from(from.z),
        )
    };
    let (dx, dy, dz) = step(before, coord);
    dy == 0 && (dx, dy, dz) == step(coord, *after)
}

/// The fewest buffer repeaters that keep `segment` blocks of dust at
/// strength ≥ 1 at the sink.
///
/// A source at strength 15 loses one unit per block, so segments of
/// at most `DUST_ATTENUATION_LIMIT` blocks reach the sink without a
/// buffer; a 16-block segment needs one; each further
/// `DUST_ATTENUATION_LIMIT` blocks bumps the count by one. Saturating
/// so a pathological segment from a hand-built IR cannot overflow.
///
/// Exact on a straight, unbranched run, and a floor on any other:
/// [`repeater_sites`] is what places them, and a coord a repeater
/// cannot stand on moves the repeater earlier. So this only sizes the
/// chain a refused segment would have needed.
pub(crate) fn buffer_count_for_segment(segment: u32) -> u32 {
    if segment <= DUST_ATTENUATION_LIMIT {
        return 0;
    }
    (segment.saturating_sub(1)) / DUST_ATTENUATION_LIMIT
}

/// [`buffer_count_for_segment`] converted to ticks by
/// [`BUFFER_REPEATER_TICKS`].
fn buffer_repeater_ticks_for_segment(segment: u32) -> u32 {
    buffer_count_for_segment(segment).saturating_mul(BUFFER_REPEATER_TICKS)
}

/// The refusal for a [`NoRepeaterSite`]: a run of `net`'s dust past
/// its allowance with no coord close enough for a repeater.
fn no_repeater_site_diagnostic(
    ir: &PlacementIr,
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    reservation: &CircuitRegionReservation,
    net: NetRef,
    stretch: NoRepeaterSite,
) -> Diagnostic {
    let NoRepeaterSite {
        from,
        spent,
        unpowered,
        allowance,
        blocked,
    } = stretch;
    let at = |c: CellCoord| format!("({},{},{})", c.x, c.y, c.z);
    let blocks = |n: u32| format!("{n} {}", if n == 1 { "block" } else { "blocks" });
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
        Allowance::Limit => format!(
            "runs out before {}, past the attenuation limit of {} of dust",
            at(unpowered),
            blocks(DUST_ATTENUATION_LIMIT),
        ),
        Allowance::Budget(budget) => format!(
            "reaches {cell} over more than the {spare} of dust it can spare — {cell} passes on the strength it receives rather than restoring it, and the wire past it needs the rest",
            cell = at(unpowered),
            spare = blocks(budget),
        ),
    };
    let why = match blocked {
        Blocked::EveryCoordTurns => "every coord between the two turns, climbs or branches — a buffer repeater carries a signal only where the wire runs straight through it at one height, so none can stand there".to_owned(),
        Blocked::NoneCloseEnough => format!(
            "every coord within {} before it turns, climbs or branches — a buffer repeater carries a signal only where the wire runs straight through it at one height, and one further back would leave too little strength to get there",
            blocks(allowance.blocks()),
        ),
    };
    let primary = format!(
        "{netlist} netlist for {kind} `{name}` routes {net} so that the signal leaving {leaving} {reach}: {why}",
        kind = entry.kind.label(),
        name = entry.name,
        net = net_label(net, ir),
    );
    let footer = match allowance {
        Allowance::Limit => format!(
            "Fix: leave the wire room to run straight at least once in every {} — a larger `region=` is one way, moving what walls it in is another — or split the logic across several `circuit` blocks",
            blocks(DUST_ATTENUATION_LIMIT),
        ),
        Allowance::Budget(budget) => format!(
            "Fix: leave the wire into {cell} room to run straight within {near} of it, or shorten the wire out of it so it can spare more — a larger `region=` is one way to do either — or drive {cell} from a cell that restores strength, or split the logic across several `circuit` blocks",
            cell = at(unpowered),
            near = blocks(budget),
        ),
    };
    error_with_footer(
        DiagnosticCode::AttenuationLimit,
        reservation.span.clone(),
        primary,
        footer,
    )
}

fn attenuation_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    cell_index: usize,
    driver_index: usize,
    segment: u32,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` has a driver segment of {segment} blocks into cell #{cell_index} port #{driver_index} — exceeds the v1 attenuation limit of {cap} blocks (dust decays 1/block, so this segment would need at least {buffers} buffer repeaters to materialize)",
        kind = entry.kind.label(),
        name = entry.name,
        cap = MAX_ATTENUATION_SEGMENT,
        buffers = buffer_repeater_ticks_for_segment(segment) / BUFFER_REPEATER_TICKS.max(1),
    );
    error_with_footer(
        DiagnosticCode::AttenuationLimit,
        reservation.span.clone(),
        primary,
        "Fix: enlarge `region=` so no driver→cell segment exceeds the cap, split into multiple `circuit` blocks, or pin cell placement closer to its drivers",
    )
}

fn attenuation_output_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    output_index: usize,
    segment: u32,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` has a driver segment of {segment} blocks into output pad #{output_index} — exceeds the v1 attenuation limit of {cap} blocks (dust decays 1/block, so this segment would need at least {buffers} buffer repeaters to materialize)",
        kind = entry.kind.label(),
        name = entry.name,
        cap = MAX_ATTENUATION_SEGMENT,
        buffers = buffer_repeater_ticks_for_segment(segment) / BUFFER_REPEATER_TICKS.max(1),
    );
    error_with_footer(
        DiagnosticCode::AttenuationLimit,
        reservation.span.clone(),
        primary,
        "Fix: enlarge `region=` so no driver→sink segment exceeds the cap, split into multiple `circuit` blocks, or pin actuator placement closer to its drivers",
    )
}

#[cfg(test)]
mod tests {
    //! Delay-pass behaviours `tests/delay.rs` cannot reach through synth
    //! fixtures: shapes only a hand-built `PlacementIr` produces.

    use std::collections::{HashMap, HashSet};

    use cairn_lang_core::Edition;
    use cairn_lang_core::error::Span;

    use super::{
        Allowance, BUFFER_REPEATER_TICKS, Blocked, DUST_ATTENUATION_LIMIT, MAX_ATTENUATION_SEGMENT,
        NoRepeaterSite, attribute_local_delay_ticks, buffer_count_for_segment,
        buffer_repeater_ticks_for_segment, compile_delay, no_repeater_site_diagnostic,
        repeater_sites, repeater_sites_of_scope,
    };
    use crate::diagnostic::DiagnosticCode;
    use crate::edition_netlist_ir::EditionCell;
    use crate::logic_ir::ScopeKind;
    use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
    use crate::placement_ir::{
        CellCoord, PlacedCellNode, PlacementIr, PlacementPhase, ScopedPlacementIrEntry,
    };
    use crate::routing_geometry::NetTree;
    use crate::test_fixtures::{reservation, scoped, staircase};

    /// The sanity cap counts the dust, not the distance.
    ///
    /// An actuator pad joins its driver's net as a terminal, and the
    /// tree reaches it through the cell row rather than straight down
    /// the region — so the segment into a pad at the far edge is
    /// `width + 2` where the straight line is `width`. At `width =
    /// 256` that is the difference between "at the cap" and "over it",
    /// and it is a shape v1's placement pass produces: one sensor
    /// driving both a cell and a door.
    ///
    /// The two widths are asserted together so the boundary is pinned
    /// from both sides rather than by one row that a changed constant
    /// would slide past.
    #[test]
    fn attenuation_cap_measures_the_routed_output_segment() {
        for (width, refuses) in [(255u32, false), (256, true)] {
            let mut ir = PlacementIr::new(Edition::Java);
            ir.region = Some(reservation(width, 4, 2));
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
                span: Span::default(),
            });
            // Something standing on the straight line, so the route out
            // to the pad is the two blocks longer that going round it
            // costs. Without it the route and the straight line are one
            // number and the fixture cannot tell which the cap was
            // measured against.
            ir.cells.push(placed_cell(
                EditionCell::JavaRepeaterOr,
                CellCoord::new(10, 0, 0),
                Vec::new(),
            ));
            ir.outputs.push(routed_output(
                NetRef::Input(0),
                CellCoord::new(width - 1, 0, 0),
            ));
            let delayed = compile_delay(&scoped(ScopeKind::Struct, "wide", ir));
            let fired = delayed
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::AttenuationLimit);
            assert_eq!(
                fired,
                refuses,
                "width {width}: straight line {}, routed {}, cap {MAX_ATTENUATION_SEGMENT}; got {:?}",
                width - 1,
                width + 1,
                delayed.diagnostics,
            );
        }
    }

    /// An actuator pad already through routing, matching what
    /// [`placed_cell`] does for a cell: these fixtures hand the delay
    /// pass an IR that skipped stages 1 and 2, so both node kinds have
    /// to arrive in the phase stage 2 would have left them in.
    fn routed_output(driver: NetRef, pad: CellCoord) -> crate::placement_ir::PlacedOutputNode {
        let mut output = crate::placement_ir::PlacedOutputNode::new(
            cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["out".into()]),
            driver,
            pad,
            Span::default(),
        );
        output.phase = PlacementPhase::Routed { wire_length: 0 };
        output
    }

    fn placed_cell(
        cell: EditionCell,
        coord: CellCoord,
        drivers: Vec<CellPortDriver>,
    ) -> PlacedCellNode {
        PlacedCellNode {
            cell,
            drivers,
            coord,
            phase: PlacementPhase::Routed { wire_length: 0 },
            span: Span::default(),
        }
    }

    /// A cell is charged once per net that drives it, not once per
    /// port.
    ///
    /// Two ports on one net are fed by one strand of dust and by the
    /// repeaters standing on it: `segment_of` is a function of the
    /// `(net, sink)` pair, so the second port re-derives the first
    /// port's number and adding them describes a signal that passes
    /// through every repeater twice.
    ///
    /// The second row is the control that keeps the rule from
    /// collapsing to "charge a cell once": two ports on two nets are
    /// two segments, and both are charged. It lands on two charges
    /// where the first row lands on one, so a fold that dropped
    /// either net — or collapsed to one charge per cell — reports a
    /// different number. (The two segments are 16 and 17 blocks, which
    /// `buffer_count_for_segment` maps to the same 1: what separates
    /// the rows is the number of nets, not the lengths.)
    ///
    /// Both rows are wire costs, not arrival times — two nets each
    /// carrying one buffer cost two buffers and arrive after one.
    /// `local_delay_is_the_wire_cost_not_the_arrival_time` is where
    /// that distinction is pinned.
    #[test]
    fn a_cell_is_charged_once_per_net_that_drives_it() {
        for (label, port_b_net, expected) in [
            (
                "both ports on one net",
                NetRef::Input(0),
                1 + BUFFER_REPEATER_TICKS,
            ),
            (
                "one port each on two nets",
                NetRef::Input(1),
                1 + 2 * BUFFER_REPEATER_TICKS,
            ),
        ] {
            let mut ir = PlacementIr::new(Edition::Java);
            ir.region = Some(reservation(20, 3, 2));
            for name in ["a", "b"] {
                ir.inputs.push(crate::netlist_ir::NetlistInput {
                    name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
                    span: Span::default(),
                });
            }
            ir.cells.push(placed_cell(
                EditionCell::JavaComparatorAnd,
                CellCoord::new(16, 0, 1),
                vec![
                    CellPortDriver {
                        port: PortName::A,
                        net: NetRef::Input(0),
                    },
                    CellPortDriver {
                        port: PortName::B,
                        net: port_b_net,
                    },
                ],
            ));
            let delayed = compile_delay(&scoped(ScopeKind::Struct, "charged", ir));
            assert!(
                delayed.diagnostics.is_empty(),
                "{label}: {:?}",
                delayed.diagnostics,
            );
            assert_eq!(
                delayed.scoped.scopes[0].ir.cells[0].local_delay_ticks(),
                Some(expected),
                "{label}: base 1 plus one charge per driving net",
            );
        }
    }

    /// `local_delay_ticks` is the wire cost of every net feeding a
    /// cell, summed — not the tick the cell's output settles on.
    ///
    /// A cell whose `a` port arrives over a segment carrying one
    /// buffer and whose `b` port over one carrying two is charged for
    /// three buffers here, because three buffer blocks stand on the
    /// wires that feed it and stage 4 lays all three —
    /// `two_distinct_nets_charge_a_cell_for_every_buffer_under_it` is
    /// where that end-to-end identity is pinned.
    ///
    /// Its output settles at `base + 2`, because a combinational cell
    /// settles when the *last* of its inputs arrives. So an arrival
    /// time maxes where this sums. Both figures are derived here from
    /// the same two segment lengths — the recorded one through the
    /// fold, the arrival one through the same `max` an arrival walk
    /// would take — so a fold that switched to a max collapses the
    /// two and trips the assert, rather than the assert restating a
    /// constant written twice.
    ///
    /// Calls the fold directly with a stand-in `buffers_on` rather
    /// than routing a layout: what is pinned is how the per-net
    /// charges combine, and two segments whose buffer counts differ
    /// by one are three lines here and a placement puzzle through the
    /// router. `a_cell_is_charged_once_per_net_that_drives_it` is
    /// where real routed input segments are measured.
    #[test]
    fn local_delay_is_the_wire_cost_not_the_arrival_time() {
        // One buffer on the short segment, two on the long one.
        const SHORT: u32 = DUST_ATTENUATION_LIMIT + 1;
        const LONG: u32 = 2 * DUST_ATTENUATION_LIMIT + 1;
        assert_eq!(buffer_count_for_segment(SHORT), 1);
        assert_eq!(buffer_count_for_segment(LONG), 2);

        // No region and no inputs: the fold walks `ir.cells` and
        // reaches the nets only through the stand-in `buffers_on`, so
        // carrying geometry here would suggest it mattered.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(4, 0, 1),
            vec![
                CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                },
                CellPortDriver {
                    port: PortName::B,
                    net: NetRef::Input(1),
                },
            ],
        ));

        let entry = ScopedPlacementIrEntry {
            kind: ScopeKind::Struct,
            name: "arrival".to_owned(),
            ir: ir.clone(),
        };
        let buffers_on = |net: NetRef, _sink: CellCoord| -> u32 {
            match net {
                NetRef::Input(0) => buffer_count_for_segment(SHORT),
                NetRef::Input(1) => buffer_count_for_segment(LONG),
                other => panic!("no other net drives this cell: {other:?}"),
            }
        };
        attribute_local_delay_ticks(&mut ir, &entry, &buffers_on);

        let base = EditionCell::JavaComparatorAnd.base_delay_ticks();
        let recorded = ir.cells[0]
            .local_delay_ticks()
            .expect("delay insertion writes Some(_)");
        let ticks = |segment| buffer_count_for_segment(segment) * BUFFER_REPEATER_TICKS;
        assert_eq!(
            recorded,
            base + ticks(SHORT) + ticks(LONG),
            "the wire cost is every buffer on every net feeding the cell",
        );

        // What the cell's output actually settles on — the longer of
        // the two segments, not both of them — and what no pass in
        // this crate computes yet. Derived from the same two lengths
        // through the `max` an arrival walk would take, so a fold
        // that became a max would make the two figures equal.
        let arrival = base + ticks(SHORT).max(ticks(LONG));
        assert_eq!(
            recorded - arrival,
            ticks(SHORT),
            "the wire cost sits above the arrival time by the buffers on the shorter segment",
        );
    }

    /// The coords a walk from `start` visits, `start` included, taking
    /// each `(step, count)` in turn.
    fn walk(start: (u32, u32, u32), moves: &[((i64, i64, i64), u32)]) -> Vec<CellCoord> {
        let mut at = start;
        let mut path = vec![CellCoord::new(at.0, at.1, at.2)];
        let axis = |value: u32, delta: i64| {
            u32::try_from(i64::from(value) + delta).expect("the walk stays on the grid")
        };
        for &((dx, dy, dz), count) in moves {
            for _ in 0..count {
                at = (axis(at.0, dx), axis(at.1, dy), axis(at.2, dz));
                path.push(CellCoord::new(at.0, at.1, at.2));
            }
        }
        path
    }

    const EAST: (i64, i64, i64) = (1, 0, 0);
    const SOUTH: (i64, i64, i64) = (0, 0, 1);
    const UP: (i64, i64, i64) = (0, 1, 0);

    /// On a straight run the repeaters stand every 15 blocks, and there
    /// are as many as the segment's length alone implies.
    #[test]
    fn a_straight_run_takes_a_repeater_every_fifteen_blocks() {
        for length in [15u32, 16, 30, 31, 46] {
            let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, length)])]);
            let sites = repeater_sites(&tree, 0, &HashMap::new())
                .expect("a straight run has room everywhere");
            let expected: Vec<CellCoord> = (1..=buffer_count_for_segment(length))
                .map(|k| CellCoord::new(k * DUST_ATTENUATION_LIMIT, 0, 0))
                .collect();
            assert_eq!(sites, expected, "{length} blocks");
        }
    }

    /// A repeater never stands on a turn: it would drive into the
    /// coord straight ahead of it, which is not the wire.
    #[test]
    fn a_repeater_steps_back_off_a_turn() {
        // 15 east then 2 south: the 15-step point is the corner.
        let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, 15), (SOUTH, 2)])]);
        assert_eq!(
            repeater_sites(&tree, 0, &HashMap::new()),
            Ok(vec![CellCoord::new(14, 0, 0)]),
            "the corner is (15,0,0); the last straight coord before it is (14,0,0)",
        );
    }

    /// Nor on a climb: a repeater drives only its own layer, so a
    /// coord whose wire goes up or down — straight up included — is
    /// not one it can carry.
    #[test]
    fn a_repeater_steps_back_off_a_climb() {
        // 14 east, 2 up, 2 east. (14,1,0) has the wire straight through
        // it, but vertically; (14,0,0) is where it turns upward.
        let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, 14), (UP, 2), (EAST, 2)])]);
        assert_eq!(
            repeater_sites(&tree, 0, &HashMap::new()),
            Ok(vec![CellCoord::new(13, 0, 0)]),
            "neither the vertical run nor the foot of the climb holds a repeater",
        );
    }

    /// Nor on a fork: one repeater faces one branch. The one on the
    /// trunk before the fork refreshes both, so the two branches share
    /// it rather than each getting one.
    ///
    /// This is the shape of two actuators on one sensor, one straight
    /// down the row and one a row over past the first one's pad.
    #[test]
    fn a_repeater_steps_back_off_a_fork_onto_the_trunk() {
        let trunk = walk((0, 0, 0), &[(EAST, 16)]);
        let branch = walk((15, 0, 0), &[(SOUTH, 1), (EAST, 1)]);
        let tree = NetTree::from_paths(&[&trunk, &branch]);
        assert_eq!(
            repeater_sites(&tree, 0, &HashMap::new()),
            Ok(vec![CellCoord::new(14, 0, 0)]),
            "(15,0,0) forks; (14,0,0) is the trunk coord before it",
        );
    }

    /// Stepping back shortens the dust behind the repeater and
    /// lengthens the run ahead of it, so a route can need one more
    /// repeater than its length alone implies.
    #[test]
    fn stepping_back_can_cost_a_repeater_the_length_does_not_imply() {
        // 30 blocks, turning at the 15-step point.
        let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, 15), (SOUTH, 15)])]);
        let sites =
            repeater_sites(&tree, 0, &HashMap::new()).expect("there is straight wire on both legs");
        assert_eq!(
            sites,
            vec![CellCoord::new(14, 0, 0), CellCoord::new(15, 0, 14)],
        );
        assert_eq!(
            buffer_count_for_segment(30),
            1,
            "the length alone asks for one"
        );
    }

    /// A stretch past the limit on which every coord turns has nowhere
    /// for a repeater, and says so rather than placing one on a turn.
    #[test]
    fn a_staircase_has_nowhere_for_a_repeater() {
        let mut moves = Vec::new();
        for _ in 0..9 {
            moves.push((EAST, 1));
            moves.push((SOUTH, 1));
        }
        let path = walk((0, 0, 0), &moves);
        let tree = NetTree::from_paths(&[&path]);
        assert_eq!(
            repeater_sites(&tree, 0, &HashMap::new()),
            Err(NoRepeaterSite {
                from: CellCoord::new(0, 0, 0),
                spent: 0,
                unpowered: path[16],
                allowance: Allowance::Limit,
                blocked: Blocked::EveryCoordTurns,
            }),
        );
    }

    /// The stretch a refusal names starts at the last repeater, not at
    /// the source: the wire before that repeater is already refreshed,
    /// and a coord on it cannot stand in for one past it.
    #[test]
    fn a_staircase_past_a_repeater_is_measured_from_that_repeater() {
        let mut moves = vec![(EAST, 20)];
        for _ in 0..10 {
            moves.push((SOUTH, 1));
            moves.push((EAST, 1));
        }
        let path = walk((0, 0, 0), &moves);
        let tree = NetTree::from_paths(&[&path]);
        // (15,0,0) first; then (19,0,0), the last straight coord before
        // the staircase; then nothing past (19,0,0) is straight.
        assert_eq!(
            repeater_sites(&tree, 0, &HashMap::new()),
            Err(NoRepeaterSite {
                from: CellCoord::new(19, 0, 0),
                spent: 0,
                unpowered: path[19 + 16],
                allowance: Allowance::Limit,
                blocked: Blocked::EveryCoordTurns,
            }),
        );
    }

    /// Dust already spent at the source brings the first repeater
    /// forward by as much.
    #[test]
    fn dust_spent_at_the_source_brings_the_first_repeater_forward() {
        let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, 20)])]);
        assert_eq!(
            repeater_sites(&tree, 5, &HashMap::new()),
            Ok(vec![CellCoord::new(10, 0, 0)]),
            "5 spent, so 10 more is the limit",
        );
    }

    /// A sink with a budget below the limit — a cell that passes its
    /// strength on — takes a repeater close enough to arrive within it,
    /// where a sink without one takes none.
    #[test]
    fn a_sink_with_a_budget_takes_a_repeater_within_it() {
        let tree = NetTree::from_paths(&[&walk((0, 0, 0), &[(EAST, 10)])]);
        assert_eq!(repeater_sites(&tree, 0, &HashMap::new()), Ok(Vec::new()));
        let budgets = HashMap::from([(CellCoord::new(10, 0, 0), 4)]);
        assert_eq!(
            repeater_sites(&tree, 0, &budgets),
            Ok(vec![CellCoord::new(9, 0, 0)]),
            "10 blocks is under the limit and over the sink's 4; the repeater stands on the last straight coord",
        );
    }

    /// A budget no coord near enough can meet is refused, and the
    /// refusal says so — not that every coord turns, because two of
    /// them run straight; they are just too far back.
    #[test]
    fn a_budget_no_straight_coord_can_meet_is_refused() {
        // 3 east then 1 south: the coord before the sink is a corner,
        // and the straight one before that is 2 blocks out.
        let path = walk((0, 0, 0), &[(EAST, 3), (SOUTH, 1)]);
        let tree = NetTree::from_paths(&[&path]);
        let sink = CellCoord::new(3, 0, 1);
        let budgets = HashMap::from([(sink, 1)]);
        let stretch = repeater_sites(&tree, 0, &budgets);
        assert_eq!(
            stretch,
            Err(NoRepeaterSite {
                from: CellCoord::new(0, 0, 0),
                spent: 0,
                unpowered: sink,
                allowance: Allowance::Budget(1),
                blocked: Blocked::NoneCloseEnough,
            }),
        );

        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(6, 3, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        let entry = ScopedPlacementIrEntry {
            kind: ScopeKind::Struct,
            name: "near".to_owned(),
            ir: ir.clone(),
        };
        let region = ir.region.clone().expect("set above");
        let diagnostic = no_repeater_site_diagnostic(
            &ir,
            &entry,
            "routed",
            &region,
            NetRef::Input(0),
            stretch.expect_err("refused above"),
        );
        assert_eq!(
            diagnostic.primary,
            "routed netlist for struct `near` routes sig.a so that the signal leaving (0,0,0) reaches (3,0,1) over more than the 1 block of dust it can spare — (3,0,1) passes on the strength it receives rather than restoring it, and the wire past it needs the rest: every coord within 1 block before it turns, climbs or branches — a buffer repeater carries a signal only where the wire runs straight through it at one height, and one further back would leave too little strength to get there",
        );
        assert_eq!(
            diagnostic.notes[0].message,
            "Fix: leave the wire into (3,0,1) room to run straight within 1 block of it, or shorten the wire out of it so it can spare more — a larger `region=` is one way to do either — or drive (3,0,1) from a cell that restores strength, or split the logic across several `circuit` blocks",
        );
    }

    /// A cell that passes strength on, whose own wire out cannot reach
    /// its sink even from one block spent, is refused on its own net —
    /// the wire out is what has no room, and the wire in could not help
    /// however short it were.
    ///
    /// A Java comparator AND at `(0,0,0)`, 1 block from its sensor,
    /// drives a gate up a staircase, every coord of which turns. At 16
    /// blocks that is one too many for dust from a fresh source, so no
    /// budget in `1..=15` is met. At 15 it is the budget-zero case: it
    /// would be met from nothing spent, which no cell ever is, so it is
    /// refused on the comparator's net too, rather than handed a budget
    /// of zero that sends the refusal upstream to a wire that cannot
    /// help.
    #[test]
    fn a_cell_whose_own_wire_has_no_room_is_refused_on_its_own_net() {
        let cell = CellCoord::new(0, 0, 0);
        for steps in [16, 15] {
            let stairs: Vec<_> = (0..steps)
                .map(|step| (if step % 2 == 0 { EAST } else { SOUTH }, 1))
                .collect();
            let out = walk((0, 0, 0), &stairs);
            let sink = *out.last().expect("a staircase has an end");
            let into = walk((0, 0, 1), &[((0, 0, -1), 1)]);
            let trees = HashMap::from([
                (NetRef::Input(0), NetTree::from_paths(&[&into])),
                (NetRef::Cell(0), NetTree::from_paths(&[&out])),
            ]);
            let mut ir = PlacementIr::new(Edition::Java);
            ir.region = Some(reservation(20, 20, 1));
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
                span: Span::default(),
            });
            ir.cells.push(placed_cell(
                EditionCell::JavaComparatorAnd,
                cell,
                vec![CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                }],
            ));
            ir.cells.push(placed_cell(
                EditionCell::JavaRepeaterOr,
                sink,
                vec![CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Cell(0),
                }],
            ));
            let entry = ScopedPlacementIrEntry {
                kind: ScopeKind::Struct,
                name: "cramped".to_owned(),
                ir: ir.clone(),
            };
            let region = ir.region.clone().expect("set above");
            let Err(refusal) = repeater_sites_of_scope(&ir, &entry, "routed", &region, &trees)
            else {
                panic!("{steps}-step staircase: the comparator's own wire has no room");
            };
            assert_eq!(refusal.code, DiagnosticCode::AttenuationLimit);
            assert!(
                refusal.primary.starts_with(
                    "routed netlist for struct `cramped` routes cell #0 so that the signal leaving (0,0,0), with 1 block of dust already spent, runs out before",
                ),
                "{steps}-step staircase: names the comparator's own net, got {:?}",
                refusal.primary,
            );
            assert!(
                refusal.primary.ends_with(
                    "every coord between the two turns, climbs or branches — a buffer repeater carries a signal only where the wire runs straight through it at one height, so none can stand there",
                ),
                "{steps}-step staircase: {:?}",
                refusal.primary,
            );
        }
    }

    /// A cell that passes strength on, whose own wire out has nowhere
    /// for a repeater, sends its repeater upstream onto the wire into
    /// it — and a cell that restores strength needs none at all.
    ///
    /// The sensor is 10 blocks from the cell in a straight line, and
    /// the cell 8 from its actuator up a staircase, every coord of
    /// which turns. Measured one net at a time neither needs a
    /// repeater. Through a Bedrock OR the two are one 18-block run of dust,
    /// and the only coords a repeater can carry it through are on the
    /// sensor's straight run: the staircase can take 7 blocks already
    /// spent, so the repeater stands 1 block before the cell.
    ///
    /// Trees grown by hand, so both legs are pinned coord by coord.
    #[test]
    fn a_cell_that_passes_strength_on_sends_its_repeater_upstream() {
        let cell = CellCoord::new(10, 0, 0);
        let pad = CellCoord::new(14, 0, 4);
        let mut stairs = Vec::new();
        for _ in 0..4 {
            stairs.push((EAST, 1));
            stairs.push((SOUTH, 1));
        }
        let into = walk((0, 0, 0), &[(EAST, 10)]);
        let out = walk((10, 0, 0), &stairs);
        assert_eq!(out.last(), Some(&pad));
        let trees = HashMap::from([
            (NetRef::Input(0), NetTree::from_paths(&[&into])),
            (NetRef::Cell(0), NetTree::from_paths(&[&out])),
        ]);
        for (kind, expected) in [
            (EditionCell::BedrockTorchOr, vec![CellCoord::new(9, 0, 0)]),
            (EditionCell::BedrockInverterTorch, Vec::new()),
        ] {
            let mut ir = PlacementIr::new(Edition::Bedrock);
            ir.region = Some(reservation(20, 6, 1));
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
                span: Span::default(),
            });
            ir.cells.push(placed_cell(
                kind,
                cell,
                vec![CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                }],
            ));
            ir.outputs.push(routed_output(NetRef::Cell(0), pad));
            let entry = ScopedPlacementIrEntry {
                kind: ScopeKind::Struct,
                name: "upstream".to_owned(),
                ir: ir.clone(),
            };
            let region = ir.region.clone().expect("set above");
            let sites = repeater_sites_of_scope(&ir, &entry, "routed", &region, &trees)
                .unwrap_or_else(|d| panic!("{kind:?}: {d:?}"));
            assert_eq!(
                sites.along(&trees, NetRef::Input(0), cell),
                expected,
                "{kind:?}: the wire into the cell",
            );
            assert_eq!(
                sites.along(&trees, NetRef::Cell(0), pad),
                Vec::new(),
                "{kind:?}: the staircase out of it",
            );
        }
    }

    /// Every cell, and whether its output is at full strength whatever
    /// reached it: the two pinned cells that pass strength on are the
    /// ones the delay pass measures across, and every `*Unpinned`
    /// placeholder answers `false` until it has a realisation.
    ///
    /// The table names every variant — `listed` below fails to compile
    /// when one is added, and the length check fails when one is left
    /// out — because `strand_invariant` reads this same method, and a
    /// wrong entry would fool it and the pass alike.
    #[test]
    fn only_the_merge_and_the_comparator_pass_strength_on() {
        const fn listed(cell: EditionCell) {
            match cell {
                EditionCell::JavaComparatorAnd
                | EditionCell::BedrockTorchAnd
                | EditionCell::JavaRepeaterOr
                | EditionCell::BedrockTorchOr
                | EditionCell::JavaInverterTorch
                | EditionCell::BedrockInverterTorch
                | EditionCell::JavaXorUnpinned
                | EditionCell::BedrockXorUnpinned
                | EditionCell::JavaNandUnpinned
                | EditionCell::BedrockNandUnpinned
                | EditionCell::JavaNorUnpinned
                | EditionCell::BedrockNorUnpinned
                | EditionCell::JavaMuxUnpinned
                | EditionCell::BedrockMuxUnpinned => {}
            }
        }
        let table = [
            (EditionCell::JavaComparatorAnd, false),
            (EditionCell::BedrockTorchOr, false),
            (EditionCell::JavaRepeaterOr, true),
            (EditionCell::BedrockTorchAnd, true),
            (EditionCell::JavaInverterTorch, true),
            (EditionCell::BedrockInverterTorch, true),
            (EditionCell::JavaXorUnpinned, false),
            (EditionCell::BedrockXorUnpinned, false),
            (EditionCell::JavaNandUnpinned, false),
            (EditionCell::BedrockNandUnpinned, false),
            (EditionCell::JavaNorUnpinned, false),
            (EditionCell::BedrockNorUnpinned, false),
            (EditionCell::JavaMuxUnpinned, false),
            (EditionCell::BedrockMuxUnpinned, false),
        ];
        let distinct: HashSet<EditionCell> = table.iter().map(|(cell, _)| *cell).collect();
        assert_eq!(distinct.len(), 14, "every variant, once");
        for (cell, regenerates) in table {
            listed(cell);
            assert_eq!(cell.regenerates(), regenerates, "{cell:?}");
        }
    }

    /// A net with a stretch past the limit on which every coord turns
    /// is refused, rather than charged for a repeater stage 4 could
    /// only put on a turn.
    #[test]
    fn a_stretch_with_nowhere_for_a_repeater_is_refused() {
        let delayed = compile_delay(&staircase(&PlacementPhase::Routed { wire_length: 0 }));
        let codes: Vec<_> = delayed.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(codes, vec![DiagnosticCode::AttenuationLimit]);
        let primary = &delayed.diagnostics[0].primary;
        assert!(
            primary.contains("routed netlist for struct `stairs` routes sig.a so that the signal leaving (0,0,0) runs out before (8,0,8)"),
            "the refusal names the net and the stretch, got {primary:?}",
        );
        let survivors: Vec<_> = delayed.scoped.scopes.iter().map(|e| &e.name).collect();
        assert_eq!(survivors, vec!["roomy"]);
    }

    #[test]
    fn cell_driver_attenuation_primary_names_cell_and_port() {
        // Cells wide-spread inside a `size=256x3` reservation, with one
        // standing on the straight line between the other two. Only
        // reachable by hand-built IR — the placement pass lays cell `i`
        // at `x = i * CELL_SPACING + 1`, two columns apart, so the span
        // from the first cell to the `k`th is `2k` and producing this
        // shape from a `.crn` would need a chain of about 130 cells.
        //
        // The straight line is 255 and the route round the blocker is
        // 257, so the cap is crossed by the detour and not by the
        // distance. That is what puts the refusal here rather than in
        // the straight-line gate `lay_nets` runs before any route is
        // laid: this fixture is the case that gate must let through,
        // and a wider region — where the distance alone is over the cap
        // — would be answered before this pass saw it.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(256, 3, 3));
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(0, 0, 0),
            vec![],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(10, 0, 0),
            vec![],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(255, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(0),
            }],
        ));
        let delayed = compile_delay(&scoped(ScopeKind::Struct, "wide", ir));
        let attenuation = delayed
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::AttenuationLimit)
            .expect("cell-driver segment past cap must fire E_ATTENUATION_LIMIT");
        assert!(
            attenuation.primary.contains("into cell #2"),
            "primary must name the failing cell index, got {:?}",
            attenuation.primary,
        );
        assert!(
            attenuation.primary.contains("port #0"),
            "primary must name the failing driver port, got {:?}",
            attenuation.primary,
        );
        assert!(
            delayed.scoped.scopes.is_empty(),
            "failed scope must be elided",
        );
    }

    /// The delay pass's *own* attenuation refusal — the one for a route
    /// that crosses the cap by going round something rather than by
    /// distance — must elide its scope without disturbing a sibling.
    ///
    /// This shape cannot be written in `.crn`: a `size=` wide enough to
    /// strand a sink is already wide enough for the straight-line gate
    /// in `lay_nets` to answer first, so the integration fixture in
    /// `tests/delay.rs` exercises the routing pass's independence
    /// rather than this one's. Hand-built is the only way in.
    #[test]
    fn a_detour_refusal_leaves_its_sibling_alone() {
        // Scope one: the detour fixture. Straight line 255, route 257,
        // so it clears the straight-line gate and fails the routed one.
        let mut detour = PlacementIr::new(Edition::Java);
        detour.region = Some(reservation(256, 3, 3));
        for x in [0, 10] {
            detour.cells.push(placed_cell(
                EditionCell::JavaComparatorAnd,
                CellCoord::new(x, 0, 0),
                vec![],
            ));
        }
        detour.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(255, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(0),
            }],
        ));

        // Scope two: roomy, and nothing in it is near the cap.
        let mut roomy = PlacementIr::new(Edition::Java);
        roomy.region = Some(reservation(16, 3, 3));
        roomy.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(1, 0, 0),
            vec![],
        ));
        roomy.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(3, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(0),
            }],
        ));

        let mut input = scoped(ScopeKind::Struct, "detour", detour);
        input.scopes.push(ScopedPlacementIrEntry {
            kind: ScopeKind::Struct,
            name: "roomy".to_owned(),
            ir: roomy,
        });

        let delayed = compile_delay(&input);

        let codes: Vec<_> = delayed.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(
            codes,
            vec![DiagnosticCode::AttenuationLimit],
            "only the detour scope is refused, and only once: {:?}",
            delayed.diagnostics,
        );
        let refusal = &delayed.diagnostics[0];
        assert!(
            refusal.primary.contains("struct `detour`"),
            "the refusal must name the scope it belongs to, got {:?}",
            refusal.primary,
        );
        // Which of the two checks answered. Both say "routed netlist"
        // from this pass and both carry `E_ATTENUATION_LIMIT`, so the
        // noun and the code tell them apart from nothing — the wording
        // of the measurement is the only thing that does. Asserting the
        // gate's phrase is absent as well, because a fixture that
        // drifted into tripping the straight-line gate would satisfy
        // every other line here while testing the wrong pass.
        assert!(
            refusal.primary.contains("has a driver segment of")
                && !refusal.primary.contains("in a straight line"),
            "this is the routed-length check, not the straight-line gate, got {:?}",
            refusal.primary,
        );

        let survivors: Vec<_> = delayed.scoped.scopes.iter().map(|e| &e.name).collect();
        assert_eq!(
            survivors,
            vec!["roomy"],
            "the sibling survives the detour scope's refusal",
        );
        assert!(
            delayed.scoped.scopes[0]
                .ir
                .cells
                .iter()
                .all(|c| c.local_delay_ticks().is_some()),
            "and is delayed in full rather than half-written",
        );
    }

    #[test]
    fn missing_region_with_cells_fires_no_circuit_region() {
        // A hand-built `PlacementIr` with cells but no region reaches
        // delay because it skipped placement. The pass refuses
        // instead of silently passing through with a `(None, None)`
        // phase-table violation.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(0, 0, 0),
            vec![],
        ));
        let delayed = compile_delay(&scoped(ScopeKind::Struct, "roomless", ir));
        let diag = delayed
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::NoCircuitRegion)
            .expect("missing region with cells must fire E_NO_CIRCUIT_REGION");
        assert!(
            diag.primary.contains("struct `roomless`"),
            "primary must name the scope, got {:?}",
            diag.primary,
        );
        assert!(
            delayed.scoped.scopes.is_empty(),
            "failed scope must elide even though it carried cells",
        );
    }

    #[test]
    fn buffer_repeater_ticks_boundary_table() {
        // Boundary values of the piecewise formula
        // `s <= 15 → 0`, `s in (15, 30] → 1 * BUFFER_REPEATER_TICKS`,
        // `s in (30, 45] → 2 * BUFFER_REPEATER_TICKS`, ... pinned as a
        // table so a `(s - 1) / 15` → `s / 15` slip trips each row
        // rather than the aggregate.
        for (segment, expected_buffers) in [
            (0_u32, 0_u32),
            (1, 0),
            (DUST_ATTENUATION_LIMIT, 0),
            (DUST_ATTENUATION_LIMIT + 1, 1),
            (2 * DUST_ATTENUATION_LIMIT, 1),
            (2 * DUST_ATTENUATION_LIMIT + 1, 2),
            (3 * DUST_ATTENUATION_LIMIT, 2),
            (3 * DUST_ATTENUATION_LIMIT + 1, 3),
            (MAX_ATTENUATION_SEGMENT, 17),
        ] {
            assert_eq!(
                buffer_repeater_ticks_for_segment(segment),
                expected_buffers * BUFFER_REPEATER_TICKS,
                "segment {segment} blocks",
            );
        }
    }

    #[test]
    #[should_panic(expected = "topological invariant broken")]
    fn out_of_range_net_ref_cell_panics_loudly() {
        // A hand-built IR with `NetRef::Cell(u32::MAX)` violates the
        // synthesis-side topological invariant. The release-mode
        // panic turns a silent last-cell fallback into a loud
        // failure, so a caller-side bug cannot produce silently wrong
        // `local_delay_ticks`.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 3, 2));
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(0, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(u32::MAX),
            }],
        ));
        let _ = compile_delay(&scoped(ScopeKind::Struct, "broken", ir));
    }

    #[test]
    #[should_panic(
        expected = "for cell #1 at (4,0,1) in struct `mixed` — delay insertion must run exactly once per routed IR"
    )]
    fn delay_panic_names_the_offending_cell_not_the_first_one() {
        // Re-running the whole pass always trips on `cells[0]`, which
        // would let a regression that hardcoded the index to zero — or
        // that read the coord off the wrong cell — pass unnoticed. A
        // hand-built IR whose first cell is still `Routed` while the
        // second is already `Delayed` forces the panic past the head
        // of the loop, so both the index and the coord have to be
        // threaded from the cell actually being transitioned.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(8, 3, 2));
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(0, 0, 0),
            vec![],
        ));
        let mut already_delayed = placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(4, 0, 1),
            vec![],
        );
        already_delayed.phase = PlacementPhase::Delayed {
            wire_length: 0,
            local_delay_ticks: 0,
        };
        ir.cells.push(already_delayed);
        let _ = compile_delay(&scoped(ScopeKind::Struct, "mixed", ir));
    }
}
