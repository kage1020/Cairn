//! Delayed Placement IR → legalized Placement IR lowering (crossing
//! legalization).
//!
//! Stage 4 of the five-stage place-and-route pipeline `spec/redstone`
//! §14.5 lays out (Placement → Steiner routing → Delay insertion →
//! Crossing legalization → Edition legalization). Walks every
//! [`crate::placement_ir::PlacedCellNode`] in each scope's delayed
//! Placement IR, rebuilds every net's Steiner tree through
//! `net_trees` — the same call the routing
//! and delay passes make, because the routing pass discards its
//! per-scope occupancy set before yielding the routed IR and storing
//! wire coords in the shared IR would bloat every JSON dump for every
//! consumer — and carries out two tasks:
//!
//! 1. **Wire crossing detection.** A wire coord (neither cell nor
//!    pad) that ends up owned by two distinct nets is a "crossing" —
//!    one strand of dust carrying two signals, which shorts them in
//!    the Minecraft voxel model. v1 does not lift wire coords
//!    themselves onto a `Bridge` layer: the routed wire path is not
//!    stored in the IR, so an escape record would have nowhere to
//!    attach, and nothing downstream reads the crossing set. So the
//!    pass reports every crossing and repairs none of them.
//!
//!    What the reservation's height decides is not whether the
//!    signals merge — they merge either way — but whether the author
//!    can do anything about it without changing `void=`. Under
//!    `void < 2` there is no layer above the plane at all, so the
//!    scope is refused with
//!    [`crate::DiagnosticCode::CrossingCongestion`] and its
//!    `Fix: increase void` is true. With a layer reserved, the design
//!    is buildable by a pass that does not exist yet, and each
//!    crossing is raised as a
//!    [`crate::DiagnosticCode::WireCrossing`] warning that names the
//!    two nets and the coord while the scope goes on through. Note
//!    that "plane" is the wrong word for the general case: the
//!    routing pass climbs onto a bridge layer to get past a block,
//!    and two nets that both climbed can meet up there —
//!    `examples/crossbar.crn` does exactly that.
//!
//!    One finding per unordered pair of nets, not per shared coord:
//!    two nets side by side for five coords are one defect. What
//!    steers buffer placement is the `wire_owners` map the crossings
//!    are folded out of: a candidate is unusable when *any* net other
//!    than its own runs through the coord, whether or not a second
//!    one makes it a crossing.
//! 2. **Implicit buffer repeater coord assignment.** The delay pass
//!    counted `floor((s - 1) / DUST_ATTENUATION_LIMIT)` buffer
//!    repeaters per driver segment of length `s` and folded their tick
//!    contribution into `delay_ticks`; this pass materialises the
//!    concrete coord of each one into
//!    [`crate::placement_ir::PlacedCellNode::buffer_coords`].
//!
//!    A repeater refreshes the dust it stands on, so each one is
//!    picked off
//!    `route_to` — the routed path
//!    from the net's source to *this* sink — at
//!    `k * DUST_ATTENUATION_LIMIT` (`k = 1..=buffer_count`), and the
//!    count comes from that same path's length through
//!    `buffer_count_for_segment`, the function the
//!    delay pass charged ticks for. Walking the straight line between
//!    the two instead is what used to put buffers on coords the net
//!    does not own: the routed path hangs off the trunk laid for a
//!    nearer sink and goes round whatever stands in its way, so a
//!    repeater on the straight line stands either in mid-air or on a
//!    neighbouring net's dust.
//!
//!    A candidate whose coord is a cell body, an I/O pad or another
//!    net's dust escapes to the first free y-layer inside the
//!    `void=<N>` budget on
//!    [`crate::placement_ir::RouteLayer::Bridge`]; if every layer is
//!    taken (or `void < 2`), the pass refuses with
//!    [`crate::DiagnosticCode::BufferCoordCollision`], naming what
//!    holds the coord.
//!
//!    A repeater this net already stands on the candidate — or lifted
//!    off it — is not an obstruction: the sinks of one net share their
//!    prefix, so segments of one net reach the same refresh
//!    points, and the block already there refreshes every one of
//!    them. The segment records that coord instead of asking for a
//!    layer of its own. Only *distinct* nets can therefore contend for
//!    a bridge column — one net never asks twice — which is what the
//!    refusal above is left for.
//!
//! [`crate::placement_ir::RouteLayer::Via`] has no producer in v1: the
//! bridge escape is a single coord, not a segment with distinct
//! plane / bridge endpoints, so there is no ramp to name. The variant
//! is kept in the enum for exhaustive matches against §14.5's full
//! vocabulary.
//!
//! Failed scopes are elided from the output so a `stage 5` consumer
//! never reads a partially-populated `buffer_coords` — the same
//! fail-loud policy the routing and delay passes use.
//!
//! The crossing pass is one
//! [`crate::placement_ir::PlacementPhase::legalize`] transition per
//! cell, per the producer↔variant table on that enum, carrying the
//! buffer coords it allocated (each stamped with a
//! [`crate::placement_ir::CellCoord::layer`]); no new IR type is
//! introduced, and
//! [`crate::placement_ir::PlacedCellNode::buffer_coords`] is the
//! read-only projection of the resulting variant. Both the layer and
//! the coord vector
//! serde-skip on their defaults, so a scope whose crossing pass
//! writes nothing dumps as the JSON its delay-pass input did apart
//! from the `stage` tag — which is exactly why that tag exists: it
//! is the only thing telling a consumer this pass ran at all when
//! there was nothing to legalize (see
//! [`crate::placement_ir::PlacementStage`]).

use std::collections::HashMap;
use std::collections::HashSet;

use cairn_lang_core::check::Severity;

use crate::delay::{DUST_ATTENUATION_LIMIT, buffer_count_for_segment};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    BufferCoord, BufferSegment, CellCoord, CellIdentity, CircuitRegionReservation, PlacementIr,
    ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{
    NetTree, Router, block_sites, collect_nets, input_pad, net_order, net_ref_key, net_trees,
    unroutable,
};

/// Output of a [`compile_crossing`] run.
///
/// Mirrors [`crate::delay::DelayOutput`]'s shape so callers see a
/// uniform result type across every stage of the place-and-route
/// pipeline. The legalized IR is a [`ScopedPlacementIr`] with every
/// non-failed scope's `buffer_coords` populated with one entry per
/// implicit buffer repeater the delay pass counted, each tagged with
/// the [`RouteLayer`] the pass chose. No new IR type; the crossing
/// pass is one [`crate::placement_ir::PlacementPhase::legalize`]
/// transition per cell, per the producer↔variant table on that enum.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct CrossingOutput {
    /// Placement IR for every scope whose crossing legalization
    /// succeeded, with every cell's `buffer_coords` populated to match
    /// the buffer tick contribution the delay pass folded into
    /// `delay_ticks`.
    pub scoped: ScopedPlacementIr,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

