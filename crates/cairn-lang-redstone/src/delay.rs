//! Routed Placement IR → delayed Placement IR lowering (delay insertion).
//!
//! Stage 3 of the five-stage place-and-route pipeline `spec/redstone`
//! §14.5 lays out (Placement → Steiner routing → Delay insertion →
//! Crossing legalization → Edition legalization). Walks every
//! [`crate::placement_ir::PlacedCellNode`] in each scope's routed
//! Placement IR, re-derives per-driver Manhattan segment lengths from
//! the same `NetRef → source coord` mapping the routing pass uses
//! (routing stored only the driver-sum `wire_length`, deliberately —
//! per-driver segments are cheap to re-walk here and would bloat the
//! JSON wire form if stored twice), and rewrites every cell's
//! [`crate::placement_ir::PlacedCellNode::delay_ticks`] from `None` to
//! `Some(base + implicit buffer contribution)`:
//!
//! - **Base delay** comes from the cell's physical realisation via
//!   [`crate::edition_netlist_ir::EditionCell::base_delay_ticks`] —
//!   1 tick for the pinned Java `ComparatorAnd` / `RepeaterOr` /
//!   `InverterTorch` and the Bedrock `InverterTorch`, 2 ticks for the
//!   Bedrock two-torch `TorchAnd`, 0 ticks for the Bedrock bare-dust
//!   `TorchOr`, and a pessimistic sentinel above every pinned base
//!   delay for the parser-unreachable `*Unpinned` variants.
//! - **Implicit buffer repeaters** are counted per driver segment.
//!   A dust segment fresh from a source at strength 15 loses one unit
//!   of signal per block (`spec/redstone` §14.5 "signal attenuation
//!   limit of 15"), so a segment of `s` blocks needs
//!   `floor((s - 1) / DUST_ATTENUATION_LIMIT)` buffer repeaters to
//!   refresh the signal to strength 15 before it reaches the sink.
//!   Each buffer contributes [`BUFFER_REPEATER_TICKS`] (default
//!   repeater delay, 1 tick).
//!
//! Buffer repeaters are **counted, not materialised** here. The
//! routing pass discarded its per-scope occupancy set before yielding
//! the routed IR, and stage 4 (crossing legalization) is the natural
//! owner of buffer coord assignment — it already needs to escape
//! cross-net overlaps into a `RouteLayer::Bridge` / `Via` layer, so
//! deferring buffer materialisation to it avoids two passes fighting
//! over the same free-block set. `delay_ticks` therefore captures the
//! tick contribution of the buffers this stage decided are needed;
//! stage 4 will place them without changing the tick count.
//!
//! `E_ATTENUATION_LIMIT` fires only when a driver segment exceeds the
//! v1 sanity cap [`MAX_ATTENUATION_SEGMENT`]. Segments in the
//! `(DUST_ATTENUATION_LIMIT, MAX_ATTENUATION_SEGMENT]` band are normal
//! and absorbed by implicit buffers; segments beyond the cap need a
//! stage-4 bridge/via escape that v1 does not implement, so this pass
//! refuses instead of silently ascribing a delay against an
//! unrealisable buffer chain. Failed scopes are elided from the
//! output for the same reason the routing pass elides congestion
//! failures — a partial `delay_ticks` set would let the future tick
//! simulator report a `latency` figure computed against a layout no
//! downstream stage can materialise.
//!
//! The delay pass is a field write on `PlacedCellNode::delay_ticks`
//! per the phase table on that type; no new IR type is introduced.
//! `--stage route` JSON keeps every key it had because `delay_ticks`
//! is serde-skipped on `None` and appended as `,"delay_ticks":N`
//! (after `wire_length` in the hand-written `Serialize` impl's
//! emission order, matching serde's compact-JSON layout) when this
//! pass writes it. The one value that moves is the `stage` tag, which
//! goes from `route` to `delay`.

use cairn_lang_core::check::Severity;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CellIdentity, CircuitRegionReservation, PlacementIr, ScopedPlacementIr,
    ScopedPlacementIrEntry,
};
use crate::routing_geometry::{input_pad, manhattan, output_pad};

/// Signal-attenuation ceiling per dust segment (`spec/redstone` §14.5
/// "signal attenuation limit of 15"). A dust source starts at strength
/// 15 and decays one unit per block, so a segment of at most this many
/// blocks reaches the sink at strength ≥ 1 without a buffer repeater.
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
/// under-reporting `delay_ticks` on every fixture that crosses the
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

