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
//! clear column between cells and one clear row either side of the cell
//! row are what leave every cell its faces.
//!
//! The pad columns stand at `x = 0` and `x = width - 1`. The first
//! cell's outer face is always the input-pad column; the last cell's is
//! the actuator-pad column at the exact `2n + 1` fit, and a clear
//! column inside the region at any width past it. The pads keep off the
//! cells two ways. They step over the cell row
//! (`grid::PadColumn`), so the coord beside an end cell in
//! a pad column holds no pad. And the row check demands `2n + 1`
//! columns rather than `2n`, because at `2n` the last cell would stand
//! at `x = width - 1`, inside the actuator-pad column, face to face
//! with the pads at `z = 0` and `z = 2`. Together they leave no pad
//! sharing a face with a cell at any width the row check accepts.
//!
//! Enough faces is not a wiring: a net passing through can still take
//! the last one, and stage 2 refuses that scope rather than shorting it.
//!
//! Two diagnostic codes join the pass:
//! - [`crate::DiagnosticCode::NoCircuitRegion`] when a scope has cells or
//!   actuator pads to place but no usable `circuit region=` reservation:
//!   on the scope's first `circuit` line that reserves nothing, with the
//!   reason, or, when the scope has no `circuit` line the walk finds, on
//!   what needed the reservation. A site is always the second kind:
//!   [`intent::circuit_lines`] reads `module.structs` and `module.defs`
//!   only, and an [`intent::SiteIr`] has no `size` and no `members`.
//! - [`crate::DiagnosticCode::RouteCongestion`] when the netlist does not
//!   fit the reservation, in any of five ways, checked in this order and
//!   each explained in its own terms: the volume (a pessimistic
//!   [`CELL_FOOTPRINT`] per cell, so a placement that fits is unlikely to
//!   flip to a routing failure), the row length, the two pad columns,
//!   the rows beside the row, and the rows the I/O pads stand in.
//!
//! Scopes whose placement fires an Error-severity diagnostic are elided
//! from the output (the diagnostic still surfaces), so a downstream pass
//! cannot silently consume a partial layout.

use std::collections::HashMap;

use cairn_lang_core::intent::{self, IntentModule};

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::edition_netlist_ir::{EditionNetlistIr, ScopedEditionNetlistIr};
use crate::grid::{CELL_ROW, PadColumn, output_pad};
use crate::logic_ir::ScopeKind;
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacedCellNode, PlacedOutputNode, PlacementIr,
    PlacementPhase, ScopedPlacementIr,
};
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

/// The `Fix:` footer every area-budget refusal carries, here and in the
/// routing pass's post-routing re-check.
pub(crate) const CONGESTION_FIX: &str = "Fix: increase `void`, enlarge region, or split the logic across several scopes, each with its own `circuit` line";

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
/// catalogue via [`intent::circuit_lines`].
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
    // One walk of the `circuit` lines, and both indexes read from it, so
    // the two cannot disagree about which line is usable.
    let lines = intent::circuit_lines(module);
    let region_index = build_region_index(&lines);
    let rejected_index = build_rejected_index(&lines);

    for entry in &scoped.scopes {
        let key = (map_scope_kind(entry.kind), entry.name.clone());
        // The rejected index is asked only when the region index has no
        // entry, so a usable line wins over a rejected one in the same
        // scope (see `build_rejected_index`).
        let region = match (region_index.get(&key), rejected_index.get(&key)) {
            (Some(&region), _) => ScopeRegion::Reserved(region),
            (None, Some(&rejected)) => ScopeRegion::Rejected(rejected),
            (None, None) => ScopeRegion::Absent,
        };
        match compile_scope(entry.kind, &entry.ir, region) {
            Ok(ir) => out.scoped.push(entry.kind, entry.name.clone(), ir),
            Err(diagnostic) => out.diagnostics.push(diagnostic),
        }
    }

    out
}

/// Result of placing one scope: the placed IR on success, a single
/// Error-severity diagnostic on failure.
type ScopePlacement = Result<PlacementIr, Diagnostic>;