impl CrossingOutput {
    /// Empty output (no legalized scopes, no diagnostics).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lower a delayed [`ScopedPlacementIr`] into a legalized
/// [`ScopedPlacementIr`].
///
/// Reads every cell's coord and the scope's
/// [`CircuitRegionReservation`] out of the input IR — the Placement
/// IR is self-describing by construction, so the crossing pass has no
/// `IntentModule` dependency.
///
/// One entry per non-empty [`PlacementIr`] whose legalization
/// succeeded; scopes that raise an Error-severity diagnostic
/// ([`DiagnosticCode::CrossingCongestion`] /
/// [`DiagnosticCode::BufferCoordCollision`] /
/// [`DiagnosticCode::NoCircuitRegion`]) are elided from the output so a
/// partial `buffer_coords` set cannot pollute the downstream
/// block-array voxel lowering. A scope can also survive carrying
/// [`DiagnosticCode::WireCrossing`] warnings: those describe a layout
/// the pass produced faithfully and cannot make correct, so the IR is
/// kept and the finding travels beside it.
#[must_use]
pub fn compile_crossing(delayed: &ScopedPlacementIr) -> CrossingOutput {
    let mut out = CrossingOutput::new();
    for entry in &delayed.scopes {
        match legalize_scope(entry) {
            Ok((ir, warnings)) => {
                out.diagnostics.extend(warnings);
                out.scoped.scopes.push(ScopedPlacementIrEntry {
                    kind: entry.kind,
                    name: entry.name.clone(),
                    ir,
                });
            }
            Err(diagnostic) => out.diagnostics.push(diagnostic),
        }
    }
    out
}

/// Result of legalizing one scope: on success the legalized IR
/// together with the Warning-severity findings the scope earned but
/// survived, on failure the single Error-severity diagnostic that
/// elides it. Warnings ride out with the IR rather than replacing it
/// because a crossing the reservation has room to lift does not stop
/// the scope from being legalized — it stops the layout from being
/// what the coords imply, which is a thing to say, not a thing to
/// refuse.
type ScopeLegalization = Result<(PlacementIr, Vec<Diagnostic>), Diagnostic>;

fn legalize_scope(entry: &ScopedPlacementIrEntry) -> ScopeLegalization {
    let source = &entry.ir;
    // Same missing-region policy as `delay::compile_delay`: the
    // placement pass elides scopes with cells or output drivers but no
    // region, so a hand-built IR reaching here in that shape is a
    // caller-side bug — refuse loud with `E_NO_CIRCUIT_REGION` so
    // downstream consumers see a consistent taxonomy. Scopes with
    // neither cells nor outputs still pass through so a module
    // without any redstone survives the crossing pipeline as-is.
    let Some(region) = source.region.clone() else {
        if source.cells.is_empty() && source.outputs.is_empty() {
            return Ok((source.clone(), Vec::new()));
        }
        return Err(missing_region_diagnostic(entry));
    };
    if source.cells.is_empty() && source.outputs.is_empty() {
        return Ok((source.clone(), Vec::new()));
    }

    let mut ir = source.clone();
    let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();

    // Netlist synthesis guarantees the topological invariant
    // (`NetRef::Cell(j)` inside `cells[i]` satisfies `j < i`), so
    // panicking loud on an out-of-range access beats silently
    // sinking into a fall-back coord. Same reasoning as the delay
    // pass's stricter branch — this pass writes `buffer_coords`, and
    // the producer↔variant table on `PlacementPhase` promises populated
    // `buffer_coords` after stage 4, so a silent under-population
    // would let the downstream voxel lowering read a stage-3 shape
    // from a stage-4 output.
    let source_of_net = |net: NetRef| -> CellCoord {
        match net {
            NetRef::Input(i) => input_pad(i as usize, &region),
            NetRef::Cell(j) => *cell_coords.get(j as usize).unwrap_or_else(|| {
                panic!(
                    "NetRef::Cell({j}) out of range (cells.len()={}) — topological invariant broken by caller-side hand-built IR",
                    cell_coords.len(),
                )
            }),
        }
    };

    // Reserved coords are cell bodies and I/O pads — they carry the
    // net's endpoint semantic, not a wire pass-through, so two nets
    // touching the same reserved coord is not a crossing (that is
    // the standard cell → downstream cell chain). Only pure wire
    // coords participate in crossing detection. The same list is what
    // the router routed around, so a wire coord here is a coord the
    // router was free to use.
    let blocks = block_sites(&ir, &region);
    let reserved: HashSet<CellCoord> = blocks.iter().map(|site| site.coord).collect();

    let nets = collect_nets(&ir);
    let order = net_order(&nets);
    let router = Router::new(&region, &blocks);
    let trees = net_trees(&nets, &router, source_of_net);
    // Same re-check the delay pass makes, for the same reason: a
    // stranded sink's route is one step, so it asks for no repeater and
    // the legalized dump would claim a circuit that cannot be wired.
    if let Some(diagnostic) = unroutable(&nets, &trees, entry, &region, source_of_net) {
        return Err(diagnostic);
    }

    // Which nets' dust runs through each non-reserved coord, recorded
    // in `order` so the list at each coord is deterministic. Not the
    // plane alone: the router climbs onto a bridge layer to get past a
    // block, and a coord it climbed onto is dust like any other — which
    // is what `claim_bridge` reads to keep a repeater off it.
    // One map answers both questions this pass asks about the routed
    // wires: which coords two nets share (a wire crossing), and
    // whether a buffer candidate would land on dust that is not its
    // own.
    let mut wire_owners: HashMap<CellCoord, Vec<NetRef>> = HashMap::new();
    for net in &order {
        for coord in trees[net].wire_path() {
            if reserved.contains(&coord) {
                continue;
            }
            let owners = wire_owners.entry(coord).or_default();
            if !owners.contains(net) {
                owners.push(*net);
            }
        }
    }

    // Two nets on one coord is one strand of dust carrying two
    // signals, whatever the reservation's height. What the height
    // decides is only whether anything could ever be done about it:
    // under `void < 2` there is no layer above the plane at all, so
    // the author has to raise `void=` before any pass could lift
    // either net, and the scope is refused. With a layer reserved the
    // design is buildable and simply is not built — v1 lifts buffer
    // repeaters and never wire — so each crossing is reported and the
    // scope survives.
    let crossings = wire_crossings(&wire_owners);
    if region.void < 2
        && let Some(anchor) = crossings.first()
    {
        return Err(crossing_congestion_diagnostic(
            entry,
            &ir,
            &region,
            anchor,
            crossings.len(),
        ));
    }
    // Nothing is left to warn about under `void < 2`: the refusal
    // above took every crossing with it, so this maps over an empty
    // list rather than re-asking the height.
    let warnings: Vec<Diagnostic> = crossings
        .iter()
        .map(|crossing| wire_crossing_diagnostic(entry, &ir, &region, crossing))
        .collect();

    let allocation = allocate_buffer_coords(
        &ir,
        entry,
        &region,
        &cell_coords,
        &trees,
        &wire_owners,
        &reserved,
    )?;

    for (index, (cell, buffers)) in ir.cells.iter_mut().zip(allocation.per_cell).enumerate() {
        // Loud in release too: `PlacementPhase::legalize_at` panics on
        // any non-`Delayed` variant, so a caller who chained
        // `compile_crossing(&legalized.scoped)` (or handed us a
        // still-`Unrouted` / `Routed` cell) trips a release-panic here
        // rather than silently producing a stale-but-plausible IR. The
        // identity rides along so that panic names the offending cell
        // rather than only the phase it tripped on.
        let identity = CellIdentity::new(index, cell.coord, entry);
        cell.phase.legalize_at(buffers, identity);
    }

    for (index, (output, buffers)) in ir.outputs.iter_mut().zip(allocation.per_output).enumerate() {
        let identity = CellIdentity::output(index, output.pad, entry);
        output.phase.legalize_at(buffers, identity);
    }

    Ok((ir, warnings))
}

/// Two nets that share at least one wire coord.
///
/// One value per unordered pair rather than one per shared coord: two
/// nets running side by side for five coords are one defect — those
/// two signals are one wire — not five, and a reader who is told five
/// times learns nothing the first telling did not carry.
#[derive(Debug, Clone, Copy)]
struct WireCrossing {
    /// The pair, `nets.0` first under [`net_ref_key`] — inputs before
    /// cells, then by index.
    nets: (NetRef, NetRef),
    /// Lowest of the coords the two share, in `(x, z, y)` order — the
    /// same key the rest of this pass anchors on, so the coord a
    /// message names does not drift between runs of one layout.
    anchor: CellCoord,
    /// How many coords the two share in all.
    coords: usize,
}

/// Fold the per-coord owner lists into one entry per crossing pair.
///
/// Each pair is put in [`net_ref_key`] order before it is folded, so
/// two coords shared by the same two nets meet in one entry and the
/// order a message names them in does not depend on which of them the
/// pass happened to route first. `wire_owners` lists a coord's owners
/// in [`net_order`], which leads with fanout — an ordering this pass
/// uses to walk work, not one to read a finding in.
///
/// Everything else about the result is a fold over a `HashMap`, so the
/// sort at the end is what makes the list an answer rather than an
/// iteration order. It is a total order because a pair appears once.
fn wire_crossings(wire_owners: &HashMap<CellCoord, Vec<NetRef>>) -> Vec<WireCrossing> {
    let mut folded: HashMap<(NetRef, NetRef), (CellCoord, usize)> = HashMap::new();
    for (coord, owners) in wire_owners {
        for (index, one) in owners.iter().enumerate() {
            for other in &owners[index + 1..] {
                let pair = if net_ref_key(*one) <= net_ref_key(*other) {
                    (*one, *other)
                } else {
                    (*other, *one)
                };
                let entry = folded.entry(pair).or_insert((*coord, 0usize));
                if (coord.x, coord.z, coord.y) < (entry.0.x, entry.0.z, entry.0.y) {
                    entry.0 = *coord;
                }
                entry.1 += 1;
            }
        }
    }
    let mut crossings: Vec<WireCrossing> = folded
        .into_iter()
        .map(|(nets, (anchor, coords))| WireCrossing {
            nets,
            anchor,
            coords,
        })
        .collect();
    crossings.sort_unstable_by_key(|crossing| {
        (net_ref_key(crossing.nets.0), net_ref_key(crossing.nets.1))
    });
    crossings
}

/// Why a buffer candidate cannot take its coord on the plane.
#[derive(Debug, Clone, Copy)]
enum PlaneOccupant {
    /// A cell body or an I/O pad. A repeater there would displace the
    /// component instead of refreshing the wire running into it.
    ///
    /// Answered by this function but not reachable through
    /// [`legalize_scope`], for the same shape of reason
    /// [`Self::Buffer`] is not: a candidate sits strictly between the
    /// ends of a route, and the router keeps every coord strictly
    /// between them off the blocks. The variant stays because it is
    /// the honest answer for the inputs, and because it is the first
    /// question worth asking about a coord — see
    /// `a_buffer_candidate_is_never_a_cell_body`.
    Component,
    /// Another net's dust runs through the coord. A repeater there
    /// would tie two signals together — the cross-net short this pass
    /// exists to keep out of the layout.
    OtherNet(NetRef),
    /// A buffer an earlier driver already claimed for another net.
    ///
    /// Answered by this function but not reachable through
    /// [`legalize_scope`], which asks about candidates only: a
    /// repeater always stands on a coord of its own net's
    /// [`NetTree::route_to`], every such coord is one
    /// [`NetTree::wire_path`] reports, and a coord two nets' wire runs
    /// through answers [`Self::OtherNet`] first. The variant stays
    /// because the answer is the honest one for the inputs, and a
    /// future producer that places a repeater beside its wire rather
    /// than on it would need it.
    Buffer,
    /// A buffer this same net already claimed. Not an obstruction: the
    /// segments past it are fed by the one repeater standing there, so
    /// a second one would be a second block on a strand of dust that
    /// has one.
    OwnBuffer,
}

/// What holds `candidate` on the plane, or `None` when it is free for
/// `net` to put a repeater on.
///
/// The order is the order the reasons matter in: a component is not
/// wire at all, a foreign net makes the coord electrically wrong
/// rather than merely occupied, and an earlier buffer is this pass's
/// own doing. Only the first is reported, because only one thing has
/// to be true for the candidate to be unusable.
///
/// Of the four, two are reachable through the pass today —
/// [`PlaneOccupant::OtherNet`] and [`PlaneOccupant::OwnBuffer`]. The
/// other two are answers this function owes its inputs rather than
/// states a candidate reaches; each says so on its own variant.
fn plane_occupant(
    candidate: CellCoord,
    net: NetRef,
    reserved: &HashSet<CellCoord>,
    wire_owners: &HashMap<CellCoord, Vec<NetRef>>,
    plane_buffers: &HashMap<CellCoord, NetRef>,
) -> Option<PlaneOccupant> {
    if reserved.contains(&candidate) {
        return Some(PlaneOccupant::Component);
    }
    if let Some(other) = wire_owners
        .get(&candidate)
        .and_then(|owners| owners.iter().find(|owner| **owner != net))
    {
        return Some(PlaneOccupant::OtherNet(*other));
    }
    if let Some(owner) = plane_buffers.get(&candidate) {
        return Some(if *owner == net {
            PlaneOccupant::OwnBuffer
        } else {
            PlaneOccupant::Buffer
        });
    }
    None
}

/// Claim the first free bridge layer in `candidate`'s column, inside
/// the reservation's `void=<N>` budget. `None` when every layer is
/// taken, and when `void < 2` leaves no layer above the plane at all.
///
/// The scan runs `y in 1..void` rather than upward from `candidate.y`,
/// so a candidate that is itself on a bridge layer is offered the
/// layers below it too. Every candidate the pass builds is a plane
/// coord today, which makes the two the same scan.
///
/// A layer is taken by an earlier repeater — `bridge_buffers` is that
/// ledger — or by dust: the router climbs off the plane to get past a
/// block, and the coord it climbs onto is a wire like any other.
/// `CellCoord::new` gives both producers the same key, so a
/// repeater cannot be dropped onto a wire that was already there.
fn claim_bridge(
    candidate: CellCoord,
    region: &CircuitRegionReservation,
    bridge_buffers: &mut HashSet<CellCoord>,
    wire_owners: &HashMap<CellCoord, Vec<NetRef>>,
) -> Option<CellCoord> {
    for y in 1..region.void {
        let bridge = CellCoord::new(candidate.x, y, candidate.z);
        if wire_owners.contains_key(&bridge) {
            continue;
        }
        if bridge_buffers.insert(bridge) {
            return Some(bridge);
        }
    }
    None
}

/// Buffer coord allocation for one scope: every cell driver segment,
/// then every actuator segment, through one [`BufferPlacer`].
///
/// The routed path is [`NetTree::route_to`] and the count is
/// [`buffer_count_for_segment`] over its length — the same function the
/// delay pass charged ticks with — so stage 3 and stage 4 describe one
/// circuit. Split out of `legalize_scope` so the entry function stays
/// under clippy's `too_many_lines` budget and the allocation strategy
/// reads as a self-contained table.
fn allocate_buffer_coords(
    ir: &PlacementIr,
    entry: &ScopedPlacementIrEntry,
    region: &CircuitRegionReservation,
    cell_coords: &[CellCoord],
    trees: &HashMap<NetRef, NetTree>,
    wire_owners: &HashMap<CellCoord, Vec<NetRef>>,
    reserved: &HashSet<CellCoord>,
) -> Result<BufferAllocation, Diagnostic> {
    let mut placer = BufferPlacer {
        region,
        wire_owners,
        reserved,
        plane: HashMap::new(),
        bridge: HashSet::new(),
        escaped: HashMap::new(),
    };

    let mut per_cell: Vec<Vec<BufferCoord>> = Vec::with_capacity(ir.cells.len());
    for (cell_index, cell) in ir.cells.iter().enumerate() {
        let sink = cell_coords[cell_index];
        let mut buffers_for_cell: Vec<BufferCoord> = Vec::new();
        for driver in &cell.drivers {
            // `route_to` answers `None` only for a sink that is not a
            // terminal of the net, which `collect_nets` makes
            // unreachable — it built the tree's terminal list out of
            // this very driver list. Loud rather than silent for the
            // same reason `source_of_net` is: the alternative is a
            // cell whose `buffer_coords` under-populate against the
            // ticks stage 3 already charged.
            let route = trees
                .get(&driver.net)
                .and_then(|tree| tree.route_to(sink))
                .unwrap_or_else(|| {
                    panic!(
                        "cell #{cell_index} at ({x},{y},{z}) is not a terminal of the net driving its port — the driver list and the collected nets disagree",
                        x = sink.x,
                        y = sink.y,
                        z = sink.z,
                    )
                });
            buffers_for_cell.extend(placer.claim(
                &route,
                driver.net,
                BufferSegment::Port(driver.port),
                &|candidate, occupant| {
                    buffer_collision_diagnostic(entry, ir, region, cell_index, candidate, occupant)
                },
                &format!("cell #{cell_index} driver"),
            )?);
        }
        // Producer-side contract on [`BufferCoord::port`]: every entry
        // must name a driver that actually exists on the owning cell.
        // Trivially true today because the push site sources
        // `driver.port` from the enclosing `for driver in &cell.drivers`
        // loop, but debug-asserted so a future buffer producer (e.g.
        // fan-out duplication) added elsewhere cannot silently emit a
        // `BufferCoord` whose `port` does not match any driver — the
        // downstream voxel lowering would then group buffers under a
        // driver that does not exist.
        debug_assert!(
            buffers_for_cell.iter().all(|b| matches!(
                b.port,
                BufferSegment::Port(port) if cell.drivers.iter().any(|d| d.port == port)
            )),
            "BufferCoord::port must reference a driver on cells[{cell_index}]",
        );
        per_cell.push(buffers_for_cell);
    }

    // The segment out to an actuator is charged for buffers by stage 3
    // exactly as a segment into a cell is, so stage 4 has to give those
    // buffers coords or the two stages disagree about how many exist.
    //
    // Same placer, and after the cells, so a cell buffer and an actuator
    // buffer cannot claim one coord. They do contend: an actuator's
    // route leaves its driver along the same trunk a cell's does, so
    // the two reach the same refresh points whenever they share a
    // prefix.
    let mut per_output: Vec<Vec<BufferCoord>> = Vec::with_capacity(ir.outputs.len());
    for (output_index, output) in ir.outputs.iter().enumerate() {
        let route = trees
            .get(&output.driver)
            .and_then(|tree| tree.route_to(output.pad))
            .unwrap_or_else(|| {
                panic!(
                    "output #{output_index} pad at ({x},{y},{z}) is not a terminal of the net driving it — the output list and the collected nets disagree",
                    x = output.pad.x,
                    y = output.pad.y,
                    z = output.pad.z,
                )
            });
        let buffers_for_output = placer.claim(
            &route,
            output.driver,
            BufferSegment::Out,
            &|candidate, occupant| {
                buffer_collision_output_diagnostic(
                    entry,
                    ir,
                    region,
                    output_index,
                    candidate,
                    occupant,
                )
            },
            &format!("output #{output_index}"),
        )?;
        // The mirror of the cell-side contract: a buffer on the wire to
        // an actuator belongs to no input port, and saying otherwise
        // would group it under a driver of a cell it is not on.
        debug_assert!(
            buffers_for_output
                .iter()
                .all(|b| matches!(b.port, BufferSegment::Out)),
            "a buffer on the wire to outputs[{output_index}] must name the outward segment",
        );
        per_output.push(buffers_for_output);
    }

    Ok(BufferAllocation {
        per_cell,
        per_output,
    })
}

/// The coords already spoken for by buffers this scope has placed, plus
/// the immutable picture of what else is in the way.
///
/// One placer serves the cell segments and the actuator segments in
/// turn, which is what keeps two buffers off one coord. Both kinds of
/// segment go through [`Self::claim`] for the same reason: stage 3
/// charges them by one rule, so stage 4 has to place them by one rule
/// or the tick count and the coord list describe different circuits.
///
/// Two of its three maps recognise a repeater this net has already
/// placed — on the plane and on a bridge layer — because a segment
/// that shares a prefix with an earlier one reaches the same refresh
/// points and must record the blocks standing there rather than ask
/// for its own. The third, [`Self::bridge`], is [`claim_bridge`]'s
/// ledger of which y-layers are spoken for, and is what keeps two
/// nets off one lifted block.
///
/// Of the two recognition maps, only [`Self::escaped`] changes an
/// answer. A plane repeater always stands on a coord of its own net's
/// route, and a coord this net's own wire runs through is free for
/// this net, so [`Self::plane`] and the free-plane arm push the same
/// entry; a foreign net reaches [`PlaneOccupant::OtherNet`] first,
/// because that repeater stands on its own net's wire too. The map
/// stays because it is what names the decision at the point the
/// decision is made, and because [`PlaneOccupant::Buffer`] is the
/// honest answer for a foreign net's repeater rather than "free".
struct BufferPlacer<'a> {
    region: &'a CircuitRegionReservation,
    wire_owners: &'a HashMap<CellCoord, Vec<NetRef>>,
    reserved: &'a HashSet<CellCoord>,
    /// Coord → the net whose repeater stands on it. Keyed by net
    /// rather than a bare set because one net's two sinks
    /// share their prefix: the candidates for both land on the
    /// same coords, and the second visit has to recognise its
    /// own repeater rather than escape around it.
    plane: HashMap<CellCoord, NetRef>,
    /// Every bridge coord [`claim_bridge`] has handed out, so the next
    /// caller over the same `(x, z)` column gets the layer above
    /// rather than the same block.
    bridge: HashSet<CellCoord>,
    /// `(net, candidate)` → the bridge coord that net's repeater was
    /// lifted onto. The plane map answers the same question for a
    /// repeater that stayed down; this one keeps a net from escaping
    /// twice off one refresh point and standing two blocks over it.
    escaped: HashMap<(NetRef, CellCoord), CellCoord>,
}