/// v1 sanity cap on a single driver segment's Manhattan length. A
/// segment longer than this needs stage-4 crossing legalization to
/// escape into a `RouteLayer::Bridge` / `Via` layer — v1 has no such
/// escape, so the delay pass refuses with `E_ATTENUATION_LIMIT` rather
/// than count an unrealisable buffer chain into `delay_ticks`.
///
/// 256 blocks equals 16 buffer repeaters back-to-back (each covering
/// `DUST_ATTENUATION_LIMIT`); anything past that in a single flat
/// segment reads as a placement mistake rather than a routing corner
/// case in every fixture the crate ships today.
pub const MAX_ATTENUATION_SEGMENT: u32 = 256;

/// Output of a [`compile_delay`] run.
///
/// Mirrors [`crate::routing::RoutingOutput`]'s shape so callers see a
/// uniform result type across every stage of the place-and-route
/// pipeline. The delayed IR is a [`ScopedPlacementIr`] with every
/// non-failed scope's `delay_ticks` promoted from `None` to `Some(_)` —
/// no new IR type; the delay pass is a field write per the phase
/// table on [`crate::placement_ir::PlacedCellNode`].
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct DelayOutput {
    /// Placement IR for every scope whose delay insertion succeeded,
    /// with every cell's `delay_ticks` field populated.
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
/// succeeded; scopes that raise an Error-severity diagnostic (today,
/// only `E_ATTENUATION_LIMIT`) are elided from the output so a partial
/// `delay_ticks` set cannot pollute a future tick simulator.
#[must_use]
pub fn compile_delay(routed: &ScopedPlacementIr) -> DelayOutput {
    let mut out = DelayOutput::new();
    for entry in &routed.scopes {
        match delay_scope(entry) {
            Ok(ir) => {
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

/// Result of delaying one scope: the delayed IR on success, a single
/// Error-severity diagnostic on failure.
type ScopeDelay = Result<PlacementIr, Diagnostic>;

fn delay_scope(entry: &ScopedPlacementIrEntry) -> ScopeDelay {
    let source = &entry.ir;
    // Region absence with any placed cells or output drivers is a
    // caller-side hand-built-IR bug: the placement pass fires
    // `E_NO_CIRCUIT_REGION` and elides such scopes before they can
    // reach delay insertion. Refuse with the same diagnostic code so
    // a downstream caller sees a consistent error surface; scopes
    // with neither cells nor output drivers still pass through so a
    // module without any redstone survives the delay pipeline as-is.
    // Stricter than routing's belt-and-braces `debug_assert! +
    // Ok(source.clone())` fall-through by design: delay writes
    // `delay_ticks`, and the phase table on `PlacedCellNode` promises
    // `(Some, Some)` after this stage — silently returning `(None,
    // None)` on a partial IR would let a future tick simulator read a
    // Stage-1 shape from a Stage-3 output.
    let Some(region) = source.region.clone() else {
        if source.cells.is_empty() && source.outputs.is_empty() {
            return Ok(source.clone());
        }
        return Err(missing_region_diagnostic(entry));
    };
    if source.cells.is_empty() && source.outputs.is_empty() {
        return Ok(source.clone());
    }

    let mut ir = source.clone();

    // Snapshot cell coords up front so the `wire_length`-style rewrite
    // that follows can index into `ir.cells` mutably without
    // re-borrowing across the `source_of_net` helper. Duplicated (not
    // extracted) from the routing pass because the closure is 10
    // lines and its `debug_assert!` invariants are pass-local; a
    // shared helper would need to carry `cell_coords` in a struct
    // that saves no runtime work.
    let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();

    // Netlist synthesis guarantees the topological invariant
    // (`NetRef::Cell(j)` inside `cells[i]` satisfies `j < i`), so
    // `.expect(...)` is safe on both debug and release paths — an
    // out-of-range access here means the caller handed in a
    // hand-built IR that skipped synthesis, in which case panicking
    // loud beats silently sinking into a fall-back coord and under-
    // reporting `delay_ticks`. Stricter than routing's `debug_assert
    // + last-cell fallback` by the same phase-table-contract argument
    // that hardens the missing-region branch above.
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

    // First pass: refuse if any driver segment exceeds the v1 sanity
    // cap. Done before writing `delay_ticks` so a failed scope leaves
    // no partial attribution behind — `delay_scope`'s `Err` return
    // makes `compile_delay` elide the whole scope.
    for (cell_index, cell) in ir.cells.iter().enumerate() {
        let sink = cell_coords[cell_index];
        for (driver_index, driver) in cell.drivers.iter().enumerate() {
            let segment = manhattan(source_of_net(driver.net), sink);
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

    // Also flag output driver segments: an actuator wired straight to
    // a sensor across a wide region can hit the same sanity cap
    // without touching a cell. Uses the same source→sink Manhattan
    // model; the sink is the output pad coord.
    for (output_index, output) in ir.outputs.iter().enumerate() {
        let sink = output_pad(output_index, &region);
        let segment = manhattan(source_of_net(output.driver), sink);
        if segment > MAX_ATTENUATION_SEGMENT {
            return Err(attenuation_output_diagnostic(
                entry,
                &region,
                output_index,
                segment,
            ));
        }
    }

    attribute_delay_ticks(&mut ir, entry, &cell_coords, &source_of_net);

    Ok(ir)
}

/// Fill every cell's `delay_ticks` with `base_delay(cell) + Σ buffer
/// ticks per driver`. Buffers are counted from the per-driver
/// Manhattan segment length recomputed via `source_of_net`; the
/// routing pass stored only the driver-sum `wire_length` so
/// re-derivation is required here (documented in the pass module doc).
///
/// Computes into a side vector first so `ir.cells` can be borrowed
/// immutably while the driver sources are looked up through
/// `source_of_net`, then commits in a mutable pass. The commit is loud
/// in release too: `PlacementPhase::delay_at` panics on any
/// non-`Routed` variant, which is what a caller who ran delay
/// insertion twice hands us — the phase table on `PlacedCellNode`
/// forbids it. `entry` is threaded in purely so that panic can name
/// the offending cell instead of leaving the operator to walk back
/// from the backtrace.
fn attribute_delay_ticks<F>(
    ir: &mut PlacementIr,
    entry: &ScopedPlacementIrEntry,
    cell_coords: &[CellCoord],
    source_of_net: &F,
) where
    F: Fn(NetRef) -> CellCoord,
{
    let delay_ticks: Vec<u32> = ir
        .cells
        .iter()
        .zip(cell_coords.iter())
        .map(|(cell, &sink)| {
            let buffer_ticks = cell
                .drivers
                .iter()
                .map(|driver| {
                    let segment = manhattan(source_of_net(driver.net), sink);
                    buffer_repeater_ticks_for_segment(segment)
                })
                .fold(0u32, u32::saturating_add);
            cell.cell.base_delay_ticks().saturating_add(buffer_ticks)
        })
        .collect();
    for (index, (cell, ticks)) in ir.cells.iter_mut().zip(delay_ticks).enumerate() {
        let identity = CellIdentity::new(index, cell.coord, entry);
        cell.phase.delay_at(ticks, identity);
    }
}

/// Buffer repeaters needed to keep `segment` blocks of dust at
/// strength ≥ 1 at the sink, multiplied by [`BUFFER_REPEATER_TICKS`].
///
/// A source at strength 15 loses one unit per block, so segments of
/// at most `DUST_ATTENUATION_LIMIT` blocks reach the sink without a
/// buffer (`(15 - 1) / 15 = 0`); a 16-block segment needs one buffer;
/// each further `DUST_ATTENUATION_LIMIT` blocks bumps the count by
/// one. Saturating arithmetic keeps a pathological segment (e.g.
/// `u32::MAX` from a hand-built IR that skips the sanity check) from
/// overflowing — the sanity cap in `delay_scope` already prevents
/// that path in practice.
fn buffer_repeater_ticks_for_segment(segment: u32) -> u32 {
    if segment <= DUST_ATTENUATION_LIMIT {
        return 0;
    }
    let buffers = (segment.saturating_sub(1)) / DUST_ATTENUATION_LIMIT;
    buffers.saturating_mul(BUFFER_REPEATER_TICKS)
}

fn attenuation_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    cell_index: usize,
    driver_index: usize,
    segment: u32,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` has a driver segment of {segment} blocks into cell #{cell_index} port #{driver_index} — exceeds the v1 attenuation limit of {cap} blocks (dust decays 1/block, so this segment would need {buffers} buffer repeaters and a stage-4 crossing-legalization escape to materialize)",
        kind = entry.kind.label(),
        name = entry.name,
        cap = MAX_ATTENUATION_SEGMENT,
        buffers = buffer_repeater_ticks_for_segment(segment) / BUFFER_REPEATER_TICKS.max(1),
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::AttenuationLimit,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: enlarge `region=` so no driver→cell segment exceeds the cap, split into multiple `circuit` blocks, or pin cell placement closer to its drivers",
    );
    debug_assert_eq!(diag.severity, Severity::Error);
    diag
}

fn attenuation_output_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    output_index: usize,
    segment: u32,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` has a driver segment of {segment} blocks into output pad #{output_index} — exceeds the v1 attenuation limit of {cap} blocks (dust decays 1/block, so this segment would need {buffers} buffer repeaters and a stage-4 crossing-legalization escape to materialize)",
        kind = entry.kind.label(),
        name = entry.name,
        cap = MAX_ATTENUATION_SEGMENT,
        buffers = buffer_repeater_ticks_for_segment(segment) / BUFFER_REPEATER_TICKS.max(1),
    );
    let mut diag = Diagnostic::new(
        DiagnosticCode::AttenuationLimit,
        reservation.span.clone(),
        primary,
    );
    diag = diag.with_footer(
        "Fix: enlarge `region=` so no driver→sink segment exceeds the cap, split into multiple `circuit` blocks, or pin actuator placement closer to its drivers",
    );
    debug_assert_eq!(diag.severity, Severity::Error);
    diag
}

/// Refuse a scope that reached delay insertion carrying cells or
/// output drivers but no `circuit region=` reservation. Reuses
/// `E_NO_CIRCUIT_REGION` because the failure mode is the same the
/// placement pass catches; a downstream reader that matches on the
/// code sees a consistent taxonomy across stages. The span anchors on
/// the first placed cell's span (mirroring the placement-pass
/// helper), falling back to a default span when the scope carries
/// only outputs and no cells to hang the span on.
fn missing_region_diagnostic(entry: &ScopedPlacementIrEntry) -> Diagnostic {
    let span = entry
        .ir
        .cells
        .first()
        .map(|c| c.span.clone())
        .unwrap_or_default();
    let primary = format!(
        "routed netlist for {kind} `{name}` reached delay insertion carrying cells or output drivers but no `circuit region=<label> void=<N>` reservation — the placement pass should have elided this scope",
        kind = entry.kind.label(),
        name = entry.name,
    );
    let mut diag = Diagnostic::new(DiagnosticCode::NoCircuitRegion, span, primary);
    diag = diag.with_footer(
        "Fix: add a `circuit region=<label> void=<N>` line to the enclosing scope, or run `--stage placement` first to see the underlying error",
    );
    debug_assert_eq!(diag.severity, Severity::Error);
    diag
}

#[cfg(test)]
mod tests {
    //! Crate-internal unit tests for delay-pass behaviours that
    //! `tests/delay.rs` cannot reach through synth fixtures alone:
    //! - the cell-driver branch of `attenuation_diagnostic` (needs a
    //!   segment past 256 blocks between two cells, which no realistic
    //!   `.crn` produces);
    //! - the missing-region refusal introduced by Critical 4, which
    //!   can only be built by hand-constructing a `PlacementIr`
    //!   because `compile_placement` already elides that shape;
    //! - `buffer_repeater_ticks_for_segment` at multiple boundary
    //!   segment lengths (0, 15, 16, 30, 31, cap) so a formula change
    //!   trips per-boundary rather than by aggregate.
    //!
    //! Uses crate-internal struct construction (all `PlacedCellNode` /
    //! `PlacementIr` / `CircuitRegionReservation` fields are `pub`;
    //! `#[non_exhaustive]` blocks only external crates), keeping the
    //! integration-test surface in `tests/delay.rs` focused on synth
    //! fixtures.

    use cairn_lang_core::Edition;
    use cairn_lang_core::error::Span;

    use super::{
        BUFFER_REPEATER_TICKS, DUST_ATTENUATION_LIMIT, MAX_ATTENUATION_SEGMENT,
        buffer_repeater_ticks_for_segment, compile_delay,
    };
    use crate::diagnostic::DiagnosticCode;
    use crate::edition_netlist_ir::EditionCell;
    use crate::logic_ir::ScopeKind;
    use crate::netlist_ir::{CellPortDriver, NetRef, PortName};
    use crate::placement_ir::{
        CellCoord, CircuitRegionReservation, PlacedCellNode, PlacementIr, PlacementPhase,
        ScopedPlacementIr, ScopedPlacementIrEntry,
    };

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
            phase: PlacementPhase::Routed { wire_length: 0 },
            span: Span::default(),
        }
    }

    #[test]
    fn cell_driver_attenuation_primary_names_cell_and_port() {
        // Two cells wide-spread inside a `size=300x3` reservation so
        // the cell[1] driver from cell[0] spans a Manhattan segment
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
        // delay because it skipped placement. Critical 4 hardens this
        // path to refuse instead of silently passing through with a
        // `(None, None)` phase-table violation.
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
        // synthesis-side topological invariant. Critical 3 hardens
        // this from "silent last-cell fallback" to a loud release
        // panic so a caller-side bug cannot produce silently wrong
        // `delay_ticks`.
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
        expected = "for cell #1 at (4,0,1) in struct `mixed` — delay insertion must run once per routed IR"
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
            delay_ticks: 0,
        };
        ir.cells.push(already_delayed);
        let _ = compile_delay(&scoped(ScopeKind::Struct, "mixed", ir));
    }
}
