//! Edition Netlist IR → Placement IR lowering.
//!
//! Stage 1 of the five-stage pipeline `spec/redstone` "Place-and-route"
//! lays out. Assigns each
//! [`crate::edition_netlist_ir::EditionCellNode`] a
//! [`crate::placement_ir::CellCoord`] inside its scope's
//! [`cairn_lang_core::CircuitRegion`] reservation, and each actuator its
//! output pad.
//!
//! The v1 layout is one row: cells are already in topological order
//! (`NetRef::Cell(j)` in `cells[i]` satisfies `j < i`), so the pass walks
//! them in that order and stamps cell `i` at `x = 1 + 2i`, `y = 0`,
//! `z = 1`. Everything 2D / 2.5D is the routing pass's concern.
//!
//! # Why the row is spaced and one row in
//!
//! A cell body is a block, so a net reaches it through a neighbouring
//! coord, and two nets on one coord or one step apart in one plane are
//! one strand carrying two signals. A two-input gate has three nets
//! touching it and so needs three free neighbours in its own plane; packed
//! at `x = i` an interior cell has two, and no region size gives the
//! third back — the router cannot lift a wire past the face it has to
//! arrive through, and `void` buys height, not room beside a cell. One
//! clear column between cells, one past the last (the actuator pads stand
//! in the column at `width - 1`), and one clear row either side of the
//! cell row are what leave every cell its faces. Enough faces is not a
//! wiring: a net passing through can still take the last one, and stage 2
//! refuses that scope rather than shorting it.
//!
//! Two diagnostic codes join the pass:
//! - [`crate::DiagnosticCode::NoCircuitRegion`] when a scope has cells or
//!   actuator pads to place but no usable `circuit region=` reservation.
//!   Sites always fall here because they carry no `size`.
//! - [`crate::DiagnosticCode::RouteCongestion`] when the netlist does not
//!   fit the reservation, in any of four ways, checked in this order and
//!   each explained in its own terms: the volume (a pessimistic
//!   [`CELL_FOOTPRINT`] per cell, so a placement that fits is unlikely to
//!   flip to a routing failure), the row length, the rows beside the row,
//!   and the rows the I/O pads stand in.
//!
//! Scopes whose placement fires an Error-severity diagnostic are elided
//! from the output (the diagnostic still surfaces), so a downstream pass
//! cannot silently consume a partial layout.

use std::collections::HashMap;

use cairn_lang_core::intent::{self, IntentModule};

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::edition_netlist_ir::{EditionNetlistIr, ScopedEditionNetlistIr};
use crate::logic_ir::ScopeKind;
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacedCellNode, PlacedOutputNode, PlacementIr,
    PlacementPhase, ScopedPlacementIr,
};
use crate::routing_geometry::output_pad;
use crate::saturating_index;

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

/// The `Fix:` footer every area-budget refusal carries, here and in the
/// routing pass's post-routing re-check.
pub(crate) const CONGESTION_FIX: &str =
    "Fix: increase `void`, enlarge region, or split into multiple `circuit` blocks";

