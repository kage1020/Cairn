//! Edition Netlist IR → Placement IR lowering.
//!
//! Stage 1 of the five-stage place-and-route pipeline `spec/redstone`
//! §14.5 lays out (Placement → Steiner routing → Delay insertion →
//! Crossing legalization → Edition legalization). Assigns each
//! [`crate::edition_netlist_ir::EditionCellNode`] a 1D
//! [`crate::placement_ir::CellCoord`] inside its scope's
//! [`cairn_lang_core::CircuitRegion`] reservation.
//!
//! The v1 algorithm is deliberately minimal: cells are already in
//! topological order (`NetRef::Cell(j)` in `cells[i]` satisfies
//! `j < i`, an invariant [`crate::netlist::compile_netlist`] carries
//! across from the Logic IR), so this pass walks them in that order
//! and stamps one row, `y = 0`, `z = 1`. The 2D / 2.5D lift is the
//! routing pass's concern — it needs the `plane` / `via` / `bridge`
//! escape hatches §14.5 mentions for fanout and for one net getting
//! past another.
//!
//! # Why the row is spaced
//!
//! Cell `i` stands at `x = 1 + 2i`: one column in from the pad column
//! at `x = 0`, one clear column between each pair, and one past the
//! last of them.
//!
//! A cell body is a block, so a net reaches it through a neighbouring
//! coord — dust does not pass through a component, and two nets that
//! share a coord, or run one step apart in one plane, are one strand
//! carrying two signals. A two-input gate has three distinct nets
//! touching it, its two drivers and its own output, so it needs three
//! free neighbours; any two neighbours of one block are two steps
//! apart, so three faces are three arrivals and not a short. Packed at
//! `x = i` against the pad column, an interior cell of a chain has
//! two: the cells on either side take the other faces, and no region
//! size gives them back. Spacing the row is the only thing that can —
//! not the router, which cannot lift a wire past the face it has to
//! arrive through, and not `void`, which buys height above a cell and
//! not room beside it.
//!
//! The column past the end is the same argument at the other end of
//! the row. `output_pad` puts the actuator pads down the column at
//! `width - 1`, so a last cell standing there has that column's pad on
//! one side, the edge of the reservation on the other, and one clear
//! column left. Under `void=1` that is one face for two nets and the
//! scope is refused two stages later; above it the last net climbs and
//! pays for the climb. Neither is what the row check is for, so the
//! row it measures is `cells * 2 + 1` columns long.
//!
//! # Why the row is one row in
//!
//! The cells stand on `z = 1`, not on the near edge of the
//! reservation, and `input_pad` / `output_pad` step along `z` from `0`
//! — the row they used to start below is the row the cells have left.
//!
//! Dust reads the dust beside it, so a lane of free coords carries one
//! net however long the lane is. A cell on `z = 0` has one lane: the
//! row at `z = 1`, which is the only row its faces open onto that no
//! other cell of the row stands in. Three nets touch a two-input gate
//! and they cannot share it. The two-gate `crossbar` example is the
//! case — with the row on the edge, no order its four nets can be laid
//! in wires the scope, and widening or deepening the reservation does
//! not change that; one row in, it wires at `5x3` with `void=2`.
//!
//! One row for the whole netlist rather than one per cell, so unlike
//! the column spacing this does not grow with the cell count: the
//! reservation needs three rows, the cells' and a clear one either
//! side, whatever is placed in it.
//!
//! Enough faces is not the same as a wiring: a net passing through can
//! still take the last one, and stage 2 refuses that scope rather than
//! shorting it.
//!
//! Two diagnostic codes join the pass:
//! - [`crate::DiagnosticCode::NoCircuitRegion`] when a scope has cells
//!   or actuator pads to place but the enclosing struct / def declared no
//!   `circuit region=` line (or no `size=WxH` header for the region to
//!   sit inside). Sites always fall here because they carry no `size`.
//! - [`crate::DiagnosticCode::RouteCongestion`] when the netlist does
//!   not fit the reservation, which it can fail to do in four ways.
//!   The volume can be short: the v1 area budget uses
//!   [`CELL_FOOTPRINT`] as a per-cell footprint estimate, deliberately
//!   pessimistic so a placement that reports "fits" is unlikely to
//!   flip to a routing failure downstream. Or the *row* can be short,
//!   which the area budget cannot see — a `size=2x8` scope with
//!   `void=3` reserves 48 cells' worth of volume and two columns of
//!   row. Or the region can be too shallow for the row to have a clear
//!   row either side of it. Or too shallow for the I/O pads, which
//!   stand one per row down the two edge columns. All four are
//!   checked, in that order, and each explains itself in its own
//!   terms. Follow-up refinement is `#[non_exhaustive]`-safe on both
//!   types.
//!
//! Scopes whose placement fires an Error-severity diagnostic are
//! elided from the output list (the diagnostic still surfaces), so a
//! downstream pass cannot silently consume a partial layout — the
//! same fail-loud policy [`crate::synth::synthesize`]'s cascade
//! suppression uses on unbound signals.