/// What one scope's `circuit` lines come to, as [`compile_placement`]
/// reads them out of [`intent::circuit_lines`].
///
/// Three answers, none of them an error, so a named type rather than a
/// `Result` nested around an `Option`: [`compile_scope`] matches all
/// three, and each failing one picks its own diagnostic.
#[derive(Clone, Copy)]
enum ScopeRegion<'a> {
    /// The scope's first usable `circuit` line.
    Reserved(&'a intent::CircuitRegion),
    /// No usable line; the scope's first `circuit` line that reserves
    /// nothing.
    Rejected(&'a intent::RejectedCircuitRegion),
    /// No `circuit` line the walk found for the scope: none at its top
    /// level or under a `level`, or the scope is a site, which the walk
    /// does not read.
    Absent,
}

fn compile_scope(
    kind: ScopeKind,
    source: &EditionNetlistIr,
    region: ScopeRegion<'_>,
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

    let region = match region {
        ScopeRegion::Reserved(region) => region,
        ScopeRegion::Rejected(rejected) => return Err(rejected_region_diagnostic(rejected)),
        ScopeRegion::Absent => return Err(missing_region_diagnostic(kind, source)),
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
    // The sensor pads stand in column `x = 0` and the actuator pads in
    // `x = width - 1`. A scope with cells has already been held to
    // three columns by the row; an identity wire has no row, and at one
    // column its two pad columns are the same one.
    if !source.inputs.is_empty() && !source.outputs.is_empty() && reservation.width < 2 {
        return Err(pad_column_diagnostic(&reservation));
    }
    // A clear row either side of the cell row, whatever the cell count.
    // Only where there is a row: an identity wire has pads and no cells,
    // and the pad check below is what sizes those.
    let row_depth = u64::from(CELL_ROW).saturating_add(2);
    if cell_count > 0 && row_depth > u64::from(reservation.depth) {
        return Err(row_depth_diagnostic(&reservation));
    }
    // `input_pad` / `output_pad` saturate z at `depth - 1`, so below
    // this depth two pads stack on one coord. A scope with cells has its
    // pads step over the cell row, which makes it one of the rows they
    // span once an edge carries two; one without has no row to step
    // over. `column` is that decision, and the pads below are laid by
    // the same value.
    let column = PadColumn::for_cell_count(source.cells.len());
    let required_pad_rows = column.rows(source.inputs.len().max(source.outputs.len()));
    if required_pad_rows > reservation.depth {
        return Err(pad_row_diagnostic(&reservation, column, required_pad_rows));
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
            output_pad(index, column, &reservation),
            source_output.span.clone(),
        ));
    }

    ir.region = Some(reservation);
    Ok(ir)
}

/// `E_NO_CIRCUIT_REGION` for a scope with something to place and no
/// `circuit` line [`intent::circuit_lines`] found for it.
///
/// A scope whose `circuit` line reserves nothing does not come here:
/// [`build_rejected_index`] holds that line, read out of the same walk
/// as the reservations, and [`rejected_region_diagnostic`] reports on
/// it. Two kinds of scope do:
///
/// - a `struct` or `def` with no `circuit` line at its top level or
///   under a `level`;
/// - a `site`, whatever it says. The walk reads `module.structs` and
///   `module.defs` only, and an [`intent::SiteIr`] has no `size` and no
///   `members`, so a site has nothing to reserve within. Its message
///   says that rather than asking for a `size=WxH` header a site cannot
///   have.
///
/// Anchored on what needed the reservation.
fn missing_region_diagnostic(kind: ScopeKind, source: &EditionNetlistIr) -> Diagnostic {
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
    let (primary, fix) = match kind {
        ScopeKind::Struct | ScopeKind::Def => (
            "this scope has redstone cells or actuator pads to place but no `circuit` line at its top level to reserve room for them",
            "Fix: add a `circuit region=<label> void=<N>` line at the top level of the scope, with a non-empty label naming the reservation (`region=floor`, `region=basement`) and an integer `void=` >= 1. It reserves within the footprint the scope's `size=WxH` header declares",
        ),
        ScopeKind::Site => (
            "this `site` has redstone cells or actuator pads to place, and a `site` has no `size=WxH` header to reserve room for them within",
            "Fix: move the redstone into a `struct` or `def` with a `size=WxH` header and a `circuit region=<label> void=<N>` line at its top level",
        ),
    };
    error_with_footer(
        DiagnosticCode::NoCircuitRegion,
        span,
        primary.to_owned(),
        fix,
    )
}