/// `used / reserved` to one decimal place, as `(whole, tenths)`, for the
/// congestion primaries. `reserved` must be non-zero.
pub(crate) fn area_ratio_tenths(used: u64, reserved: u64) -> (u64, u64) {
    let ratio_x10 = used.saturating_mul(10) / reserved;
    (ratio_x10 / 10, ratio_x10 % 10)
}

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
/// `module`, which provides the `circuit region=<label> void=<N>`
/// catalogue via [`intent::circuit_regions`].
///
/// One [`PlacementIr`] entry per non-empty [`EditionNetlistIr`] whose
/// placement succeeded; scopes whose placement raises an
/// Error-severity diagnostic are elided so downstream passes cannot
/// consume a partial layout.
#[must_use]
pub fn compile_placement(
    scoped: &ScopedEditionNetlistIr,
    module: &IntentModule,
) -> PlacementOutput {
    let mut out = PlacementOutput::new();
    let region_index = build_region_index(module);

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
    // An identity wire (outputs but no cells) is a layout too: its
    // actuator pad needs the reservation as a cell does. A sensor nothing
    // reads is not. The same predicate the later passes use.
    if source.cells.is_empty() && source.outputs.is_empty() {
        return Ok(PlacementIr::new(source.edition));
    }

    let mut ir = PlacementIr::new(source.edition);
    ir.inputs.clone_from(&source.inputs);
    ir.signal_defs.clone_from(&source.signal_defs);

    let Some(region) = region else {
        return Err(missing_region_diagnostic(source));
    };

    // `saturating_index` only clamps when the scope holds more than
    // `u32::MAX` cells, which no netlist this crate can be handed
    // reaches, so the clamped branch is unreachable today. If it ever
    // is reached, the clamp is not what refuses the scope: the row test
    // below needs `2 * cells + 1` columns and refuses first, whatever
    // the area test makes of `u32::MAX * CELL_FOOTPRINT` (1.7e10, a
    // figure plenty of ordinary reservations clear — `100000x100000`
    // with `void=2` is 2e10).
    let cell_count = saturating_index(source.cells.len());
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
    // The row is `2 * cells + 1` columns, which the area test cannot see:
    // a `size=2x8` scope with `void=3` reserves 48 cells' worth of volume
    // and a row two columns long. Nothing downstream would notice a cell
    // placed past the pad column either.
    let row_columns = u64::from(cell_count)
        .saturating_mul(u64::from(CELL_SPACING))
        .saturating_add(1);
    if row_columns > u64::from(reservation.width) {
        return Err(row_overflow_diagnostic(&reservation, cell_count));
    }
    // A clear row either side of the cell row, whatever the cell count.
    // Only where there is a row: an identity wire has pads and no cells,
    // and the pad check below is what sizes those.
    let row_depth = u64::from(CELL_ROW).saturating_add(2);
    if cell_count > 0 && row_depth > u64::from(reservation.depth) {
        return Err(row_depth_diagnostic(&reservation));
    }
    // `input_pad` / `output_pad` saturate z at `depth - 1`, so below this
    // depth two pads stack on one coord. Pads share a row with a cell
    // without sharing a coord, so the cell row is not added.
    let pad_rows = saturating_index(source.inputs.len().max(source.outputs.len()));
    if pad_rows > reservation.depth {
        return Err(pad_row_diagnostic(&reservation, pad_rows));
    }

    for (index, source_cell) in source.cells.iter().enumerate() {
        let x = saturating_index(index)
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

    // Pads take their coordinate from the geometry the routing pass
    // measures against, so the segment out to an actuator is a placed
    // object rather than something re-derived per pass.
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
    // `reserved_area > 0` by construction: `void=0` is refused and
    // `intent::Size` is `NonZeroU32` on both axes.
    debug_assert!(
        reserved_area > 0,
        "reservation.reserved_area() must be > 0 to compare against required_area",
    );
    let (whole, tenths) = area_ratio_tenths(required_area, reserved_area);
    let primary = format!(
        "synthesized netlist needs ~{whole}.{tenths}x the reserved area (void={void}, region {width}x{depth})",
        void = reservation.void,
        width = reservation.width,
        depth = reservation.depth,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        CONGESTION_FIX,
    )
}

/// The reservation has the volume but not the row.
///
/// Kept apart from [`congestion_diagnostic`] because the numbers that
/// explain it are different — a ratio of areas says nothing about a row
/// that is three columns short — while the code stays
/// [`DiagnosticCode::RouteCongestion`]: `spec/redstone` "Place-and-route"
/// asks for one fail-loud when routing does not fit the region, and names
/// area shortage as the example rather than as the only shape.
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
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        "Fix: widen the enclosing `size=WxH` past twice the cell count, or split into multiple `circuit` blocks. Raising `void` does not help — cells are laid in one row and `void` buys height, not length",
    )
}

/// The reservation has the row but not the rows beside it.
///
/// Kept apart from the two footprint refusals for the reason they are
/// kept apart from each other: the resource is a different one, and the
/// numbers that explain a row with nothing beside it say nothing about
/// a region short of volume. [`DiagnosticCode::RouteCongestion`] is
/// shared with them, per the single fail-loud for "routing does not fit
/// the region" in `spec/redstone` "Place-and-route".
fn row_depth_diagnostic(reservation: &CircuitRegionReservation) -> Diagnostic {
    let primary = format!(
        "synthesized netlist needs {rows} rows for its cell row and a clear row on either side of it, but the reserved region is only {depth} deep (region {width}x{depth}, void={void})",
        rows = u64::from(CELL_ROW).saturating_add(2),
        depth = reservation.depth,
        width = reservation.width,
        void = reservation.void,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        "Fix: deepen the enclosing `size=WxH` to at least three rows, or split into multiple `circuit` blocks. Raising `void` does not help — a wire reaches a cell through a face in the cell's own plane, and `void` buys height above it",
    )
}

/// The reservation has no room for the I/O pads the scope needs.
///
/// Separate from the other three because the resource is a different
/// one again: `void` buys height, the row buys length, the rows beside
/// the row buy the lanes, and this buys the rows the pads stand in. Sharing
/// [`DiagnosticCode::RouteCongestion`] with them keeps the single
/// fail-loud for "routing does not fit the region" in `spec/redstone`
/// "Place-and-route".
fn pad_row_diagnostic(reservation: &CircuitRegionReservation, pad_rows: u32) -> Diagnostic {
    let primary = format!(
        "synthesized netlist needs {needed} rows for its I/O pads but the reserved region is only {depth} deep (region {width}x{depth}, void={void})",
        needed = pad_rows,
        depth = reservation.depth,
        width = reservation.width,
        void = reservation.void,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        "Fix: deepen the enclosing `size=WxH` so the region has one row per sensor or actuator, or split into multiple `circuit` blocks. Raising `void` does not help — pads stand beside the cells, not above them",
    )
}

fn build_region_index(
    module: &IntentModule,
) -> HashMap<(intent::ScopeKind, String), intent::CircuitRegion> {
    let mut index: HashMap<(intent::ScopeKind, String), intent::CircuitRegion> = HashMap::new();
    for region in intent::circuit_regions(module) {
        // Multiple `circuit region=` lines in one scope: first wins,
        // silently — a warning would need a policy no code defines yet.
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