impl BufferPlacer<'_> {
    /// Claim a coord for every buffer repeater [`buffer_count_for_segment`]
    /// implies on `route`, attributing each to `port`.
    ///
    /// Candidates sit at `k * DUST_ATTENUATION_LIMIT` along the routed
    /// path — the dust the signal actually travels, so a plane buffer
    /// always stands on the wire it refreshes. A candidate held by
    /// something else ([`plane_occupant`]) escapes to the first free
    /// [`RouteLayer::Bridge`] layer in its column; if none is free,
    /// `on_collision` builds the refusal, because the two callers name
    /// the offending segment differently and "cell #3" on a finding
    /// about an actuator wire sends the reader to the wrong line.
    /// `subject` names the segment in the panic messages that guard the
    /// index invariants.
    fn claim(
        &mut self,
        route: &[CellCoord],
        net: NetRef,
        port: BufferSegment,
        on_collision: &dyn Fn(CellCoord, PlaneOccupant) -> Diagnostic,
        subject: &str,
    ) -> Result<Vec<BufferCoord>, Diagnostic> {
        let segment = u32::try_from(route.len().saturating_sub(1)).unwrap_or(u32::MAX);
        let mut claimed = Vec::new();
        for k in 1..=buffer_count_for_segment(segment) {
            // `route.len() == segment + 1` and `k * DUST_ATTENUATION_LIMIT
            // <= buffer_count * DUST_ATTENUATION_LIMIT <= segment - 1`,
            // so `idx` is always a valid index. Loud in release: a
            // silent saturating fallback would place the buffer at the
            // sink coord and let a caller-side bug (segment /
            // buffer_count / route.len() drift) materialise a buffer on
            // top of a component without any diagnostic.
            let idx = (k as usize).saturating_mul(DUST_ATTENUATION_LIMIT as usize);
            let candidate = *route.get(idx).unwrap_or_else(|| {
                panic!(
                    "buffer index {idx} out of range (route.len()={}) for {subject} — segment / buffer_count / route.len() invariant broken by caller-side hand-built IR",
                    route.len(),
                )
            });
            let occupant =
                plane_occupant(candidate, net, self.reserved, self.wire_owners, &self.plane);
            let Some(occupant) = occupant else {
                self.plane.insert(candidate, net);
                claimed.push(BufferCoord::new(port, candidate));
                continue;
            };
            // The sinks of one net share their prefix, so two
            // segments of one net compute the same candidates — two
            // ports of one cell, two cells past the same 15-block
            // point, a cell and an actuator. The repeater standing
            // there already refreshes the signal for every one of
            // them, and escaping around it puts a second block on a
            // strand of dust that has one: four stacked repeaters over
            // a fan-out to two actuators, and a refusal on the
            // `void=1` layouts that have no layer to stack into. The
            // segment records the coord it shares rather than taking
            // one of its own; `BufferCoord` is an attribution list.
            if matches!(occupant, PlaneOccupant::OwnBuffer) {
                claimed.push(BufferCoord::new(port, candidate));
                continue;
            }
            // The same rule one layer up. Another net's dust holds
            // the plane, so the first segment to reach the coord lifts
            // its repeater onto a bridge — and that block sits over the
            // coord every segment of this net passes through, so it
            // refreshes all of them. Escaping again would stack
            // a second block on the same point of the same wire, and
            // exhaust the `void=<N>` budget doing it.
            if let Some(&reused) = self.escaped.get(&(net, candidate)) {
                claimed.push(BufferCoord::new(port, reused));
                continue;
            }
            let bridge = claim_bridge(candidate, self.region, &mut self.bridge, self.wire_owners)
                .ok_or_else(|| on_collision(candidate, occupant))?;
            self.escaped.insert((net, candidate), bridge);
            claimed.push(BufferCoord::new(port, bridge));
        }
        Ok(claimed)
    }
}

/// Where every implicit buffer repeater in one scope goes: one entry
/// per cell, then one per actuator pad. Two vectors rather than one
/// because the two commit into different nodes, and a single flat list
/// would need the split re-derived at the commit site.
struct BufferAllocation {
    per_cell: Vec<Vec<BufferCoord>>,
    per_output: Vec<Vec<BufferCoord>>,
}