/// `E_NO_CIRCUIT_REGION` on the `circuit` line that reserves nothing.
///
/// The primary carries [`intent::CircuitRegionDefect`]'s reason clause,
/// so what is wrong with the line is worded once, beside the type, in
/// core. The match here only picks the `Fix:` line, the repair this pass
/// can offer: one arm per defect, and a `_` arm, which the type's
/// `#[non_exhaustive]` asks for, for one added in core before this pass
/// has a repair for it.
fn rejected_region_diagnostic(rejected: &intent::RejectedCircuitRegion) -> Diagnostic {
    use intent::CircuitRegionDefect as Defect;
    let fix = match &rejected.defect {
        Defect::NestedUnderLevel { .. } => "Fix: move the `circuit` line out of the `level` to the scope's top level. That changes nothing else about it: a reservation spans the scope's `size=WxH`, and no pass reads a `level`'s `y=` for a `circuit` line".to_owned(),
        Defect::NoSize { .. } => "Fix: give the enclosing scope a `size=WxH` header".to_owned(),
        Defect::RegionMissing { .. } => {
            "Fix: add `region=<label>` naming the reservation (`region=floor`, `region=basement`)"
                .to_owned()
        }
        Defect::RegionNotLabel { .. } => {
            "Fix: write `region=` as an identifier or a string (`region=floor`, `region=basement`)"
                .to_owned()
        }
        Defect::RegionEmpty { .. } => {
            "Fix: give `region=` a non-empty label (`region=floor`, `region=basement`)".to_owned()
        }
        Defect::VoidMissing { .. } => {
            "Fix: add `void=<N>`, the height of the service layer, an integer >= 1".to_owned()
        }
        Defect::VoidNotInteger { .. } => "Fix: write `void=` as an integer >= 1".to_owned(),
        Defect::VoidBelowOne { .. } => "Fix: set `void=` to an integer >= 1".to_owned(),
        Defect::VoidTooLarge { .. } => format!(
            "Fix: set `void=` to an integer from 1 to {limit}",
            limit = u32::MAX,
        ),
        _ => "Fix: write the line as `circuit region=<label> void=<N>` at the top level of a `struct` or `def` with a `size=WxH` header, with a non-empty label and an integer `void=` >= 1".to_owned(),
    };
    error_with_footer(
        DiagnosticCode::NoCircuitRegion,
        rejected.span.clone(),
        format!(
            "this `circuit` line reserves no room for the scope's redstone cells or actuator pads: {reason}",
            reason = rejected.defect,
        ),
        fix,
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
/// The row is `2n + 1` columns: a cell and a clear column per cell from
/// `x = 1`, ending before the actuator-pad column at `x = width - 1`.
/// At `2n` the last cell would stand in that column, face to face with
/// the pads at `z = 0` and `z = 2`.
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
        "Fix: widen the enclosing `size=WxH` past twice the cell count, or split the logic across several scopes, each with its own `circuit` line. Raising `void` does not help — cells are laid in one row and `void` buys height, not length",
    )
}

/// The reservation is one column wide, and the scope has pads on both
/// edges.
///
/// The sensor pads stand in column `x = 0` and the actuator pads in
/// `x = width - 1`; at one column those are the same column, and the
/// first sensor and the first actuator collide at `(0,0,0)` at any
/// depth. Kept apart from the other refusals for their reason: the
/// numbers that explain it are columns, not area or rows. Shares
/// [`DiagnosticCode::RouteCongestion`] with them, per the single
/// fail-loud for "routing does not fit the region" in `spec/redstone`
/// "Place-and-route".
fn pad_column_diagnostic(reservation: &CircuitRegionReservation) -> Diagnostic {
    let primary = format!(
        "synthesized netlist needs 2 columns, one for its sensor pads at `x = 0` and one for its actuator pads at `x = width - 1`, but the reserved region is only {width} wide, so the two are one column (region {width}x{depth}, void={void})",
        width = reservation.width,
        depth = reservation.depth,
        void = reservation.void,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        "Fix: widen the enclosing `size=WxH` to at least two columns. Raising `void` or deepening the region does not help — the pads collide in that one column at any height or depth",
    )
}