use std::collections::HashMap;

use cairn_lang_core::check::Severity;
use cairn_lang_core::intent::{self, IntentModule};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::edition_netlist_ir::{EditionNetlistIr, ScopedEditionNetlistIr};
use crate::logic_ir::ScopeKind;
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacedCellNode, PlacedOutputNode, PlacementIr,
    PlacementPhase, ScopedPlacementIr,
};
use crate::routing_geometry::output_pad;

/// Per-cell footprint used by the v1 congestion estimate. Four blocks
/// covers a two-input gate's cell plus its short input tails, and is
/// deliberately pessimistic so a placement that reports "fits" almost
/// never flips to a routing failure downstream. A future revision that
/// distinguishes `Not` / `Or` / `And` footprints (or reads the
/// per-tile size from the physical tile catalogue) is a value change,
/// not a schema change.
pub const CELL_FOOTPRINT: u32 = 4;

/// Columns the row spends per cell: the cell's own, and the clear one
/// beside it.
///
/// The spacing is what leaves a two-input gate a free neighbour for
/// each of the three nets that touch it — see the module doc. Read
/// here by the row-length refusal and by the coordinate it refuses on
/// behalf of, so the two cannot drift. The refusal adds one more
/// column for the end of the row; that one is not per cell, so it is
/// not folded in here.
const CELL_SPACING: u32 = 2;

/// The row the cells stand on, counted from the near edge of the
/// reservation.
///
/// One row in, so every cell has a clear lane on each side of it rather
/// than only the one — see the module doc. Read here by the coordinate
/// and by the depth refusal that reserves the rows it needs, so the two
/// cannot drift.
const CELL_ROW: u32 = 1;

/// Output of a [`compile_placement`] run.
///
/// Diagnostics are surfaced separately from the IR so a caller can
/// render every finding even when the IR itself is empty (for example
/// when every scope failed congestion). Matches the shape
/// [`crate::synth::SynthOutput`] uses at the top of the pipeline.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct PlacementOutput {
    /// Placement IR for every scope that placed successfully.
    pub scoped: ScopedPlacementIr,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

