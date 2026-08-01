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
//!   to place but the enclosing struct / def declared no
//!   `circuit region=` line (or no `size=WxH` header for the region to
//!   sit inside). Sites always fall here because they carry no `size`.
//! - [`crate::DiagnosticCode::RouteCongestion`] when the netlist needs
//!   more area than the reservation offers. The v1 area budget uses
//!   [`CELL_FOOTPRINT`] as a per-cell footprint estimate — deliberately
//!   pessimistic so a placement that reports "fits" almost never
//!   flips to a routing failure downstream. Follow-up refinement is
//!   `#[non_exhaustive]`-safe on both types.
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
    CellCoord, CircuitRegionReservation, PlacedCellNode, PlacementIr, PlacementPhase,
    ScopedPlacementIr,
};

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
    // Identity-wire scopes — inputs/outputs but zero cells, e.g. a
    // `pressure_plate -> sig.a` bound directly to `door opened_by=sig.a`
    // with no `logic` line in between — reach the placement pass because
    // `EditionNetlistIr::is_empty()` requires *all three* of inputs /
    // outputs / cells to be empty for elision. There is nothing to place
    // for a scope with no cells, so drop it from the Placement IR at
    // this stage. The routing pass (M6 follow-up) will consume the
    // Edition Netlist IR directly for these no-cell wires; nothing in
    // the placement contract survives without at least one cell to
    // coordinate.
    if source.cells.is_empty() {
        return Ok(PlacementIr::new(source.edition));
    }

    let mut ir = PlacementIr::new(source.edition);
    ir.inputs.clone_from(&source.inputs);
    ir.outputs.clone_from(&source.outputs);
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
    ir.region = Some(reservation);
    Ok(ir)
}

fn missing_region_diagnostic(source: &EditionNetlistIr) -> Diagnostic {
    let span = source
        .cells
        .first()
        .map(|c| c.span.clone())
        .unwrap_or_default();
    Diagnostic::new(
        DiagnosticCode::NoCircuitRegion,
        span,
        "this scope has redstone cells to place but no usable `circuit region=<label> void=<N>` reservation is in scope (missing line, malformed `region=` / `void=`, or the enclosing scope has no `size=WxH` header)"
            .to_owned(),
    )
    .with_footer(
        "add a `circuit region=<label> void=<N>` line whose `region=` names a member kind that lives in the scope's footprint (a non-empty label) and whose `void=` is an integer >= 1, and give the enclosing scope a `size=WxH` header",
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
    debug_assert_eq!(diag.severity, Severity::Error);
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