/// The reservation has the row but not the rows beside it.
///
/// Kept apart from the refusals checked before it for the reason they
/// are kept apart from each other: the resource is a different one, and
/// the numbers that explain a row with nothing beside it say nothing
/// about a region short of volume. [`DiagnosticCode::RouteCongestion`] is
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
        "Fix: deepen the enclosing `size=WxH` to at least three rows, or split the logic across several scopes, each with its own `circuit` line. Raising `void` does not help — a wire reaches a cell through a face in the cell's own plane, and `void` buys height above it",
    )
}

/// The reservation has no room for the I/O pads the scope needs.
///
/// Separate from the other footprint refusals because the resource is a
/// different one again: `void` buys height, the row buys length, a
/// second column buys the sensor and actuator pads a column each, the
/// rows beside the row buy the lanes, and this buys the rows the pads
/// stand in. Sharing [`DiagnosticCode::RouteCongestion`] with them keeps
/// the single fail-loud for "routing does not fit the region" in
/// `spec/redstone` "Place-and-route".
fn pad_row_diagnostic(
    reservation: &CircuitRegionReservation,
    column: PadColumn,
    required_pad_rows: u32,
) -> Diagnostic {
    let primary = format!(
        "synthesized netlist needs {required_pad_rows} rows for its I/O pads but the reserved region is only {depth} deep (region {width}x{depth}, void={void})",
        depth = reservation.depth,
        width = reservation.width,
        void = reservation.void,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        format!(
            "Fix: deepen the enclosing `size=WxH` so {rule}, or split the logic across several scopes, each with its own `circuit` line. Raising `void` does not help — the pads stand at `y = 0`, and `void` buys layers above them",
            rule = column.depth_rule(),
        ),
    )
}

/// The first usable `circuit` line of each scope, out of the one walk
/// [`compile_placement`] reads.
fn build_region_index(
    lines: &[Result<intent::CircuitRegion, intent::RejectedCircuitRegion>],
) -> HashMap<(intent::ScopeKind, String), &intent::CircuitRegion> {
    let mut index = HashMap::new();
    for region in lines.iter().filter_map(|line| line.as_ref().ok()) {
        // Multiple `circuit region=` lines in one scope: `check` refuses
        // each line after the first (`E_DUPLICATE_CIRCUIT`), and a caller
        // that places a module without `check` gets the first usable one.
        let key = (region.scope_kind, region.scope_name.clone());
        index.entry(key).or_insert(region);
    }
    index
}

/// The first `circuit` line of each scope that reserves nothing, out of
/// the same walk.
///
/// Not filtered against [`build_region_index`]: a scope can have a
/// usable line and a rejected one, and both indexes then hold it. It is
/// [`compile_placement`] that asks this index only after the region
/// index came up empty, so a pass that read this one on its own would
/// find scopes that do have a reservation.
fn build_rejected_index(
    lines: &[Result<intent::CircuitRegion, intent::RejectedCircuitRegion>],
) -> HashMap<(intent::ScopeKind, String), &intent::RejectedCircuitRegion> {
    let mut index = HashMap::new();
    for rejected in lines.iter().filter_map(|line| line.as_ref().err()) {
        // Several rejected lines in one scope: first wins, as in
        // `build_region_index`. The finding names the first and says
        // nothing of the rest, and a rejected line beside a usable one
        // is not reported here, since the usable line wins. `check`
        // refuses each top-level line after the first as
        // `E_DUPLICATE_CIRCUIT`; a line under a `level` is in that
        // level's body, which it does not count.
        let key = (rejected.scope_kind, rejected.scope_name.clone());
        index.entry(key).or_insert(rejected);
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

#[cfg(test)]
mod tests;