/// The output-segment mirror of [`buffer_collision_diagnostic`]. Kept
/// separate rather than given a cell index it does not have: "cell #3"
/// on a finding about the wire to an actuator sends the reader to the
/// wrong line.
fn buffer_collision_output_diagnostic(
    entry: &ScopedPlacementIrEntry,
    ir: &PlacementIr,
    reservation: &CircuitRegionReservation,
    output_index: usize,
    candidate: CellCoord,
    occupant: PlaneOccupant,
) -> Diagnostic {
    let held_by = plane_occupant_label(occupant, ir);
    let primary = format!(
        "routed netlist for {kind} `{name}` cannot place an implicit buffer repeater on the wire to actuator #{output_index}: its wire reaches ({x},{y},{z}), which is already {held_by}, and the `void={void}` reservation offers no bridge layer to escape to",
        kind = entry.kind.label(),
        name = entry.name,
        x = candidate.x,
        y = candidate.y,
        z = candidate.z,
        void = reservation.void,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::BufferCoordCollision,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: increase `void` so buffers can fall onto a bridge layer, or enlarge `region=` so buffer candidates have room on the plane",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

/// `{crossing_count} wire crossing(s)` counts pairs of nets, the unit
/// [`WireCrossing`] is one of, not coords: the warning half of this
/// defect counts the same way, and a reader comparing a `void=1` run
/// against a `void=2` one is comparing the same number.
fn crossing_congestion_diagnostic(
    entry: &ScopedPlacementIrEntry,
    ir: &PlacementIr,
    reservation: &CircuitRegionReservation,
    crossing: &WireCrossing,
    crossing_count: usize,
) -> Diagnostic {
    let (first, second) = crossing.nets;
    let primary = format!(
        "routed netlist for {kind} `{name}` has {crossing_count} wire crossing(s), including {first_label} vs {second_label} at ({x},{y},{z}) — but the `void={void}` reservation offers no bridge layer to escape to (bridges need at least y=1, which requires void>=2)",
        kind = entry.kind.label(),
        name = entry.name,
        first_label = net_label(first, ir),
        second_label = net_label(second, ir),
        x = crossing.anchor.x,
        y = crossing.anchor.y,
        z = crossing.anchor.z,
        void = reservation.void,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::CrossingCongestion,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: increase `void` so bridges have a y-layer above the plane, enlarge `region=` so fewer wires cross, or split the logic across multiple `circuit` blocks",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

/// One crossing the reservation has a layer for and no pass lifts.
///
/// Says what the layout does rather than what the reservation allows:
/// the height is why this is a warning and not a refusal, so it is the
/// note and not the headline.
///
/// The suggested fix names splitting the logic first because enlarging
/// `region=` is not always a remedy — the I/O pads are packed down the
/// column at `x=0` by index, so two nets that cross on their way out of
/// that column go on crossing however wide the footprint grows.
fn wire_crossing_diagnostic(
    entry: &ScopedPlacementIrEntry,
    ir: &PlacementIr,
    reservation: &CircuitRegionReservation,
    crossing: &WireCrossing,
) -> Diagnostic {
    let (first, second) = crossing.nets;
    let primary = format!(
        "routed netlist for {kind} `{name}`: {first_label} and {second_label} both run through ({x},{y},{z}), so the two signals are one strand of dust in the layout these coords describe — crossing legalization escapes buffer repeaters only, and never the wire itself",
        kind = entry.kind.label(),
        name = entry.name,
        first_label = net_label(first, ir),
        second_label = net_label(second, ir),
        x = crossing.anchor.x,
        y = crossing.anchor.y,
        z = crossing.anchor.z,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::WireCrossing,
        reservation.span.clone(),
        primary,
    );
    if crossing.coords > 1 {
        diag = diag.with_footer(format!(
            "the two share {coords} coords in all; the one named above is the lowest of them",
            coords = crossing.coords,
        ));
    }
    diag = diag.with_footer(format!(
        "the `void={void}` reservation does have a layer above the plane to lift one of them onto — nothing in the v1 pipeline lifts it",
        void = reservation.void,
    ));
    diag = diag.with_footer(
        "Fix: split the logic across multiple `circuit` blocks so each routes fewer nets, or enlarge `region=` — though a crossing among the I/O pads at `x=0` will not move, since they are packed down that column by index",
    );
    debug_assert_eq!(diag.severity(), Severity::Warning);
    diag
}

/// Human-facing label for a [`NetRef`] used inside diagnostic prose.
/// Resolves `Input(i)` to the sensor's dotted `sig.<name>` when the
/// scope carries one at that index, falling back to the raw `input pad
/// #i` form for a hand-built IR whose input row is shorter than the
/// synthesis path implies. Cell drivers surface as `cell #j` because
/// the Netlist IR does not carry a source-level name for a synthesised
/// gate.
fn net_label(net: NetRef, ir: &PlacementIr) -> String {
    match net {
        NetRef::Input(i) => ir
            .inputs
            .get(i as usize)
            .map_or_else(|| format!("input pad #{i}"), |input| input.name.to_string()),
        NetRef::Cell(j) => format!("cell #{j}"),
    }
}

/// What is standing on the coord, in the words the two collision
/// diagnostics both use. Shared so the cell-side and output-side
/// findings cannot start describing the same obstruction differently.
fn plane_occupant_label(occupant: PlaneOccupant, ir: &PlacementIr) -> String {
    match occupant {
        PlaneOccupant::Component => "a cell body or an I/O pad".to_owned(),
        PlaneOccupant::OtherNet(other) => format!("{}'s wire", net_label(other, ir)),
        PlaneOccupant::Buffer | PlaneOccupant::OwnBuffer => "another buffer repeater".to_owned(),
    }
}

fn buffer_collision_diagnostic(
    entry: &ScopedPlacementIrEntry,
    ir: &PlacementIr,
    reservation: &CircuitRegionReservation,
    cell_index: usize,
    candidate: CellCoord,
    occupant: PlaneOccupant,
) -> Diagnostic {
    let held_by = plane_occupant_label(occupant, ir);
    let primary = format!(
        "routed netlist for {kind} `{name}` cannot place an implicit buffer repeater for cell #{cell_index}: its wire reaches ({x},{y},{z}), which is already {held_by}, and the `void={void}` reservation offers no bridge layer to escape to",
        kind = entry.kind.label(),
        name = entry.name,
        x = candidate.x,
        y = candidate.y,
        z = candidate.z,
        void = reservation.void,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::BufferCoordCollision,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: increase `void` so buffers can fall onto a bridge layer, or enlarge `region=` so buffer candidates have room on the plane",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

fn missing_region_diagnostic(entry: &ScopedPlacementIrEntry) -> Diagnostic {
    let span = entry
        .ir
        .cells
        .first()
        .map(|c| c.span.clone())
        .unwrap_or_default();
    let primary = format!(
        "delayed netlist for {kind} `{name}` reached crossing legalization carrying cells or output drivers but no `circuit region=<label> void=<N>` reservation — the placement pass should have elided this scope",
        kind = entry.kind.label(),
        name = entry.name,
    );
    let mut diag = Diagnostic::new(DiagnosticCode::NoCircuitRegion, span, primary);
    diag = diag.with_footer(
        "Fix: add a `circuit region=<label> void=<N>` line to the enclosing scope, or run `--stage placement` first to see the underlying error",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

#[cfg(test)]
mod tests {
    //! Crate-internal unit tests for crossing-legalization behaviours
    //! that `tests/crossing.rs` cannot reach through synth fixtures
    //! alone:
    //! - the plane-crossing branch (needs two nets whose routed
    //!   trees actually overlap on a non-endpoint coord, which no
    //!   realistic single-scope `.crn` produces yet — the
    //!   `redstone-door` example has one cell and one net);
    //! - the buffer-coord bridge escape (needs a segment past 15
    //!   blocks and a candidate that collides with a reserved coord,
    //!   which again the fixtures do not exercise);
    //! - the `E_CROSSING_CONGESTION` / `E_BUFFER_COORD_COLLISION`
    //!   diagnostic codes and the `E_NO_CIRCUIT_REGION` refusal;
    //! - the topological invariant panic mirroring
    //!   [`crate::delay::compile_delay`].
    //!
    //! Uses crate-internal struct construction (all `PlacedCellNode` /
    //! `PlacementIr` / `CircuitRegionReservation` fields are `pub`;
    //! `#[non_exhaustive]` blocks only external crates), keeping the
    //! integration-test surface in `tests/crossing.rs` focused on
    //! synth fixtures.

    use cairn_lang_core::Edition;
    use cairn_lang_core::error::Span;

    use std::collections::{HashMap, HashSet};

    use super::{PlaneOccupant, Severity, claim_bridge, compile_crossing, plane_occupant};
    use crate::delay::{BUFFER_REPEATER_TICKS, compile_delay};
    use crate::diagnostic::{Diagnostic, DiagnosticCode};
    use crate::edition_netlist_ir::EditionCell;
    use crate::logic_ir::ScopeKind;
    use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
    use crate::placement_ir::{BufferSegment, PlacedOutputNode};
    use crate::placement_ir::{
        CellCoord, CircuitRegionReservation, PlacedCellNode, PlacementIr, PlacementPhase,
        RouteLayer, ScopedPlacementIr, ScopedPlacementIrEntry,
    };
    use crate::routing::compile_routing;
    use crate::routing_geometry::{Router, block_sites, collect_nets, input_pad, net_trees};

    /// The Error-severity half of a run's findings.
    ///
    /// Most fixtures below are built so one net's dust runs through
    /// the coord another net wants a repeater on — which is a
    /// crossing, and the pass says so. What those fixtures are about
    /// is the buffer escape, so they assert that nothing *refused*
    /// them rather than that nothing was said. Fixtures with no
    /// crossing keep asserting the whole list is empty, which is the
    /// stronger claim and the one that would notice a warning
    /// appearing where two nets never meet.
    fn errors(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
        diagnostics
            .iter()
            .filter(|d| d.severity() == Severity::Error)
            .collect()
    }

    /// The primary line of every `W_WIRE_CROSSING` in a run, in the
    /// order the pass raised them.
    fn crossing_reports(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WireCrossing)
            .map(|d| d.primary.as_str())
            .collect()
    }

    fn reservation(width: u32, depth: u32, void: u32) -> CircuitRegionReservation {
        CircuitRegionReservation {
            label: "floor".to_owned(),
            void,
            width,
            depth,
            span: Span::default(),
        }
    }

    fn scoped(kind: ScopeKind, name: &str, ir: PlacementIr) -> ScopedPlacementIr {
        let mut scoped = ScopedPlacementIr::new();
        scoped.scopes.push(ScopedPlacementIrEntry {
            kind,
            name: name.to_owned(),
            ir,
        });
        scoped
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
            phase: PlacementPhase::Delayed {
                wire_length: 0,
                delay_ticks: 0,
            },
            span: Span::default(),
        }
    }

    #[test]
    fn no_scopes_input_yields_no_scopes_output() {
        // A module without any redstone (upstream stages already
        // elided every empty scope through `ScopedPlacementIr::push`)
        // survives crossing legalization as a no-op, matching the
        // delay pass's shape.
        let legalized = compile_crossing(&ScopedPlacementIr::new());
        assert!(legalized.diagnostics.is_empty());
        assert!(legalized.scoped.scopes.is_empty());
    }

    #[test]
    fn empty_scope_passes_through_untouched() {
        // A caller-side hand-built input that hands the pass a scope
        // with no cells and no outputs must return that scope verbatim
        // — the "empty" elision is `ScopedPlacementIr::push`'s job on
        // the input side, not this pass's. Mirrors the delay pass's
        // pass-through so a downstream reader sees consistent
        // behaviour across stages.
        let ir = PlacementIr::new(Edition::Java);
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "empty", ir));
        assert!(legalized.diagnostics.is_empty());
        assert_eq!(legalized.scoped.scopes.len(), 1);
        assert!(legalized.scoped.scopes[0].ir.cells.is_empty());
    }

    #[test]
    fn single_net_no_buffer_is_untouched() {
        // 1 cell driven from Input(0), routed segment <= 15 → no
        // buffer coord, no crossing. `buffer_coords` stays empty.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(0, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "single", ir));
        assert!(legalized.diagnostics.is_empty());
        let cell = &legalized.scoped.scopes[0].ir.cells[0];
        assert!(
            cell.buffer_coords().is_empty(),
            "segment <= 15 blocks needs no buffer, got {:?}",
            cell.buffer_coords(),
        );
        assert_eq!(
            cell.coord.layer,
            RouteLayer::Plane,
            "cell coord stays on plane",
        );
    }

    #[test]
    fn long_segment_places_buffer_on_plane() {
        // A routed driver segment of 16 blocks trips the
        // attenuation limit once → exactly one buffer coord at
        // `k=1 * DUST_ATTENUATION_LIMIT = 15` steps along the route.
        // No collision → the buffer sits on `RouteLayer::Plane`.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(16, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "long", ir));
        assert!(
            legalized.diagnostics.is_empty(),
            "clean fixture: {:?}",
            legalized.diagnostics,
        );
        let cell = &legalized.scoped.scopes[0].ir.cells[0];
        assert_eq!(
            cell.buffer_coords().len(),
            1,
            "16-block segment needs exactly one buffer, got {:?}",
            cell.buffer_coords(),
        );
        assert_eq!(
            cell.buffer_coords()[0].coord.layer,
            RouteLayer::Plane,
            "no collision → buffer stays on plane",
        );
        assert_eq!(
            cell.buffer_coords()[0].port,
            BufferSegment::Port(PortName::A),
            "buffer preserves its driver port on the plane placement path",
        );
        // Pins the `PlacedCellNode` `Serialize` impl's widest path
        // (stage + cell + drivers + coord + wire_length + delay_ticks
        // + buffer_coords). Without this, no test would exercise the
        // full `Legalized { buffer_coords: <non-empty> }` JSON shape —
        // only the narrower `Legalized { buffer_coords: empty }` case
        // is covered by the byte-identity tests. A regression that
        // dropped `buffer_coords` (or announced the wrong
        // `field_count`) would slip past every other assertion here.
        let json = serde_json::to_string(&legalized.scoped)
            .expect("legalized scoped IR must serialise cleanly");
        assert!(
            json.contains(
                "\"buffer_coords\":[{\"port\":\"a\",\"coord\":{\"x\":15,\"y\":0,\"z\":1}}]"
            ),
            "expected buffer_coords entry to appear in JSON verbatim, got {json}",
        );
        // The stage tag and a populated `buffer_coords` coexist: no
        // `.crn` example reaches this path (every fixture's segments
        // sit below the attenuation limit), so this hand-built IR is
        // the only place the pairing is observable.
        assert!(
            json.contains("\"stage\":\"crossing\""),
            "expected the crossing stage tag alongside buffer_coords, got {json}",
        );
    }

    #[test]
    fn crossing_congestion_fires_when_void_is_one() {
        // Two nets whose routed trees overlap on a plane coord with
        // `void=1`: no bridge layer to escape to → refuse. Built by
        // hand because a single-scope `.crn` with genuine crossings
        // does not exist in the example set yet.
        //
        // Input pads land at (0, 0, 1) and (0, 0, 2) per
        // `routing_geometry::input_pad` (z = 1 + i, saturating at depth-1).
        // Placing cell 0 at (3, 0, 3) and cell 1 at (3, 0, 1) makes
        // the two routes (x before z, nothing in the way) share the
        // wire coord (3, 0, 2) — neither cell nor pad, so it counts as
        // a genuine crossing. void=1 rejects the bridge escape.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 5, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["b".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 3),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "crossed", ir));
        assert_eq!(
            legalized
                .diagnostics
                .iter()
                .map(|d| d.code)
                .collect::<Vec<_>>(),
            vec![DiagnosticCode::CrossingCongestion],
            "void=1 must trip E_CROSSING_CONGESTION and nothing else — the \
             refusal already carries the crossing, so a `W_WIRE_CROSSING` \
             beside it would report one defect twice: {:?}",
            legalized.diagnostics,
        );
        assert!(
            legalized.scoped.scopes.is_empty(),
            "failed scope must elide",
        );
    }

    /// The same two nets over the same coord, with a service layer
    /// reserved above them.
    ///
    /// Nothing about the wire changes: v1 lifts buffer repeaters onto
    /// a bridge and never the wire itself, so those two signals are
    /// one strand of dust here exactly as they are at `void=1`. What
    /// changes is whether the author can do anything about it without
    /// touching `void=` — they can, so the scope is kept and the
    /// finding is a warning.
    ///
    /// The pair shares two coords and earns one warning: a reader who
    /// is told twice learns nothing the first telling did not carry.
    #[test]
    fn a_crossing_with_a_layer_to_spare_is_reported_and_the_scope_kept() {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 5, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["b".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 3),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "crossed", ir));
        assert_eq!(
            legalized
                .diagnostics
                .iter()
                .map(|d| d.code)
                .collect::<Vec<_>>(),
            vec![DiagnosticCode::WireCrossing],
            "one warning for the one pair: {:?}",
            legalized.diagnostics,
        );
        let warning = &legalized.diagnostics[0];
        assert_eq!(
            warning.primary,
            "routed netlist for struct `crossed`: sig.a and sig.b both run \
             through (2,0,2), so the two signals are one strand of dust in \
             the layout these coords describe — crossing legalization \
             escapes buffer repeaters only, and never the wire itself",
        );
        assert!(
            warning
                .notes
                .iter()
                .any(|note| note.message.contains("the two share 2 coords in all")),
            "the count of shared coords is what says how far the merge runs: {:?}",
            warning.notes,
        );
        assert_eq!(legalized.scoped.scopes.len(), 1);
        assert_eq!(legalized.scoped.scopes[0].ir.cells.len(), 2);
        // The warning is a report, not a repair: cells and buffers
        // stay exactly where the unwarned path leaves them. Locked
        // here so a change that grows wire-layer materialisation on
        // the IR trips this test rather than silently reshaping the
        // wire form.
        for cell in &legalized.scoped.scopes[0].ir.cells {
            assert_eq!(
                cell.coord.layer,
                RouteLayer::Plane,
                "cell coord stays on plane; got {:?}",
                cell.coord,
            );
            assert!(
                cell.buffer_coords().is_empty(),
                "short segments need no buffer coord; got {:?}",
                cell.buffer_coords(),
            );
        }
    }

    /// A sink fourteen blocks from its driver with something standing
    /// halfway.
    ///
    /// The pad is at `(0,0,1)` and the sink at `(14,0,1)`, so the
    /// straight line between them is 14 blocks and asks for no buffer
    /// repeater at all. A block at `(7,0,1)` sends the wire around it,
    /// and the two blocks that costs put the segment over the
    /// attenuation limit: 16 blocks of dust, and a repeater 15 along.
    /// That gap between the straight line and the route is what every
    /// measurement in this pass has to be taken along.
    ///
    /// The blocker drives nothing. A block is all the fixture needs,
    /// and a driver would make it a sink of the net under test — a
    /// different layout, and one with wire of its own running through
    /// the coords the assertions are about.
    fn walled_scope(void: u32) -> ScopedPlacementIr {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, void));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(7, 0, 1),
            Vec::new(),
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(14, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        scoped(ScopeKind::Struct, "walled", ir)
    }

    /// The index of the sink in [`walled_scope`]'s cell list.
    const WALLED_SINK: usize = 1;

    /// The buffer materialises on the dust the signal actually
    /// travels, at 15 blocks along the 16-block route.
    ///
    /// The straight line from the pad to this sink is 14 blocks, so
    /// measuring it asks for no buffer at all: 16 blocks of dust with
    /// nothing refreshing it, which is the signal never arriving. The
    /// coord is pinned rather than derived so a change to the axis
    /// order or the tie-break has to say so here.
    #[test]
    fn buffer_lands_on_the_routed_path_not_the_straight_line() {
        let legalized = compile_crossing(&walled_scope(2));
        assert!(
            legalized.diagnostics.is_empty(),
            "the candidate is free wire: {:?}",
            legalized.diagnostics,
        );
        let cells = &legalized.scoped.scopes[0].ir.cells;
        let buffers = cells[WALLED_SINK].buffer_coords();
        assert_eq!(buffers.len(), 1, "16 blocks of dust need one: {buffers:?}");
        assert_eq!(buffers[0].coord, CellCoord::new(14, 0, 0));
        assert_eq!(buffers[0].coord.layer, RouteLayer::Plane);
    }

    /// A second net over the walled fixture, so the "no buffer sits on
    /// a foreign net's dust" half of `every_buffer_stands_on_dust_the_
    /// routing_pass_laid` has something to reject. `sig.b` runs from
    /// its own pad along the row the `sig.a` route comes back down.
    fn two_net_walled_scope() -> ScopedPlacementIr {
        let mut scope = walled_scope(2);
        let ir = &mut scope.scopes[0].ir;
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["b".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(13, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        scope
    }

    /// Every buffer stands on dust **stage 2 laid** — on the plane, a
    /// coord of its own net's [`NetTree::wire_path`]; on a bridge, a
    /// layer above one in the same column — and never on a coord
    /// another net's wire runs through, whichever layer that is.
    ///
    /// "A layer above one in the same column" rather than "the plane
    /// coord below it", because the routed wire itself climbs now: a
    /// candidate the router lifted over a block is a bridge coord, and
    /// a repeater escaping that stands over a bridge rather than over
    /// the ground layer.
    ///
    /// Checked against `wire_path` rather than against the `route_to`
    /// the allocator itself reads: comparing the production path to
    /// itself would assert nothing, and the failure this guards is
    /// precisely a route that wanders off the wire the routing pass
    /// put in the occupancy set.
    #[test]
    fn every_buffer_stands_on_dust_the_routing_pass_laid() {
        let legalized = compile_crossing(&two_net_walled_scope());
        assert!(
            errors(&legalized.diagnostics).is_empty(),
            "{:?}",
            legalized.diagnostics
        );
        let ir = &legalized.scoped.scopes[0].ir;
        let region = ir.region.clone().expect("fixture carries a region");
        let nets = collect_nets(ir);
        assert!(
            nets.len() >= 2,
            "the fixture needs a second net for the cross-net half to run: {nets:?}",
        );
        let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();
        let router = Router::new(&region, &block_sites(ir, &region));
        let trees = net_trees(&nets, &router, |net| match net {
            NetRef::Input(i) => input_pad(i as usize, &region),
            NetRef::Cell(j) => cell_coords[j as usize],
        });
        let owned: HashMap<NetRef, HashSet<CellCoord>> = trees
            .iter()
            .map(|(net, tree)| (*net, tree.wire_path().into_iter().collect()))
            .collect();

        let mut checked = 0;
        let mut cross_checks = 0;
        for (index, cell) in ir.cells.iter().enumerate() {
            for buffer in cell.buffer_coords() {
                checked += 1;
                let BufferSegment::Port(port) = buffer.port else {
                    panic!("a cell's buffer must name one of its driver ports");
                };
                let driver = cell
                    .drivers
                    .iter()
                    .find(|d| d.port == port)
                    .expect("every buffer names a driver of its own cell");
                assert!(
                    owned[&driver.net].iter().any(|dust| {
                        (dust.x, dust.z) == (buffer.coord.x, buffer.coord.z)
                            && dust.y <= buffer.coord.y
                    }),
                    "buffer {:?} on cell #{index} is not over dust the routing pass laid for {:?}",
                    buffer.coord,
                    driver.net,
                );
                assert_eq!(
                    buffer.coord.layer == RouteLayer::Plane,
                    buffer.coord.y == 0,
                    "the layer a buffer carries follows its height: {:?}",
                    buffer.coord,
                );
                for (other, dust) in &owned {
                    if *other == driver.net {
                        continue;
                    }
                    cross_checks += 1;
                    assert!(
                        !dust.contains(&buffer.coord),
                        "buffer {:?} shorts onto {other:?}'s wire",
                        buffer.coord,
                    );
                }
            }
        }
        assert!(
            checked >= 1,
            "the fixture has to emit a buffer to mean anything"
        );
        assert!(
            cross_checks >= 1,
            "the cross-net half never ran — the fixture lost its second net",
        );
    }

    /// Stage 3 charges ticks for the repeaters a cell's signals pass
    /// through and stage 4 materialises them; the two counts are one
    /// number or the delay is a fiction.
    ///
    /// Distinct coords rather than `buffer_coords().len()`, because
    /// the vector attributes one entry per driver segment and two
    /// segments of one net share the repeater standing on their
    /// prefix — see [`BufferCoord`]. Every cell in this fixture has a
    /// single driver, so the two counts coincide here;
    /// `ports_sharing_a_net_share_the_repeater_the_charge_and_the_dust`
    /// is where they come apart.
    ///
    /// `phase4_invariant` already property-tests that agreement, but
    /// its strategy seeds sinks along one row from one pad, and an
    /// unobstructed layout is exactly where the straight line and the
    /// route coincide — the invariant held there before this pass read
    /// the route at all. The walled fixture is the discriminating
    /// case: Manhattan says 14 blocks and no buffer, the route says 16
    /// and one.
    #[test]
    fn the_buffer_count_matches_the_ticks_delay_charged() {
        let mut routed = walled_scope(2);
        for cell in &mut routed.scopes[0].ir.cells {
            cell.phase = PlacementPhase::Routed { wire_length: 0 };
        }
        let delayed = crate::delay::compile_delay(&routed);
        assert!(
            delayed.diagnostics.is_empty(),
            "16 blocks is inside the sanity cap: {:?}",
            delayed.diagnostics,
        );
        let legalized = compile_crossing(&delayed.scoped);
        assert!(
            legalized.diagnostics.is_empty(),
            "{:?}",
            legalized.diagnostics
        );

        for (index, cell) in legalized.scoped.scopes[0].ir.cells.iter().enumerate() {
            let charged = cell
                .delay_ticks()
                .expect("stage 3 wrote the ticks")
                .saturating_sub(cell.cell.base_delay_ticks());
            let blocks: HashSet<CellCoord> = cell.buffer_coords().iter().map(|b| b.coord).collect();
            let placed = u32::try_from(blocks.len()).expect("small");
            assert_eq!(
                charged,
                placed.saturating_mul(BUFFER_REPEATER_TICKS),
                "cell #{index} was charged for {charged} ticks of buffer but got {placed} block(s)",
            );
        }
    }

    /// A candidate held by another net escapes upward instead of
    /// taking the coord. This is the arm that keeps a repeater from
    /// tying two signals together, and it is a *success* path — the
    /// refusal below only happens once the bridge layers run out.
    #[test]
    fn a_candidate_on_another_nets_wire_escapes_to_a_bridge() {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(22, 4, 3));
        for name in ["a", "b"] {
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
                span: Span::default(),
            });
        }
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(18, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        // `sig.b` runs from its pad at (0,0,2) down to (15,0,0), so it
        // owns (15,0,1) — the coord `sig.a`'s route wants 15 blocks
        // along.
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(15, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));

        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "shared", ir));
        assert!(
            errors(&legalized.diagnostics).is_empty(),
            "void=3 leaves a bridge layer free: {:?}",
            legalized.diagnostics,
        );
        let buffers = legalized.scoped.scopes[0].ir.cells[0].buffer_coords();
        assert_eq!(buffers.len(), 1);
        assert_eq!(
            buffers[0].coord,
            CellCoord::with_layer(15, 1, 1, RouteLayer::Bridge),
            "the repeater lifts off the shared coord rather than joining the two nets",
        );
    }

    /// A repeater is never lifted onto a coord a wire already runs
    /// through.
    ///
    /// The routed wire climbs off the ground layer to get past a block
    /// now, so a bridge column is not empty by construction any more.
    /// Called directly: reaching a taken bridge layer through
    /// `compile_crossing` needs a layout where one net climbs over
    /// exactly the column a second net's repeater is bounced out of,
    /// and the fixture that arranged it would be describing the search
    /// rather than the rule.
    #[test]
    fn a_bridge_layer_a_wire_runs_through_is_not_free() {
        let region = reservation(20, 3, 4);
        let candidate = CellCoord::new(15, 0, 1);
        let occupied = CellCoord::new(15, 1, 1);
        let wire_owners: HashMap<CellCoord, Vec<NetRef>> =
            [(occupied, vec![NetRef::Input(0)])].into_iter().collect();

        let mut ledger: HashSet<CellCoord> = HashSet::new();
        assert_eq!(
            claim_bridge(candidate, &region, &mut ledger, &wire_owners),
            Some(CellCoord::new(15, 2, 1)),
            "the first free layer is the one above the wire",
        );
        assert_eq!(
            claim_bridge(candidate, &region, &mut ledger, &HashMap::new()),
            Some(CellCoord::new(15, 1, 1)),
            "and with no wire there it is the first layer, so the fixture is not \
             just reading a full ledger",
        );
    }

    /// The occupant a candidate reports is the first reason in a fixed
    /// order, so a coord that is several things at once always reads
    /// the same way. Called directly: reaching every arm through
    /// `compile_crossing` would need three fixtures whose only
    /// difference is which reason wins.
    #[test]
    fn plane_occupant_reports_the_first_reason_in_order() {
        let coord = CellCoord::new(4, 0, 1);
        let mine = NetRef::Input(0);
        let theirs = NetRef::Input(1);
        let reserved: HashSet<CellCoord> = [coord].into_iter().collect();
        let owned: HashMap<CellCoord, Vec<NetRef>> =
            [(coord, vec![mine, theirs])].into_iter().collect();
        // Owned by `theirs`: a repeater `mine` already stands on is not
        // an obstruction to `mine`, so the Buffer arm only reads as one
        // when the buffer belongs to someone else.
        let buffers: HashMap<CellCoord, NetRef> = [(coord, theirs)].into_iter().collect();
        let empty_reserved: HashSet<CellCoord> = HashSet::new();
        let empty_owned: HashMap<CellCoord, Vec<NetRef>> = HashMap::new();
        let empty_buffers: HashMap<CellCoord, NetRef> = HashMap::new();

        assert!(matches!(
            plane_occupant(coord, mine, &reserved, &owned, &buffers),
            Some(PlaneOccupant::Component),
        ));
        assert!(matches!(
            plane_occupant(coord, mine, &empty_reserved, &owned, &buffers),
            Some(PlaneOccupant::OtherNet(net)) if net == theirs,
        ));
        assert!(matches!(
            plane_occupant(coord, mine, &empty_reserved, &empty_owned, &buffers),
            Some(PlaneOccupant::Buffer),
        ));
        assert!(
            plane_occupant(coord, mine, &empty_reserved, &empty_owned, &empty_buffers).is_none(),
        );
        // A coord only this net owns is free for this net's own buffer.
        let ours_only: HashMap<CellCoord, Vec<NetRef>> =
            [(coord, vec![mine])].into_iter().collect();
        assert!(plane_occupant(coord, mine, &empty_reserved, &ours_only, &empty_buffers).is_none(),);
        // This net's own repeater reads as its own: one net's two
        // sinks share their prefix, so the second walk down it revisits
        // the coord the first one claimed, and the caller decides what
        // to do about that.
        let ours_buffer: HashMap<CellCoord, NetRef> = [(coord, mine)].into_iter().collect();
        assert!(matches!(
            plane_occupant(coord, mine, &empty_reserved, &empty_owned, &ours_buffer),
            Some(PlaneOccupant::OwnBuffer),
        ));
    }

    /// Two nets want the same 15-block point, and the reservation
    /// decides whether the second one fits.
    ///
    /// `sig.a` runs left-to-right from its pad along `z=1` and cell #0
    /// drives right-to-left along the same row, so the 15-step point of
    /// both segments is `(15,0,1)` and each is on the other's wire.
    /// Cell #1 asks for it first and is bounced to the first bridge
    /// layer; cell #2 then finds the plane held by `sig.a`'s wire and
    /// the layer above it taken.
    ///
    /// Both sinks sit one row off that shared row so neither cell body
    /// stands on the other net's route — a block in the way would send
    /// the router round it, and the two segments would stop meeting.
    ///
    /// Two nets rather than two cells of one net: a net's own
    /// repeater — on the plane or lifted — serves every segment that
    /// reaches it, so one net can never exhaust a column.
    fn two_nets_over_one_coord_scope(void: u32) -> ScopedPlacementIr {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(40, 4, void));
        for name in ["a", "b"] {
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
                span: Span::default(),
            });
        }
        // #0 is the far driver; its own segment buffers off to the
        // side, on the `sig.b` pad row.
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(30, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        // #1 asks for (15,0,1) first and is lifted off it, because the
        // wire cell #0 drives runs through the coord on its way back
        // down the row.
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(20, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        // #2 is fed by cell #0 across 20 blocks and wants the same
        // coord, with `sig.a`'s wire on the plane below it.
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(10, 0, 2),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(0),
            }],
        ));
        scoped(ScopeKind::Struct, "shared", ir)
    }

    /// One bridge layer, two nets: the refusal names the net that
    /// holds the plane.
    #[test]
    fn buffer_collision_names_the_net_holding_the_coord() {
        let legalized = compile_crossing(&two_nets_over_one_coord_scope(2));
        let diag = legalized
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::BufferCoordCollision)
            .unwrap_or_else(|| panic!("expected a refusal, got {:?}", legalized.diagnostics));
        assert!(
            diag.primary.contains("sig.a's wire"),
            "the refusal must name what holds the coord: {}",
            diag.primary,
        );
        assert!(
            diag.primary.contains("(15,0,1)"),
            "and where: {}",
            diag.primary,
        );
    }

    /// One layer further up, the same two nets both fit — the second
    /// escape lands on `y=2`.
    ///
    /// This is the only case that watches [`claim_bridge`] advance:
    /// every other escape in the suite takes `y=1` on a free column.
    /// A `claim_bridge` that ignored [`HashSet::insert`]'s answer and
    /// always returned `y=1` would put two nets' repeaters on one
    /// block, and nothing else would notice.
    #[test]
    fn a_second_net_over_one_coord_takes_the_next_bridge_layer() {
        let legalized = compile_crossing(&two_nets_over_one_coord_scope(3));
        assert!(
            errors(&legalized.diagnostics).is_empty(),
            "void=3 has a layer for each of them: {:?}",
            legalized.diagnostics,
        );
        let cells = &legalized.scoped.scopes[0].ir.cells;
        assert_eq!(
            cells[1].buffer_coords()[0].coord,
            CellCoord::with_layer(15, 1, 1, RouteLayer::Bridge),
            "the first net to ask takes the first layer",
        );
        assert_eq!(
            cells[2].buffer_coords()[0].coord,
            CellCoord::with_layer(15, 2, 1, RouteLayer::Bridge),
            "the second takes the next one up, not the same block",
        );
    }

    /// Three pairs of nets over one row, reported in net order.
    ///
    /// The pairs are folded out of a `HashMap` keyed by coord, so the
    /// order they come out in is the order the map hands them over —
    /// which is a fresh random order for every map a thread builds.
    /// Legalizing the same scope eight times is therefore eight
    /// different iteration orders, and an implementation that
    /// forwarded them unsorted would have to win eight times running
    /// to pass this.
    ///
    /// The order asserted is `net_ref_key`'s: inputs before cells,
    /// then by index — the order the IR itself lists them in, rather
    /// than the fanout-first `net_order` the pass routes in. Which one
    /// a pair is named in is visible here too: `sig.b` leads
    /// `cell #0` even though the cell has the wider fanout.
    #[test]
    fn several_crossings_are_reported_in_net_order() {
        let expected = [
            "routed netlist for struct `shared`: sig.a and sig.b both run through (9,0,1),",
            "routed netlist for struct `shared`: sig.a and cell #0 both run through (10,0,1),",
            "routed netlist for struct `shared`: sig.b and cell #0 both run through (10,0,1),",
        ];
        for run in 0..8 {
            let legalized = compile_crossing(&two_nets_over_one_coord_scope(3));
            let reports = crossing_reports(&legalized.diagnostics);
            assert_eq!(
                reports.len(),
                expected.len(),
                "run {run}: three pairs share dust on this fixture: {reports:?}",
            );
            for (report, prefix) in reports.iter().zip(expected) {
                assert!(
                    report.starts_with(prefix),
                    "run {run}: expected {prefix:?}, got {report:?}",
                );
            }
        }
    }

    /// The refusal counts pairs of nets, the unit the warning half
    /// counts in, and names the lowest of them.
    ///
    /// Three nets over one row is three pairs, and it is the number a
    /// reader compares against a `void=2` run of the same source — so
    /// a refusal that counted shared coords instead would report six
    /// where the warnings report three, and one of the two would be
    /// describing something else. `sig.a vs sig.b` leads because
    /// `net_ref_key` puts the inputs first, and `(9,0,1)` is the
    /// lowest coord that pair shares.
    #[test]
    fn the_refusal_counts_pairs_of_nets_and_names_the_lowest() {
        let legalized = compile_crossing(&two_nets_over_one_coord_scope(1));
        let diag = legalized
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::CrossingCongestion)
            .expect("crossing must fire");
        assert!(
            diag.primary
                .contains("has 3 wire crossing(s), including sig.a vs sig.b at (9,0,1)"),
            "three pairs, named lowest first: {}",
            diag.primary,
        );
    }

    #[test]
    fn crossing_diagnostic_names_both_conflicting_nets() {
        // Same crossing fixture as the void=1 refusal, this time
        // pinned to the primary text so a diagnostic-consumer can
        // rely on the "left vs right at coord" shape. Uses the
        // `NetlistInput.name` values `sig.a` / `sig.b` so the
        // human-facing label resolution path is exercised.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 5, 1));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["b".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 3),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(3, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "crossed", ir));
        let diag = legalized
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::CrossingCongestion)
            .expect("crossing must fire");
        assert!(
            diag.primary.contains("sig.a"),
            "primary must name the first net (sig.a): {}",
            diag.primary,
        );
        assert!(
            diag.primary.contains("sig.b"),
            "primary must name the second net (sig.b): {}",
            diag.primary,
        );
    }

    /// A fan-out's shared prefix carries one repeater, and every sink
    /// past it is fed by that one.
    ///
    /// The tree reaches the cells in the row through one another, so
    /// the 15-step point of the route into each of them is the same
    /// coord. A repeater standing there refreshes the signal for all
    /// of them; a second sink asking for one is asking for a block
    /// that is already in the layout.
    ///
    /// The `void` column is what the rows are for. Escaping around
    /// the repeater needs a layer above the plane, so two sinks used
    /// to cost two blocks under `void=3` and three sinks were refused
    /// under `void=2` for wanting a third layer. Reuse needs no layer
    /// at all, which is why `void=1` is a row here.
    ///
    /// The coord is compared whole rather than by `x`/`z`: a
    /// `CellCoord::new` is on [`RouteLayer::Plane`] by construction
    /// and `layer` is part of the comparison, so a buffer that
    /// escaped upward fails this assertion rather than passing it on
    /// its footprint.
    #[test]
    fn a_shared_prefix_carries_one_repeater_however_many_sinks_hang_off_it() {
        for (sinks, void) in [(2u32, 3u32), (3, 2), (2, 1), (3, 1)] {
            let mut ir = PlacementIr::new(Edition::Java);
            ir.region = Some(reservation(20, 3, void));
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
                span: Span::default(),
            });
            for x in 16..16 + sinks {
                ir.cells.push(placed_cell(
                    EditionCell::JavaRepeaterOr,
                    CellCoord::new(x, 0, 1),
                    vec![CellPortDriver {
                        port: PortName::A,
                        net: NetRef::Input(0),
                    }],
                ));
            }
            let legalized = compile_crossing(&scoped(ScopeKind::Struct, "fanout", ir));
            assert!(
                legalized.diagnostics.is_empty(),
                "{sinks} sinks at void={void} need one repeater and no escape: {:?}",
                legalized.diagnostics,
            );
            for (index, cell) in legalized.scoped.scopes[0].ir.cells.iter().enumerate() {
                let buffers = cell.buffer_coords();
                assert_eq!(
                    buffers.len(),
                    1,
                    "cell #{index} of {sinks} at void={void}: {buffers:?}",
                );
                assert_eq!(
                    buffers[0].coord,
                    CellCoord::new(15, 0, 1),
                    "cell #{index} of {sinks} at void={void} must name the repeater standing on the shared prefix",
                );
            }
        }
    }

    /// A buffer candidate is never a component.
    ///
    /// A candidate sits strictly between the ends of a route, and the
    /// router keeps every coord strictly between them off the blocks —
    /// so the coord a repeater lands on is wire, never a cell body or a
    /// pad. [`PlaneOccupant::Component`] is unreachable through this
    /// pass for that reason. What still refuses is a candidate on
    /// another net's dust with every bridge layer above it taken, which
    /// `buffer_collision_names_the_net_holding_the_coord` covers.
    ///
    /// The layout below is the one that used to refuse: `sig.a`'s route
    /// ran straight down the row and its 15-step point landed inside
    /// `sig.b`'s cell, with `void=1` reserving no layer to lift the
    /// repeater onto.
    #[test]
    fn a_buffer_candidate_is_never_a_cell_body() {
        let legalized = compile_crossing(&cell_on_the_fifteen_step_point());
        assert!(
            legalized.diagnostics.is_empty(),
            "the route goes round the body, so nothing collides: {:?}",
            legalized.diagnostics,
        );
        let cells = &legalized.scoped.scopes[0].ir.cells;
        let buffers = cells[1].buffer_coords();
        assert_eq!(
            buffers.len(),
            1,
            "the segment is over 15 blocks: {buffers:?}"
        );
        assert_eq!(
            buffers[0].coord.layer,
            RouteLayer::Plane,
            "and it stands on the plane, with no layer needed: {buffers:?}",
        );
        assert_ne!(
            buffers[0].coord,
            CellCoord::new(15, 0, 1),
            "the coord it used to want is the cell body",
        );
    }

    /// `sig.b`'s cell standing where `sig.a`'s route used to put its
    /// repeater.
    fn cell_on_the_fifteen_step_point() -> ScopedPlacementIr {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, 1));
        for name in ["a", "b"] {
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
                span: Span::default(),
            });
        }
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(15, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(16, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        scoped(ScopeKind::Struct, "packed", ir)
    }

    /// Two ports of one cell on one net are fed by one strand of
    /// dust: two attributions, one block, one charge.
    ///
    /// Threaded through routing → delay → crossing rather than handed
    /// a `Delayed` fixture, because the three numbers this pins are
    /// written by three different passes — `wire_length` by stage 2,
    /// `delay_ticks` by stage 3, `buffer_coords` by stage 4 — and the
    /// defect was that each of them counted the one segment once per
    /// port.
    ///
    /// `JavaComparatorAnd` rather than a Mux so the base delay is a
    /// pinned 1 rather than the `_Unpinned` sentinel, and because
    /// `sig.s0 = sig.a and sig.a` is how a `.crn` reaches this shape —
    /// see `ports_sharing_a_net_are_measured_once` in
    /// `tests/routing.rs`.
    #[test]
    fn ports_sharing_a_net_share_the_repeater_the_charge_and_the_dust() {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(PlacedCellNode {
            cell: EditionCell::JavaComparatorAnd,
            drivers: vec![
                CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(0),
                },
                CellPortDriver {
                    port: PortName::B,
                    net: NetRef::Input(0),
                },
            ],
            coord: CellCoord::new(16, 0, 1),
            phase: PlacementPhase::Unrouted,
            span: Span::default(),
        });
        let routed = compile_routing(&scoped(ScopeKind::Struct, "shared", ir));
        assert!(routed.diagnostics.is_empty(), "{:?}", routed.diagnostics);
        let delayed = compile_delay(&routed.scoped);
        assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
        let legalized = compile_crossing(&delayed.scoped);
        assert!(
            legalized.diagnostics.is_empty(),
            "one repeater on free wire needs no escape: {:?}",
            legalized.diagnostics,
        );

        let cell = &legalized.scoped.scopes[0].ir.cells[0];
        // The pad sits at (0,0,1) and the cell at (16,0,1): 16 blocks
        // of dust, laid once.
        assert_eq!(
            cell.wire_length(),
            Some(16),
            "one strand of dust, measured once",
        );
        assert_eq!(
            cell.delay_ticks(),
            Some(1 + BUFFER_REPEATER_TICKS),
            "base 1 plus the one repeater the signal passes through",
        );
        let buffers = cell.buffer_coords();
        assert_eq!(
            buffers.iter().map(|b| b.port).collect::<Vec<_>>(),
            vec![
                BufferSegment::Port(PortName::A),
                BufferSegment::Port(PortName::B),
            ],
            "one attribution per driver, in driver order",
        );
        let distinct: HashSet<CellCoord> = buffers.iter().map(|b| b.coord).collect();
        assert_eq!(
            distinct,
            [CellCoord::new(15, 0, 1)].into_iter().collect(),
            "both attributions name the one block: {buffers:?}",
        );
    }

    /// A repeater this net had to lift off the plane serves every
    /// segment that reaches the coord it was lifted from.
    ///
    /// The cell's two ports read one signal, so both compute the same
    /// candidate — and the candidate carries another net's dust, which
    /// cannot hold this net's repeater. The first port lifts one onto a
    /// bridge layer; the second must record that block rather than ask
    /// for a layer of its own.
    ///
    /// The two rows fail in different directions without the memo. At
    /// `void=2` there is no second layer, so the scope is refused
    /// outright; at `void=3` a second block stacks at `(15,2,1)` over
    /// a point that already has one.
    #[test]
    fn a_lifted_repeater_serves_every_segment_that_reaches_its_coord() {
        for void in [2u32, 3] {
            let mut ir = PlacementIr::new(Edition::Java);
            ir.region = Some(reservation(20, 3, void));
            for name in ["a", "blocker"] {
                ir.inputs.push(crate::netlist_ir::NetlistInput {
                    name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
                    span: Span::default(),
                });
            }
            // Its wire comes down the `x=15` column and crosses
            // `sig.a`'s row at the 15-step point, so the plane there
            // belongs to somebody else.
            ir.cells.push(placed_cell(
                EditionCell::JavaRepeaterOr,
                CellCoord::new(15, 0, 0),
                vec![CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(1),
                }],
            ));
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
                        net: NetRef::Input(0),
                    },
                ],
            ));
            let legalized = compile_crossing(&scoped(ScopeKind::Struct, "lifted", ir));
            assert!(
                errors(&legalized.diagnostics).is_empty(),
                "void={void}: one lift serves both ports: {:?}",
                legalized.diagnostics,
            );
            let buffers = legalized.scoped.scopes[0].ir.cells[1].buffer_coords();
            assert_eq!(
                buffers
                    .iter()
                    .map(|b| (b.port, b.coord))
                    .collect::<Vec<_>>(),
                vec![
                    (
                        BufferSegment::Port(PortName::A),
                        CellCoord::with_layer(15, 1, 1, RouteLayer::Bridge),
                    ),
                    (
                        BufferSegment::Port(PortName::B),
                        CellCoord::with_layer(15, 1, 1, RouteLayer::Bridge),
                    ),
                ],
                "void={void}: both ports name the one lifted block",
            );
        }
    }

    /// The memo answers for the coord that was lifted, not for the net
    /// that lifted it.
    ///
    /// One segment long enough to want three repeaters, with two of
    /// its three candidates carrying another net's dust. A memo keyed
    /// on the net alone would hand the second lift the first one's
    /// coord — a repeater over a point it does not refresh, and a block
    /// count quietly dropping by one.
    #[test]
    fn the_escape_memo_answers_per_coord_and_not_per_net() {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(50, 4, 2));
        for name in ["a", "first_blocker", "second_blocker"] {
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
                span: Span::default(),
            });
        }
        // Each blocker's wire comes down its own column and crosses
        // `sig.a`'s row on the way, so two of the three candidates are
        // held by a net that is not `sig.a`.
        for (x, net) in [(15u32, NetRef::Input(1)), (30, NetRef::Input(2))] {
            ir.cells.push(placed_cell(
                EditionCell::JavaRepeaterOr,
                CellCoord::new(x, 0, 0),
                vec![CellPortDriver {
                    port: PortName::A,
                    net,
                }],
            ));
        }
        // 46 blocks from the pad at (0,0,1): repeaters wanted at 15,
        // 30 and 45, the first two of them on a cell body.
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(46, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "twice", ir));
        assert!(
            errors(&legalized.diagnostics).is_empty(),
            "each column has its own free layer: {:?}",
            legalized.diagnostics,
        );
        assert_eq!(
            legalized.scoped.scopes[0].ir.cells[2]
                .buffer_coords()
                .iter()
                .map(|b| b.coord)
                .collect::<Vec<_>>(),
            vec![
                CellCoord::with_layer(15, 1, 1, RouteLayer::Bridge),
                CellCoord::with_layer(30, 1, 1, RouteLayer::Bridge),
                CellCoord::new(45, 0, 1),
            ],
            "each lift stands over its own candidate, and the free one stays down",
        );
    }

    /// The wire out to an actuator records a repeater a *cell*
    /// segment placed.
    ///
    /// One placer walks the cells and then the actuator pads, so this
    /// is the only direction the two segment kinds can meet: the cell
    /// takes the plane coord, and the pad's route reaches it 15 blocks
    /// along as well. The reverse cannot happen, because no cell is
    /// allocated after an output.
    ///
    /// The pad's route is longer than the cell's by more than the four
    /// blocks between them, because the cell is a block on the row and
    /// the wire out now steps around it — so the pad's segment asks for
    /// a second repeater of its own past the shared one.
    #[test]
    fn an_actuator_segment_records_the_repeater_a_cell_segment_placed() {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(30, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers: vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            coord: CellCoord::new(16, 0, 1),
            phase: PlacementPhase::Unrouted,
            span: Span::default(),
        });
        ir.outputs.push(PlacedOutputNode::new(
            cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            NetRef::Input(0),
            CellCoord::new(29, 0, 1),
            Span::default(),
        ));
        let routed = compile_routing(&scoped(ScopeKind::Struct, "both", ir));
        assert!(routed.diagnostics.is_empty(), "{:?}", routed.diagnostics);
        let delayed = compile_delay(&routed.scoped);
        assert!(delayed.diagnostics.is_empty(), "{:?}", delayed.diagnostics);
        let legalized = compile_crossing(&delayed.scoped);
        assert!(
            legalized.diagnostics.is_empty(),
            "the pad's segment reuses the cell's repeater: {:?}",
            legalized.diagnostics,
        );

        let ir = &legalized.scoped.scopes[0].ir;
        let shared = CellCoord::new(15, 0, 1);
        assert_eq!(
            ir.cells[0]
                .buffer_coords()
                .iter()
                .map(|b| (b.port, b.coord))
                .collect::<Vec<_>>(),
            vec![(BufferSegment::Port(PortName::A), shared)],
        );
        assert_eq!(
            ir.outputs[0]
                .buffer_coords()
                .iter()
                .map(|b| (b.port, b.coord))
                .collect::<Vec<_>>(),
            vec![
                (BufferSegment::Out, shared),
                (BufferSegment::Out, CellCoord::new(29, 0, 0)),
            ],
            "the wire to the pad names the block the cell segment put there",
        );
    }

    /// Every port spelling reaches both push sites and the wire form.
    ///
    /// A regression that hard-coded `PortName::A` at the plane push or
    /// at the bridge push would slip past every other buffer test,
    /// which drives a single `A`. The three drivers are on three
    /// *different* nets — a Mux whose ports shared one net would now
    /// take one coord for all three and never reach the escape, which
    /// is what makes this the fixture that keeps the escape path
    /// covered.
    ///
    /// `sig.blocker`'s route descends the column at `x=15`, so its
    /// dust runs through `(15,0,1)` — the coord the `Sel` route wants
    /// 15 blocks along. Its dust rather than its cell: a body would
    /// answer [`PlaneOccupant::Component`] and never reach the
    /// [`PlaneOccupant::OtherNet`] arm this fixture is built on. `A`
    /// and `B` approach along their own pad rows and find their
    /// candidates free.
    #[test]
    fn mux_ports_reach_the_wire_form_on_plane_and_through_an_escape() {
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(22, 5, 3));
        for name in ["sel", "blocker", "port_a", "port_b"] {
            ir.inputs.push(crate::netlist_ir::NetlistInput {
                name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![name.into()]),
                span: Span::default(),
            });
        }
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(15, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(1),
            }],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaMuxUnpinned,
            CellCoord::new(18, 0, 1),
            vec![
                CellPortDriver {
                    port: PortName::Sel,
                    net: NetRef::Input(0),
                },
                CellPortDriver {
                    port: PortName::A,
                    net: NetRef::Input(2),
                },
                CellPortDriver {
                    port: PortName::B,
                    net: NetRef::Input(3),
                },
            ],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "mux", ir));
        assert!(
            errors(&legalized.diagnostics).is_empty(),
            "void=3 leaves a bridge layer free: {:?}",
            legalized.diagnostics,
        );
        let bufs = legalized.scoped.scopes[0].ir.cells[1].buffer_coords();
        assert_eq!(
            bufs.iter().map(|b| (b.port, b.coord)).collect::<Vec<_>>(),
            vec![
                (
                    BufferSegment::Port(PortName::Sel),
                    CellCoord::with_layer(15, 1, 1, RouteLayer::Bridge),
                ),
                (BufferSegment::Port(PortName::A), CellCoord::new(15, 0, 3),),
                (BufferSegment::Port(PortName::B), CellCoord::new(15, 0, 4),),
            ],
            "each port keeps its own segment's coord across both push sites",
        );
        let json = serde_json::to_string(&legalized.scoped)
            .expect("legalized scoped IR must serialise cleanly");
        for fragment in [
            "\"port\":\"sel\"",
            "\"port\":\"a\"",
            "\"port\":\"b\"",
            "\"layer\":\"bridge\"",
        ] {
            assert!(
                json.contains(fragment),
                "JSON wire form must carry {fragment}, got {json}",
            );
        }
    }

    #[test]
    #[should_panic(
        expected = "for cell #0 at (16,0,1) in struct `twice` — crossing legalization must run exactly once per delayed IR"
    )]
    fn re_running_crossing_pass_panics_loudly() {
        // Chaining `compile_crossing(&legalized.scoped)` is forbidden
        // by the producer↔variant table on `PlacementPhase`. Loud in
        // release so
        // a caller cannot silently double-populate `buffer_coords`.
        // The expected substring pins the cell identity as well as the
        // invariant: the breadcrumb is what tells an operator which
        // cell tripped the guard without walking the backtrace back
        // into the IR.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(20, 3, 2));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(16, 0, 1),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        let first = compile_crossing(&scoped(ScopeKind::Struct, "twice", ir));
        let _second = compile_crossing(&first.scoped);
    }

    #[test]
    fn missing_region_with_cells_fires_no_circuit_region() {
        // Hand-built `PlacementIr` with cells but no region reaches
        // crossing because it skipped placement. Mirrors the delay
        // pass's hardening.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(0, 0, 0),
            vec![],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "roomless", ir));
        let diag = legalized
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
            legalized.scoped.scopes.is_empty(),
            "failed scope must elide",
        );
    }

    #[test]
    fn missing_region_empty_scope_passes_through() {
        // No region + no cells + no outputs = harmless empty scope
        // that the pass returns verbatim. Prior stages will not
        // produce this shape, but a hand-built input reaching here
        // must not trip the `NoCircuitRegion` refusal — that guard
        // only fires when the scope carries cells or outputs.
        let ir = PlacementIr::new(Edition::Java);
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "harmless", ir));
        assert!(legalized.diagnostics.is_empty());
        assert_eq!(legalized.scoped.scopes.len(), 1);
        assert!(legalized.scoped.scopes[0].ir.region.is_none());
    }

    #[test]
    fn output_only_scope_with_missing_region_refuses() {
        // Outputs but no cells with no region: still a phase-table
        // violation because `PlacementIr::is_empty` returns false as
        // long as outputs exist. Should refuse with `NoCircuitRegion`
        // for parity with the delay pass.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.outputs.push(PlacedOutputNode::new(
            cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["x".into()]),
            NetRef::Input(0),
            CellCoord::new(3, 0, 1),
            Span::default(),
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "outputless", ir));
        assert!(
            legalized
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::NoCircuitRegion),
            "missing region with outputs must refuse: {:?}",
            legalized.diagnostics,
        );
    }

    #[test]
    fn buffer_coord_index_at_kth_boundary() {
        // Pins the mapping `k → path_index = k * DUST_ATTENUATION_LIMIT`
        // that `allocate_buffer_coords` inlines. A routed driver segment
        // of 46 blocks trips the attenuation limit three times, so
        // three buffer coords land at `k = 1, 2, 3`. The delay pass has
        // a mirrored boundary-row test on the tick side of the same
        // formula (`s → buffers`); this test is its structural mirror
        // on the coord side — a slip in either pass's
        // `(segment - 1) / DUST_ATTENUATION_LIMIT` derivation trips a
        // dedicated row rather than the aggregate delay total.
        //
        // The route `(0, 0, 1) → (45, 0, 0)` walks x++ 45 steps then
        // z-- 1 step, so `path[k * 15]` is `(k * 15, 0, 1)` for
        // `k = 1, 2, 3` (path[45] is the last x-axis step before the
        // final z-- to the sink). No collision → every buffer stays on
        // plane, which is the invariant the boundary formula depends
        // on: if a k-th buffer landed off-formula, the plane candidate
        // would drift too and the layer assertion would trip alongside
        // the coord assertion.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(50, 3, 3));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(45, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "boundary", ir));
        assert!(
            legalized.diagnostics.is_empty(),
            "clean fixture: {:?}",
            legalized.diagnostics,
        );
        let bufs = legalized.scoped.scopes[0].ir.cells[0].buffer_coords();
        assert_eq!(
            bufs.len(),
            3,
            "segment 46 → floor((46-1)/15) = 3 buffers, got {bufs:?}",
        );
        assert_eq!(
            bufs[0].coord,
            CellCoord::new(15, 0, 1),
            "k=1 buffer sits at path[15]",
        );
        assert_eq!(
            bufs[1].coord,
            CellCoord::new(30, 0, 1),
            "k=2 buffer sits at path[30]",
        );
        assert_eq!(
            bufs[2].coord,
            CellCoord::new(45, 0, 1),
            "k=3 buffer sits at path[45] — the last x-axis step before the final z decrement",
        );
        for b in bufs {
            assert_eq!(
                b.coord.layer,
                RouteLayer::Plane,
                "no collision → every k-th buffer stays on plane; got {b:?}",
            );
        }
    }

    #[test]
    fn buffer_coord_index_at_max_segment_boundary() {
        // Companion to `buffer_coord_index_at_kth_boundary`: pins the
        // formula against the far end of the range the delay pass's
        // `MAX_ATTENUATION_SEGMENT = 256` sanity cap permits. A
        // segment of exactly 256 blocks yields 17 buffers at
        // `k * 15` for `k = 1..=17`, matching the tick-side boundary
        // table row for that segment. Together the two tests bracket
        // the piecewise formula at both boundaries on the coord side.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(256, 3, 3));
        ir.inputs.push(crate::netlist_ir::NetlistInput {
            name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
            span: Span::default(),
        });
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(255, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
        ));
        let legalized = compile_crossing(&scoped(ScopeKind::Struct, "max_boundary", ir));
        assert!(
            legalized.diagnostics.is_empty(),
            "segment == MAX_ATTENUATION_SEGMENT must legalize cleanly: {:?}",
            legalized.diagnostics,
        );
        let bufs = legalized.scoped.scopes[0].ir.cells[0].buffer_coords();
        assert_eq!(bufs.len(), 17, "segment 256 → 17 buffers, got {bufs:?}");
        for k in 1..=17u32 {
            assert_eq!(
                bufs[(k - 1) as usize].coord,
                CellCoord::new(k * 15, 0, 1),
                "k={k} buffer must sit at path[k * 15]",
            );
        }
    }

    #[test]
    #[should_panic(expected = "topological invariant broken")]
    fn out_of_range_net_ref_cell_panics_loudly() {
        // Mirrors `delay.rs`'s equivalent guard: a hand-built IR with
        // `NetRef::Cell(u32::MAX)` violates the synthesis-side
        // topological invariant, and the crossing pass panics loud so
        // a caller-side bug cannot produce silently wrong
        // `buffer_coords`.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(5, 3, 2));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(0, 0, 0),
            vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Cell(u32::MAX),
            }],
        ));
        let _ = compile_crossing(&scoped(ScopeKind::Struct, "broken", ir));
    }

    #[test]
    #[should_panic(
        expected = "for cell #1 at (4,0,1) in struct `mixed` — crossing legalization must run exactly once per delayed IR"
    )]
    fn legalize_panic_names_the_offending_cell_not_the_first_one() {
        // Re-running the whole pass always trips on `cells[0]`, which
        // would let a regression that hardcoded the index to zero — or
        // that read the coord off the wrong cell — pass unnoticed. A
        // hand-built IR whose first cell is still `Delayed` while the
        // second is already `Legalized` forces the panic past the head
        // of the loop, so both the index and the coord have to be
        // threaded from the cell actually being transitioned.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(8, 3, 2));
        ir.cells.push(placed_cell(
            EditionCell::JavaRepeaterOr,
            CellCoord::new(0, 0, 0),
            vec![],
        ));
        let mut already_legalized =
            placed_cell(EditionCell::JavaRepeaterOr, CellCoord::new(4, 0, 1), vec![]);
        already_legalized.phase = PlacementPhase::Legalized {
            wire_length: 0,
            delay_ticks: 0,
            buffer_coords: Vec::new(),
        };
        ir.cells.push(already_legalized);
        let _ = compile_crossing(&scoped(ScopeKind::Struct, "mixed", ir));
    }

    mod phase4_invariant {
        //! Property tests for the crossing / delay agreement invariant
        //! (see the `phase4_buffer_tick_invariant_holds` doc). Kept
        //! inside the crossing pass's own crate-internal test module so
        //! the strategy can hand-build `Unrouted` [`PlacedCellNode`]s
        //! (the `PlacementPhase` variants and the `pub(crate)`
        //! `phase` field are not reachable from `tests/crossing.rs`).
        use proptest::prelude::*;

        use super::{
            BufferSegment, CellCoord, CellPortDriver, Edition, EditionCell, HashSet, NetRef,
            PlacedCellNode, PlacementIr, PlacementPhase, PortName, RouteLayer, Router, ScopeKind,
            Span, block_sites, collect_nets, compile_crossing, input_pad, net_trees, placed_cell,
            reservation, scoped,
        };
        use crate::delay::{BUFFER_REPEATER_TICKS, compile_delay};
        use crate::routing::compile_routing;

        /// Strategy over sink positions for the phase-4 invariant
        /// property test. Each `(x, z)` in the returned `Vec` seeds one
        /// cell at `(x, 0, z)` driven from `Input(0)` at `(0, 0, 1)`.
        /// `x` in `1..=99` covers both the sub-limit segments (zero
        /// buffers) and the multi-boundary ones, so `buffer_total` is
        /// non-zero on most cases and the invariant discriminates.
        ///
        /// `z` varies rather than sitting at 0. A row of collinear
        /// sinks is the one shape where every strand runs along a
        /// single axis, which is where a route and the straight line
        /// between its ends coincide — and coinciding is what hid the
        /// bug this suite missed. Off-axis terminals are what let the
        /// two differ.
        ///
        /// The `bool` gives a cell a second port on the *same* net.
        /// Without it every cell has one driver, and the block count
        /// and the attribution count coincide for the whole strategy
        /// — which is how the invariant below could be stated in
        /// attributions and still hold.
        ///
        /// Changing the tuple invalidates every persisted seed in
        /// `proptest-regressions/crossing.txt`: a seed is an RNG
        /// state, so it replays as an unrelated value under a
        /// different shape rather than failing to load. Anything a
        /// retired seed was holding has to be written as a named test
        /// before the shape changes.
        fn phase4_scope_strategy() -> impl Strategy<Value = Vec<(u32, u32, bool)>> {
            prop::collection::vec((1u32..=99u32, 0u32..8u32, prop::bool::ANY), 1..=3)
        }

        /// Placate an unused-import lint when the outer `mod tests`
        /// re-exports items the inner `super::*` glob would otherwise
        /// re-import; keeps `placed_cell` alive as an intentional
        /// symbol import for future cases that want the shorter
        /// helper.
        #[allow(dead_code)]
        fn _keep_placed_cell_alive() -> PlacedCellNode {
            placed_cell(
                EditionCell::JavaRepeaterOr,
                CellCoord::new(0, 0, 0),
                Vec::new(),
            )
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

            /// Crossing / delay agreement invariant: on every emitted
            /// scope, `Σ (blocks under cell.buffer_coords()) ×
            /// BUFFER_REPEATER_TICKS` must equal `Σ (cell.delay_ticks()
            /// − cell.cell.base_delay_ticks())`, where the blocks under
            /// a cell are its buffer coords deduplicated: the vector
            /// attributes an entry per driver segment, and segments of
            /// one net share the repeater on their prefix. The sum is
            /// over cells rather than over the scope's blocks, because
            /// what it adds up is per-cell latency — a repeater two
            /// cells hang off delays both of them. A drift in either
            /// pass's `(segment − 1) / DUST_ATTENUATION_LIMIT`
            /// derivation, in `BUFFER_REPEATER_TICKS`, or in the
            /// per-edition base-delay table trips this shared
            /// assertion rather than each pass's own boundary rows
            /// in isolation.
            ///
            /// Uses hand-built `Unrouted` cells threaded through
            /// `compile_routing → compile_delay → compile_crossing`.
            /// `checked_sub` / `checked_mul` refuse to silently clamp
            /// on the failure direction the invariant is meant to
            /// catch (a delay pass regressing below the edition
            /// `base_delay_ticks`, or a buffer count overflowing
            /// `u32`).
            #[test]
            fn phase4_buffer_tick_invariant_holds(xs in phase4_scope_strategy()) {
                let mut ir = PlacementIr::new(Edition::Java);
                ir.region = Some(reservation(200, 10, 3));
                ir.inputs.push(crate::netlist_ir::NetlistInput {
                    name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec!["a".into()]),
                    span: Span::default(),
                });
                for &(x, z, shared_port) in &xs {
                    let mut drivers = vec![CellPortDriver {
                        port: PortName::A,
                        net: NetRef::Input(0),
                    }];
                    if shared_port {
                        drivers.push(CellPortDriver {
                            port: PortName::B,
                            net: NetRef::Input(0),
                        });
                    }
                    ir.cells.push(PlacedCellNode {
                        cell: EditionCell::JavaRepeaterOr,
                        drivers,
                        coord: CellCoord::new(x, 0, z),
                        phase: PlacementPhase::Unrouted,
                        span: Span::default(),
                    });
                }
                let routed = compile_routing(&scoped(ScopeKind::Struct, "prop", ir));
                prop_assert!(
                    routed.diagnostics.is_empty(),
                    "routing diagnostics for xs={:?}: {:?}",
                    xs,
                    routed.diagnostics,
                );
                let delayed = compile_delay(&routed.scoped);
                prop_assert!(
                    delayed.diagnostics.is_empty(),
                    "delay diagnostics for xs={:?}: {:?}",
                    xs,
                    delayed.diagnostics,
                );
                let legalized = compile_crossing(&delayed.scoped);
                // A refused scope is elided, and an elided scope
                // carries no buffers to check. Off-axis terminals make
                // two sinks share a route prefix often enough that
                // `void=3` runs out of bridge layers, and refusing is
                // the documented answer there — so the case is
                // rejected rather than failed.
                prop_assume!(legalized.diagnostics.is_empty());

                for entry in &legalized.scoped.scopes {
                    // Where the buffers landed, not just how many.
                    // Totals alone let a coord move anywhere as long as
                    // the count holds, which is exactly the shape of
                    // the bug this suite missed.
                    let region = entry.ir.region.clone().expect("fixture carries a region");
                    let nets = collect_nets(&entry.ir);
                    let cell_coords: Vec<CellCoord> =
                        entry.ir.cells.iter().map(|c| c.coord).collect();
                    let router = Router::new(&region, &block_sites(&entry.ir, &region));
                    let trees = net_trees(&nets, &router, |net| match net {
                        NetRef::Input(i) => input_pad(i as usize, &region),
                        NetRef::Cell(j) => cell_coords[j as usize],
                    });
                    for cell in &entry.ir.cells {
                        for buffer in cell.buffer_coords() {
                            let BufferSegment::Port(port) = buffer.port else {
                                panic!("a cell's buffer must name one of its driver ports");
                            };
                            let driver = cell
                                .drivers
                                .iter()
                                .find(|d| d.port == port)
                                .expect("every buffer names a driver of its own cell");
                            let dust: HashSet<CellCoord> =
                                trees[&driver.net].wire_path().into_iter().collect();
                            let footprint =
                                CellCoord::new(buffer.coord.x, 0, buffer.coord.z);
                            prop_assert!(
                                dust.contains(&footprint),
                                "buffer {:?} is not over dust the routing pass laid for {:?} (xs={:?})",
                                buffer.coord,
                                driver.net,
                                xs,
                            );
                            // And on *this* segment's route, not just
                            // somewhere on the net. The two differ for
                            // an entry the placer took from a memo
                            // rather than from the route it was
                            // walking: `wire_path` is the whole net's
                            // dust and would accept a coord that
                            // refreshes a sibling sink instead of this
                            // one.
                            let route: HashSet<CellCoord> = trees[&driver.net]
                                .route_to(cell.coord)
                                .expect("every cell is a terminal of the net driving it")
                                .into_iter()
                                .collect();
                            prop_assert!(
                                route.contains(&footprint),
                                "buffer {:?} is not over the route into this cell for {:?} (xs={:?})",
                                buffer.coord,
                                driver.net,
                                xs,
                            );
                            if buffer.coord.layer == RouteLayer::Plane {
                                prop_assert_eq!(buffer.coord.y, 0);
                            }
                        }
                    }
                    // Per cell as well as in total: an over-charged
                    // cell and an under-charged neighbour cancel in a
                    // sum.
                    for cell in &entry.ir.cells {
                        let dt = cell
                            .delay_ticks()
                            .expect("legalized cells carry Some(delay_ticks)");
                        let base = cell.cell.base_delay_ticks();
                        // `checked_sub` here as well as in the scope
                        // total: a cell that regressed below its
                        // edition base would clamp to zero and match a
                        // cell with no buffers, which is the failure
                        // the per-cell loop exists to name.
                        let delta = dt.checked_sub(base);
                        prop_assert!(
                            delta.is_some(),
                            "delay_ticks {} < base_delay_ticks {} for cell at {:?} (xs={:?})",
                            dt,
                            base,
                            cell.coord,
                            xs,
                        );
                        let delta = delta.unwrap_or_default();
                        let blocks: HashSet<CellCoord> =
                            cell.buffer_coords().iter().map(|b| b.coord).collect();
                        let placed = u32::try_from(blocks.len())
                            .expect("buffer block count fits in u32");
                        prop_assert_eq!(
                            delta,
                            placed.checked_mul(BUFFER_REPEATER_TICKS)
                                .expect("block count × BUFFER_REPEATER_TICKS fits in u32"),
                            "cell at {:?} charged {} ticks of buffer but stands on {} block(s) (xs={:?})",
                            cell.coord,
                            delta,
                            placed,
                            xs,
                        );
                    }
                }

                for entry in &legalized.scoped.scopes {
                    let buffer_total: u32 = entry
                        .ir
                        .cells
                        .iter()
                        .map(|c| {
                            let blocks: HashSet<CellCoord> =
                                c.buffer_coords().iter().map(|b| b.coord).collect();
                            u32::try_from(blocks.len())
                                .expect("buffer block count fits in u32")
                        })
                        .sum();
                    let mut delta_total: u32 = 0;
                    for cell in &entry.ir.cells {
                        let dt = cell
                            .delay_ticks()
                            .expect("legalized cells carry Some(delay_ticks)");
                        let base = cell.cell.base_delay_ticks();
                        let delta = dt.checked_sub(base);
                        prop_assert!(
                            delta.is_some(),
                            "delay_ticks {} < base_delay_ticks {} for cell at {:?} — delay pass regressed below the edition base",
                            dt,
                            base,
                            cell.coord,
                        );
                        delta_total = delta_total
                            .checked_add(delta.unwrap())
                            .expect("delta_total sum overflowed u32");
                    }
                    let lhs = buffer_total
                        .checked_mul(BUFFER_REPEATER_TICKS)
                        .expect("buffer_total × BUFFER_REPEATER_TICKS overflowed u32");
                    prop_assert_eq!(
                        lhs,
                        delta_total,
                        "buffer block count × BUFFER_REPEATER_TICKS ({}) must equal Σ(delay_ticks − base_delay_ticks) ({}) for scope `{}` with xs={:?}",
                        lhs,
                        delta_total,
                        entry.name,
                        xs,
                    );
                }
            }
        }
    }
}
