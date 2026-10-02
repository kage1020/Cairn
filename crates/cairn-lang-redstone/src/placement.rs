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
//! (`routing_geometry::PadColumn`), so the coord beside an end cell in
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
use crate::routing_geometry::{PadColumn, output_pad};
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
/// and by the depth refusal that reserves the rows it needs, and by the
/// pad coordinates that step over it, so the three cannot drift.
pub(crate) const CELL_ROW: u32 = 1;

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
            "Fix: deepen the enclosing `size=WxH` so {rule}, or split into multiple `circuit` blocks. Raising `void` does not help — the pads stand at `y = 0`, and `void` buys layers above them",
            rule = column.depth_rule(),
        ),
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

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use cairn_lang_core::{Edition, lower, parse};

    use super::compile_placement;
    use crate::routing_geometry::{BlockKind, BlockSite, PadColumn, block_sites};
    use crate::{compile_edition_netlist, compile_netlist, synthesize};

    /// Every block the placement pass puts in the one scope `source`
    /// declares: cells, sensor pads and actuator pads.
    ///
    /// Java only. A pad's coordinate is a function of the region and
    /// the cell count, and a cell's of its index, so the Bedrock layout
    /// is the same one.
    fn placed_sites(source: &str) -> Vec<BlockSite> {
        let intent = lower(&parse(source).expect("the fixture parses"));
        let netlist = compile_netlist(&synthesize(&intent).scoped);
        let placed = compile_placement(&compile_edition_netlist(&netlist, Edition::Java), &intent);
        assert!(
            placed.diagnostics.is_empty(),
            "the fixture places: {:?}\n{source}",
            placed.diagnostics,
        );
        let ir = &placed.scoped.scopes[0].ir;
        let region = ir.region.clone().expect("a placed scope has a region");
        block_sites(ir, &region)
    }

    fn apart(a: &BlockSite, b: &BlockSite) -> u32 {
        a.coord.x.abs_diff(b.coord.x)
            + a.coord.y.abs_diff(b.coord.y)
            + a.coord.z.abs_diff(b.coord.z)
    }

    fn is_cell(site: &BlockSite) -> bool {
        matches!(site.kind, BlockKind::Cell)
    }

    /// Every pad that shares a face with a cell, as `(pad, cell)`.
    fn pads_against_cells(sites: &[BlockSite]) -> Vec<(BlockSite, BlockSite)> {
        let mut touching = Vec::new();
        for pad in sites.iter().filter(|s| !is_cell(s)) {
            for cell in sites.iter().filter(|s| is_cell(s)) {
                if apart(pad, cell) == 1 {
                    touching.push((*pad, *cell));
                }
            }
        }
        touching
    }

    /// The nearest any pad stands to any cell, or `None` with no cells.
    fn nearest_pad_to_a_cell(sites: &[BlockSite]) -> Option<u32> {
        sites
            .iter()
            .filter(|s| !is_cell(s))
            .flat_map(|pad| {
                sites
                    .iter()
                    .filter(|s| is_cell(s))
                    .map(|cell| apart(pad, cell))
            })
            .min()
    }

    /// Every two blocks on one coord, as `(first, second)`.
    fn shared_coords(sites: &[BlockSite]) -> Vec<(BlockSite, BlockSite)> {
        let mut shared = Vec::new();
        for (i, first) in sites.iter().enumerate() {
            for second in &sites[i + 1..] {
                if first.coord == second.coord {
                    shared.push((*first, *second));
                }
            }
        }
        shared
    }

    /// A chain of `cells` cells over `sensors` plates, the last cell on
    /// the first door and each other door on a sensor of its own, so
    /// the pads beside the row belong to nets the end cells have nothing
    /// to do with.
    fn chain(cells: usize, sensors: usize, doors: usize, width: usize, depth: u32) -> String {
        let mut source =
            String::from("theme t:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n");
        let _ = writeln!(
            source,
            "struct s size={width}x{depth}\n  floor mat_slot=wall"
        );
        for (d, side) in ["front", "back", "left", "right"]
            .iter()
            .take(doors)
            .enumerate()
        {
            let _ = writeln!(source, "  door id=d{d} side={side} at=center mat_slot=door");
        }
        for i in 0..sensors {
            let at = if i % 2 == 0 {
                "front.outside"
            } else {
                "inside.front"
            };
            let _ = writeln!(
                source,
                "  pressure_plate id=p{i} at={at} offset={i} y=0 -> sig.s{i}"
            );
        }
        let mut previous = String::from("sig.s0");
        for c in 0..cells {
            let other = (c + 1) % sensors;
            let op = if c % 2 == 0 { "or" } else { "and" };
            let _ = writeln!(source, "  logic sig.c{c} = {previous} {op} sig.s{other}");
            previous = format!("sig.c{c}");
        }
        let _ = writeln!(source, "  door[id=d0] opened_by={previous}");
        for d in 1..doors {
            let _ = writeln!(source, "  door[id=d{d}] opened_by=sig.s{}", d % sensors);
        }
        source.push_str("  circuit region=floor void=2\n");
        source
    }

    /// No pad stands face to face with a cell or on another block, at
    /// any width the row check accepts, and at the narrowest the end
    /// cells stand exactly one diagonal step from the pads.
    ///
    /// A pad is a terminal of the one net it carries, and the router's
    /// one-step rule keeps one net's dust away from another's rather
    /// than a pad away from a cell, so a pad against a cell of a net it
    /// has nothing to do with would take one of that cell's faces where
    /// no pass looks. The pad columns stand at `x = 0` and
    /// `x = width - 1`, and at the narrowest row the end cells stand in
    /// the columns beside them; what keeps the two apart is that the
    /// pads skip the cell row. Asserted against every cell, not only the
    /// ones of other nets, because the layout gives the stronger answer.
    ///
    /// "Not touching" alone is one-sided: pads moved far from the cells,
    /// or all onto one coord, would pass it. So the sweep also holds the
    /// distance to exactly 2 at the narrowest width, and every block to a
    /// coord of its own.
    ///
    /// The sweep starts at zero cells, where there is no row to step
    /// over and the region is exactly as deep as the pads, so a column
    /// that skipped a row it does not have would be refused or would
    /// stack its last two pads. A cell-less scope starts at two columns:
    /// at one, the sensor and actuator columns are the same column, which
    /// this pass places and stage 2 refuses (see
    /// `pass::pad_overlap_diagnostic`).
    #[test]
    fn no_pad_stands_against_a_cell() {
        // Reported with this source, where `sig.b`'s sensor pad and its
        // door's pad both stood against an inverter that reads only
        // `sig.a`.
        let reported = "\
theme t:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct s size=3x5
  floor mat_slot=wall
  door id=d0 side=front at=center mat_slot=door
  door id=d1 side=back at=center mat_slot=door
  pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=pb at=inside.front offset=0 y=0 -> sig.b
  logic sig.x = not sig.a
  door[id=d0] opened_by=sig.x
  door[id=d1] opened_by=sig.b
  circuit region=floor void=2
";
        assert_eq!(pads_against_cells(&placed_sites(reported)), Vec::new());

        for cells in 0..=3 {
            let column = PadColumn::for_cell_count(cells);
            for sensors in 2..=4 {
                for doors in 1..=3 {
                    let pads = column.rows(sensors.max(doors));
                    let depth = if cells == 0 { pads } else { pads.max(3) };
                    let narrowest = (2 * cells + 1).max(if cells == 0 { 2 } else { 1 });
                    for width in narrowest..=narrowest + 2 {
                        let source = chain(cells, sensors, doors, width, depth);
                        let at = format!(
                            "{cells} cells, {sensors} sensors, {doors} doors, \
                             {width}x{depth}:\n{source}"
                        );
                        let sites = placed_sites(&source);
                        assert_eq!(pads_against_cells(&sites), Vec::new(), "{at}");
                        assert_eq!(shared_coords(&sites), Vec::new(), "{at}");
                        let nearest = nearest_pad_to_a_cell(&sites);
                        if cells == 0 {
                            assert_eq!(nearest, None, "{at}");
                        } else if width == narrowest {
                            assert_eq!(nearest, Some(2), "{at}");
                        } else {
                            assert!(nearest >= Some(2), "{at}");
                        }
                    }
                }
            }
        }
    }
}
