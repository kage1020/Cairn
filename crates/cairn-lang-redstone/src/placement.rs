//! Stage 1 of `spec/redstone` "Place-and-route": lays each scope's cells in one row at `x = 1 +
//! 2i`, `z = 1` of its `circuit region=` reservation, and gives each actuator its pad. The clear
//! column and rows around each cell leave a two-input gate the three free faces its nets need.

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

/// Per-cell footprint of the congestion estimate, in blocks: a cell and its input tails.
pub const CELL_FOOTPRINT: u32 = 4;

/// Columns the row spends per cell: its own and the clear one beside it.
const CELL_SPACING: u32 = 2;

/// The `Fix:` footer of every area-budget refusal.
pub(crate) const CONGESTION_FIX: &str = "Fix: increase `void`, enlarge region, or split the logic across several scopes, each with its own `circuit` line";

/// `used / reserved` to one decimal place, as `(whole, tenths)`; `reserved` must be non-zero.
pub(crate) fn area_ratio_tenths(used: u64, reserved: u64) -> (u64, u64) {
    let ratio_x10 = used.saturating_mul(10) / reserved;
    (ratio_x10 / 10, ratio_x10 % 10)
}

/// Output of a [`compile_placement`] run: the placed scopes and the findings.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct PlacementOutput {
    /// Placement IR for every scope that placed successfully.
    pub scoped: ScopedPlacementIr,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

impl PlacementOutput {
    /// An empty output.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lowers a [`ScopedEditionNetlistIr`] to a [`ScopedPlacementIr`], leaving out a scope that fails
/// to place.
#[must_use]
pub fn compile_placement(
    scoped: &ScopedEditionNetlistIr,
    module: &IntentModule,
) -> PlacementOutput {
    let mut out = PlacementOutput::new();
    let lines = intent::circuit_lines(module);
    let region_index = build_region_index(&lines);
    let rejected_index = build_rejected_index(&lines);

    for entry in &scoped.scopes {
        let key = (map_scope_kind(entry.kind), entry.name.clone());
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

type ScopePlacement = Result<PlacementIr, Diagnostic>;

/// What one scope's `circuit` lines come to.
#[derive(Clone, Copy)]
enum ScopeRegion<'a> {
    /// The scope's first usable `circuit` line.
    Reserved(&'a intent::CircuitRegion),
    /// No usable line; the scope's first `circuit` line that reserves nothing.
    Rejected(&'a intent::RejectedCircuitRegion),
    /// No `circuit` line the walk found for the scope, which is always so for a site.
    Absent,
}

fn compile_scope(
    kind: ScopeKind,
    source: &EditionNetlistIr,
    region: ScopeRegion<'_>,
) -> ScopePlacement {
    // Outputs alone still need the reservation, for their pads.
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
    let row_columns = u64::from(cell_count)
        .saturating_mul(u64::from(CELL_SPACING))
        .saturating_add(1);
    if row_columns > u64::from(reservation.width) {
        return Err(row_overflow_diagnostic(&reservation, cell_count));
    }
    if !source.inputs.is_empty() && !source.outputs.is_empty() && reservation.width < 2 {
        return Err(pad_column_diagnostic(&reservation));
    }
    let row_depth = u64::from(CELL_ROW).saturating_add(2);
    if cell_count > 0 && row_depth > u64::from(reservation.depth) {
        return Err(row_depth_diagnostic(&reservation));
    }
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

/// `E_NO_CIRCUIT_REGION` for a scope with something to place and no `circuit` line the walk found.
fn missing_region_diagnostic(kind: ScopeKind, source: &EditionNetlistIr) -> Diagnostic {
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

/// `E_NO_CIRCUIT_REGION` on the `circuit` line that reserves nothing, with core's reason clause.
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

/// The reservation has the volume but not the `2n + 1`-column row.
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

/// The reservation is one column wide, so the sensor and actuator pad columns collide.
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

/// The reservation has the row but not the clear rows beside it.
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

/// The reservation has too few rows for the I/O pads the scope needs.
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

/// The first usable `circuit` line of each scope.
fn build_region_index(
    lines: &[Result<intent::CircuitRegion, intent::RejectedCircuitRegion>],
) -> HashMap<(intent::ScopeKind, String), &intent::CircuitRegion> {
    let mut index = HashMap::new();
    for region in lines.iter().filter_map(|line| line.as_ref().ok()) {
        let key = (region.scope_kind, region.scope_name.clone());
        index.entry(key).or_insert(region);
    }
    index
}

/// The first `circuit` line of each scope that reserves nothing. Not filtered against
/// [`build_region_index`]: a scope may be in both, and the region index wins.
fn build_rejected_index(
    lines: &[Result<intent::CircuitRegion, intent::RejectedCircuitRegion>],
) -> HashMap<(intent::ScopeKind, String), &intent::RejectedCircuitRegion> {
    let mut index = HashMap::new();
    for rejected in lines.iter().filter_map(|line| line.as_ref().err()) {
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