impl PlacementOutput {
    /// Empty output (no placed scopes, no diagnostics).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lower a [`ScopedEditionNetlistIr`] to a [`ScopedPlacementIr`] against
/// `intent`. `intent` provides the `circuit region=<label> void=<N>`
/// catalogue via [`intent::circuit_regions`].
///
/// One [`PlacementIr`] entry per non-empty [`EditionNetlistIr`] whose
/// placement succeeded; scopes whose placement raises an
/// Error-severity diagnostic are elided so downstream passes cannot
/// consume a partial layout.
#[must_use]
pub fn compile_placement(
    scoped: &ScopedEditionNetlistIr,
    intent: &IntentModule,
) -> PlacementOutput {
    let mut out = PlacementOutput::new();
    let region_index = build_region_index(intent);

    for entry in &scoped.scopes {
        let key = (map_scope_kind(entry.kind), entry.name.clone());
        let region = region_index.get(&key);
        match compile_scope(&entry.ir, region) {
            Ok(ir) => out.scoped.push(entry.kind, entry.name.clone(), ir),
            Err(diagnostic) => out.diagnostics.push(diagnostic),
        }
    }

    out
}

/// Result of placing one scope: the placed IR on success, a single
/// Error-severity diagnostic on failure.
type ScopePlacement = Result<PlacementIr, Diagnostic>;

fn compile_scope(
    source: &EditionNetlistIr,
    region: Option<&intent::CircuitRegion>,
) -> ScopePlacement {
    // An identity-wire scope — inputs and outputs but zero cells, e.g. a
    // `pressure_plate -> sig.a` bound straight to `door opened_by=sig.a`
    // with no `logic` line between them — is a layout too. `spec/redstone`
    // §14.2 permits the direct binding, and across a wide footprint that
    // wire needs the same buffer repeaters any other wire does. What it
    // has to place is the actuator pad, which needs the reservation
    // exactly as a cell does.
    //
    // A sensor nothing reads is the other cell-less shape, and it is not
    // a layout: there is no wire, because nothing is on the other end.
    // The predicate is the one the routing, delay, and crossing passes
    // already spell out — anything else and placement disagrees with its
    // own siblings about what there is to place.
    if source.cells.is_empty() && source.outputs.is_empty() {
        return Ok(PlacementIr::new(source.edition));
    }

    let mut ir = PlacementIr::new(source.edition);
    ir.inputs.clone_from(&source.inputs);
    ir.signal_defs.clone_from(&source.signal_defs);

    let Some(region) = region else {
        return Err(missing_region_diagnostic(source));
    };

    // Clamp on `u32::MAX` mirrors [`crate::netlist`]'s `safe_index`: a
    // `.crn` big enough to overflow `u32` is well past any Cairn build
    // the compiler will practically finish, so saturate rather than
    // panic. On saturation, `required_area` is `u32::MAX * CELL_FOOTPRINT`
    // in `u64`, which is guaranteed larger than any legitimate
    // `reserved_area` (`u32^3` at most), so congestion still fires.
    let cell_count = u32::try_from(source.cells.len()).unwrap_or(u32::MAX);
    let required_area = u64::from(cell_count) * u64::from(CELL_FOOTPRINT);
    let reservation = CircuitRegionReservation {
        label: region.label.clone(),
        void: region.void,
        width: region.width,
        depth: region.depth,
        span: region.span.clone(),
    };
    if required_area > reservation.reserved_area() {
        return Err(congestion_diagnostic(&reservation, required_area));
    }
    // The v1 layout is a single spaced row: cell `i` stands at
    // `x = 1 + 2i`, so the last one sits at `2 * cells - 1` and the row
    // wants a column past it as well — `2 * cells + 1` in all, for the
    // reason the module doc gives. The area test above cannot see that.
    // A `size=2x8` scope with `void=3` reserves 48 cells' worth of
    // volume and offers a row two columns long, and a three-cell
    // netlist passes the first and overruns the second.
    //
    // Nothing downstream would notice either: every later pass reads the
    // coordinates this one stamps, and `routing_geometry::output_pad`
    // puts the actuator pad at `width - 1`. A cell past that column sits
    // to the right of the pad it drives, so the wire runs backwards out
    // of the reservation the author declared — and a cell outside the
    // region entirely is a sink the router cannot reach, which would
    // surface two passes later as a congestion refusal saying every
    // route runs through a component, of a coord no route could enter.
    let row_columns = u64::from(cell_count)
        .saturating_mul(u64::from(CELL_SPACING))
        .saturating_add(1);
    if row_columns > u64::from(reservation.width) {
        return Err(row_overflow_diagnostic(&reservation, cell_count));
    }
    // The row needs a clear row on either side of it, for the reason the
    // module doc gives: a cell against the edge of the reservation has
    // one lane beside it, and dust reads the dust beside it, so one lane
    // carries one net. `CELL_ROW` rows stand before the cells, the cells
    // take one, and one more has to be clear behind them. Unlike the row
    // length this does not grow with the netlist — it is the same three
    // rows for one cell as for a hundred.
    let row_depth = u64::from(CELL_ROW).saturating_add(2);
    if row_depth > u64::from(reservation.depth) {
        return Err(row_depth_diagnostic(&reservation));
    }
    // The pads need rows of their own. `input_pad` and `output_pad` step
    // along z from 0 and saturate at `depth - 1`, so a reservation holds
    // its I/O only while `depth` is at least the larger of the two pad
    // counts; below that the saturation stacks pads on one coord. Rows,
    // not rows past the cell row — a pad stands in the column at `x = 0`
    // or `x = width - 1`, which no cell occupies, so a pad and a cell
    // share a row without sharing a coord. Refused here rather than left
    // to the routing pass's occupancy sweep so stage 1 stops emitting a
    // dump whose coordinates contradict each other.
    let pad_rows = source.inputs.len().max(source.outputs.len());
    let pad_rows = u32::try_from(pad_rows).unwrap_or(u32::MAX);
    if pad_rows > reservation.depth {
        return Err(pad_row_diagnostic(&reservation, pad_rows));
    }

    for (index, source_cell) in source.cells.iter().enumerate() {
        // Same saturating-cast rationale as `cell_count` above: a
        // `.crn` big enough to overflow `u32` cannot practically
        // finish compilation. The row-length refusal above has already
        // turned any width this could overrun into a diagnostic.
        let x = u32::try_from(index)
            .unwrap_or(u32::MAX)
            .saturating_mul(CELL_SPACING)
            .saturating_add(1);
        ir.cells.push(PlacedCellNode {
            cell: source_cell.cell,
            drivers: source_cell.drivers.clone(),
            coord: CellCoord::new(x, 0, CELL_ROW),
            phase: PlacementPhase::Unrouted,
            span: source_cell.span.clone(),
        });
    }
    debug_assert!(
        ir.cells.iter().all(|c| c.cell.edition() == source.edition),
        "compile_scope placed a cell whose edition tag disagrees with the container's",
    );

    // Actuator pads take their coordinate from the same geometry the
    // routing pass measures against, so the segment out to an actuator
    // is a placed object rather than something re-derived per pass.
    for (index, source_output) in source.outputs.iter().enumerate() {
        ir.outputs.push(PlacedOutputNode::new(
            source_output.name.clone(),
            source_output.driver,
            output_pad(index, &reservation),
            source_output.span.clone(),
        ));
    }

    ir.region = Some(reservation);
    Ok(ir)
}

fn missing_region_diagnostic(source: &EditionNetlistIr) -> Diagnostic {
    // An identity-wire scope has no cell to point at, so fall through to
    // the actuator binding that made the scope need a reservation in the
    // first place. A default span would render the finding at byte 0,
    // which for the one scope shape that reaches here without a cell is
    // every time.
    let span = source
        .cells
        .first()
        .map(|c| c.span.clone())
        .or_else(|| source.outputs.first().map(|o| o.span.clone()))
        .unwrap_or_default();
    Diagnostic::new(
        DiagnosticCode::NoCircuitRegion,
        span,
        "this scope has redstone cells or actuator pads to place but no usable `circuit region=<label> void=<N>` reservation is in scope (missing line, malformed `region=` / `void=`, or the enclosing scope has no `size=WxH` header)"
            .to_owned(),
    )
    .with_footer(
        "add a `circuit region=<label> void=<N>` line whose `region=` is a non-empty label naming the reservation (`region=floor`, `region=basement` — the name is the author's to choose and is echoed back in diagnostics) and whose `void=` is an integer >= 1, and give the enclosing scope a `size=WxH` header",
    )
}

fn congestion_diagnostic(reservation: &CircuitRegionReservation, required_area: u64) -> Diagnostic {
    let reserved_area = reservation.reserved_area();
    // `reserved_area > 0` by construction: `parse_circuit_region_fixture`
    // rejects `void=0`, and `intent::Size` guarantees `NonZeroU32` for
    // width and height. If a future change violates either, the debug
    // assertion catches it before the division underflows the ratio.
    debug_assert!(
        reserved_area > 0,
        "reservation.reserved_area() must be > 0 to compare against required_area",
    );
    let ratio_x10 = (required_area * 10) / reserved_area;
    let whole = ratio_x10 / 10;
    let tenths = ratio_x10 % 10;
    let primary = format!(
        "synthesized netlist needs ~{whole}.{tenths}x the reserved area (void={void}, region {width}x{depth})",
        void = reservation.void,
        width = reservation.width,
        depth = reservation.depth,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: increase `void`, enlarge region, or split into multiple `circuit` blocks",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

/// The reservation has the volume but not the row.
///
/// Kept apart from [`congestion_diagnostic`] because the numbers that
/// explain it are different — a ratio of areas says nothing about a row
/// that is three columns short — while the code stays
/// [`DiagnosticCode::RouteCongestion`]: `spec/redstone` §14.5 asks for
/// one fail-loud when routing does not fit the region, and names area
/// shortage as the example rather than as the only shape.
fn row_overflow_diagnostic(reservation: &CircuitRegionReservation, cell_count: u32) -> Diagnostic {
    let primary = format!(
        "synthesized netlist needs {columns} columns for a row of {cell_count} cells, a clear column beside each and one past the end of the row, but the reserved region is only {width} wide (region {width}x{depth}, void={void})",
        columns = u64::from(cell_count)
            .saturating_mul(u64::from(CELL_SPACING))
            .saturating_add(1),
        width = reservation.width,
        depth = reservation.depth,
        void = reservation.void,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: widen the enclosing `size=WxH` past twice the cell count, or split into multiple `circuit` blocks. Raising `void` does not help — cells are laid in one row and `void` buys height, not length",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

/// The reservation has the row but not the rows beside it.
///
/// Kept apart from the two footprint refusals for the reason they are
/// kept apart from each other: the resource is a different one, and the
/// numbers that explain a row with nothing beside it say nothing about
/// a region short of volume. [`DiagnosticCode::RouteCongestion`] is
/// shared with them, per `spec/redstone` §14.5's single fail-loud for
/// "routing does not fit the region".
fn row_depth_diagnostic(reservation: &CircuitRegionReservation) -> Diagnostic {
    let primary = format!(
        "synthesized netlist needs {rows} rows for its cell row and a clear row on either side of it, but the reserved region is only {depth} deep (region {width}x{depth}, void={void})",
        rows = u64::from(CELL_ROW).saturating_add(2),
        depth = reservation.depth,
        width = reservation.width,
        void = reservation.void,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: deepen the enclosing `size=WxH` to at least three rows, or split into multiple `circuit` blocks. Raising `void` does not help — a wire reaches a cell through a face in the cell's own plane, and `void` buys height above it",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

/// The reservation has no room for the I/O pads the scope needs.
///
/// Separate from the other three because the resource is a different
/// one again: `void` buys height, the row buys length, the rows beside
/// the row buy the lanes, and this buys the rows the pads stand in. Sharing
/// [`DiagnosticCode::RouteCongestion`] with them keeps `spec/redstone`
/// §14.5's single fail-loud for "routing does not fit the region".
fn pad_row_diagnostic(reservation: &CircuitRegionReservation, pad_rows: u32) -> Diagnostic {
    let primary = format!(
        "synthesized netlist needs {needed} rows for its I/O pads but the reserved region is only {depth} deep (region {width}x{depth}, void={void})",
        needed = pad_rows,
        depth = reservation.depth,
        width = reservation.width,
        void = reservation.void,
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: deepen the enclosing `size=WxH` so the region has one row per sensor or actuator, or split into multiple `circuit` blocks. Raising `void` does not help — pads stand beside the cells, not above them",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

fn build_region_index(
    intent: &IntentModule,
) -> HashMap<(intent::ScopeKind, String), intent::CircuitRegion> {
    let mut index: HashMap<(intent::ScopeKind, String), intent::CircuitRegion> = HashMap::new();
    for region in intent::circuit_regions(intent) {
        // Multiple `circuit region=` lines in one scope: first wins.
        // v1 stays silent because a warning would need a policy
        // decision (`W_MULTIPLE_CIRCUIT_REGIONS` is not defined
        // anywhere yet), and the routing pass — which is where
        // `spec/redstone` §14.5's "split into multiple `circuit`
        // blocks" fix hint actually matters — has not landed. A
        // follow-up PR that either adds a warning here or teaches
        // routing to consume every reservation is a
        // `#[non_exhaustive]`-safe extension.
        let key = (region.scope_kind, region.scope_name.clone());
        index.entry(key).or_insert(region);
    }
    index
}

fn map_scope_kind(kind: ScopeKind) -> intent::ScopeKind {
    match kind {
        ScopeKind::Struct => intent::ScopeKind::Struct,
        ScopeKind::Def => intent::ScopeKind::Def,
        ScopeKind::Site => intent::ScopeKind::Site,
    }
}
