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
//!   per block from strength 15, so a segment of `s` blocks needs
//!   `floor((s - 1) / DUST_ATTENUATION_LIMIT)` repeaters, each worth
//!   [`BUFFER_REPEATER_TICKS`].
//!
//! A segment is the *routed* path from the net's source to the sink
//! (`route_to`), not the Manhattan distance: the two differ whenever the
//! wire goes round something, and counting against the route is what
//! lets stage 4 put every buffer this stage paid for onto the dust it
//! refreshes. Buffers are counted here and given coords by stage 4; the
//! two agree by construction through `buffer_count_for_segment`.
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
//! longer than v1 will build. Failed scopes are elided so a partial
//! `local_delay_ticks` set never reaches a downstream reader.
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
use crate::routing_geometry::sum_over_driving_nets;

/// Signal-attenuation ceiling per dust segment (`spec/redstone`
/// "Place-and-route" — "signal attenuation limit of 15"). A dust source
/// starts at strength 15 and decays one unit per block, so a segment of
/// at most this many blocks reaches the sink at strength ≥ 1 without a
/// buffer repeater.
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

/// v1 sanity cap on a single driver segment's *routed* length — the
/// dust the signal travels, not the straight line between its ends. A
/// segment longer than this asks for a buffer chain longer than v1
/// will build, so the delay pass refuses with `E_ATTENUATION_LIMIT`
/// rather than count a chain nothing materialises into `local_delay_ticks`.
///
/// 256 blocks is 17 buffer repeaters (`(256 - 1) / 15`); anything past
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
/// `E_NO_CIRCUIT_REGION` that [`crate::pass`] asks on its behalf.
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

    attribute_local_delay_ticks(&mut ir, entry, &|net, sink| nets.segment(net, sink));

    Ok(ir)
}

/// Fill every cell's `local_delay_ticks` with `base_delay(cell) + Σ buffer
/// ticks per driving net`, and every actuator pad's with the buffer
/// ticks on its own segment (a pad is not a cell, so no base delay).
///
/// Per net rather than per driver — see [`sum_over_driving_nets`] — and
/// summed rather than maxed, because the figure is a local wire cost
/// and not the tick the cell's output settles on (see the module doc).
fn attribute_local_delay_ticks<F>(
    ir: &mut PlacementIr,
    entry: &ScopedPlacementIrEntry,
    segment_of: &F,
) where
    F: Fn(NetRef, CellCoord) -> u32,
{
    attribute_nodes(
        ir,
        entry,
        |cell| {
            let buffer_ticks = sum_over_driving_nets(&cell.drivers, |net| {
                buffer_repeater_ticks_for_segment(segment_of(net, cell.coord))
            });
            cell.cell.base_delay_ticks().saturating_add(buffer_ticks)
        },
        |output| buffer_repeater_ticks_for_segment(segment_of(output.driver, output.pad)),
        |phase, ticks, identity| phase.delay_at(ticks, identity),
    );
}

/// Buffer repeaters needed to keep `segment` blocks of dust at
/// strength ≥ 1 at the sink.
///
/// A source at strength 15 loses one unit per block, so segments of
/// at most `DUST_ATTENUATION_LIMIT` blocks reach the sink without a
/// buffer; a 16-block segment needs one; each further
/// `DUST_ATTENUATION_LIMIT` blocks bumps the count by one. Saturating
/// so a pathological segment from a hand-built IR cannot overflow.
///
/// `crate::crossing` calls this with the same routed length to decide
/// how many coords to materialise, so the ticks this pass writes and
/// the `buffer_coords` stage 4 emits are one number by construction.
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

fn attenuation_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    cell_index: usize,
    driver_index: usize,
    segment: u32,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` has a driver segment of {segment} blocks into cell #{cell_index} port #{driver_index} — exceeds the v1 attenuation limit of {cap} blocks (dust decays 1/block, so this segment would need {buffers} buffer repeaters to materialize)",
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
        "routed netlist for {kind} `{name}` has a driver segment of {segment} blocks into output pad #{output_index} — exceeds the v1 attenuation limit of {cap} blocks (dust decays 1/block, so this segment would need {buffers} buffer repeaters to materialize)",
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

    use cairn_lang_core::Edition;
    use cairn_lang_core::error::Span;

    use super::{
        BUFFER_REPEATER_TICKS, DUST_ATTENUATION_LIMIT, MAX_ATTENUATION_SEGMENT,
        attribute_local_delay_ticks, buffer_count_for_segment, buffer_repeater_ticks_for_segment,
        compile_delay,
    };
    use crate::diagnostic::DiagnosticCode;
    use crate::edition_netlist_ir::EditionCell;
    use crate::logic_ir::ScopeKind;
    use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
    use crate::placement_ir::{
        CellCoord, PlacedCellNode, PlacementIr, PlacementPhase, ScopedPlacementIrEntry,
    };
    use crate::test_fixtures::{reservation, scoped};

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
    /// Calls the fold directly with a stand-in `segment_of` rather
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
        // reaches the nets only through the stand-in `segment_of`, so
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
        let segment_of = |net: NetRef, _sink: CellCoord| -> u32 {
            match net {
                NetRef::Input(0) => SHORT,
                NetRef::Input(1) => LONG,
                other => panic!("no other net drives this cell: {other:?}"),
            }
        };
        attribute_local_delay_ticks(&mut ir, &entry, &segment_of);

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

    #[test]
    fn cell_driver_attenuation_primary_names_cell_and_port() {
        // Two cells wide-spread inside a `size=300x3` reservation so
        // the cell[1] driver from cell[0] spans a routed segment
        // over `MAX_ATTENUATION_SEGMENT`. Only reachable by hand-built
        // IR — the placement pass lays cells at `x = topological
        // index`, so producing this shape from a `.crn` would need a
        // 258-cell chain.
        let mut ir = PlacementIr::new(Edition::Java);
        ir.region = Some(reservation(300, 3, 3));
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(0, 0, 0),
            vec![],
        ));
        ir.cells.push(placed_cell(
            EditionCell::JavaComparatorAnd,
            CellCoord::new(299, 0, 0),
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
            attenuation.primary.contains("into cell #1"),
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
