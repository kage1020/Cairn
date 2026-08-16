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
//! and stamps `x = i`, `y = 0`, `z = 0`. The 2D / 2.5D lift is the
//! routing pass's concern — it needs the `plane` / `via` / `bridge`
//! escape hatches §14.5 mentions for crossings and fanout.
//!
//! Two diagnostic codes join the pass:
//! - [`crate::DiagnosticCode::NoCircuitRegion`] when a scope has cells
//!   or actuator pads to place but the enclosing struct / def declared no
//!   `circuit region=` line (or no `size=WxH` header for the region to
//!   sit inside). Sites always fall here because they carry no `size`.
//! - [`crate::DiagnosticCode::RouteCongestion`] when the netlist does
//!   not fit the reservation, which it can fail to do in two ways. The
//!   volume can be short: the v1 area budget uses [`CELL_FOOTPRINT`] as
//!   a per-cell footprint estimate, deliberately pessimistic so a
//!   placement that reports "fits" is unlikely to flip to a routing
//!   failure downstream. Or the *row* can be short, which the area
//!   budget cannot see — a `size=2x8` scope with `void=3` reserves 48
//!   cells' worth of volume and two columns of row. Both are checked,
//!   in that order, and each explains itself in its own terms.
//!   Follow-up refinement is `#[non_exhaustive]`-safe on both types.
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
    // The v1 layout is a single row: cell `i` stands at `x = i`, so the
    // row has to be at least as long as the cell count. The area test
    // above cannot see that. A `size=2x8` scope with `void=3` reserves 48
    // cells' worth of volume and offers a row two columns long, and a
    // three-cell netlist passes the first and overruns the second by one.
    //
    // Nothing downstream would notice either: every later pass reads the
    // coordinates this one stamps, and `routing_geometry::output_pad`
    // puts the actuator pad at `width - 1`. A cell past that column sits
    // to the right of the pad it drives, so the wire runs backwards out
    // of the reservation the author declared.
    if cell_count > reservation.width {
        return Err(row_overflow_diagnostic(&reservation, cell_count));
    }
    // The pads need rows of their own. `input_pad` and `output_pad` step
    // along z from 1 and saturate at `depth - 1`, and the cells occupy
    // `z = 0`, so a reservation holds its I/O only while `depth` is at
    // least one more than the larger of the two pad counts. Below that
    // the saturation stacks pads on each other, and at `depth == 1` it
    // drops them onto the cell row — which is the case the row check
    // above reads as fine, because the pad it reasons about is no longer
    // where it assumes. Refused here rather than left to the routing
    // pass's occupancy sweep so stage 1 stops emitting a dump whose
    // coordinates contradict each other.
    let pad_rows = source.inputs.len().max(source.outputs.len());
    let pad_rows = u32::try_from(pad_rows).unwrap_or(u32::MAX);
    if pad_rows > 0 && reservation.depth <= pad_rows {
        return Err(pad_row_diagnostic(&reservation, pad_rows));
    }

    for (index, source_cell) in source.cells.iter().enumerate() {
        // Same saturating-cast rationale as `cell_count` above: a
        // `.crn` big enough to overflow `u32` cannot practically
        // finish compilation.
        let x = u32::try_from(index).unwrap_or(u32::MAX);
        ir.cells.push(PlacedCellNode {
            cell: source_cell.cell,
            drivers: source_cell.drivers.clone(),
            coord: CellCoord::new(x, 0, 0),
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
        "synthesized netlist needs a row of {cell_count} cells but the reserved region is only {width} wide (region {width}x{depth}, void={void})",
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
        "Fix: widen the enclosing `size=WxH` so the region is at least as wide as the cell count, or split into multiple `circuit` blocks. Raising `void` does not help — cells are laid in one row and `void` buys height, not length",
    );
    debug_assert_eq!(diag.severity(), Severity::Error);
    diag
}

/// The reservation has no room for the I/O pads the scope needs.
///
/// Separate from the two footprint refusals because the resource is a
/// different one again: `void` buys height, the row buys length, and
/// this buys the rows the pads stand in. Sharing
/// [`DiagnosticCode::RouteCongestion`] with them keeps `spec/redstone`
/// §14.5's single fail-loud for "routing does not fit the region".
fn pad_row_diagnostic(reservation: &CircuitRegionReservation, pad_rows: u32) -> Diagnostic {
    let primary = format!(
        "synthesized netlist needs {needed} rows for its cells and I/O pads but the reserved region is only {depth} deep (region {width}x{depth}, void={void})",
        needed = pad_rows.saturating_add(1),
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
        "Fix: deepen the enclosing `size=WxH` so the region has one row per sensor or actuator plus the cell row, or split into multiple `circuit` blocks. Raising `void` does not help — pads stand beside the cells, not above them",
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
