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
//! `--stage route` JSON stays byte-identical to today because
//! `delay_ticks` is serde-skipped on `None` and appended as
//! `,"delay_ticks":N` (after `wire_length` in field declaration order,
//! matching serde's compact-JSON layout) when this pass writes it.

use cairn_lang_core::check::Severity;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacementIr, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::routing::{input_pad, manhattan, output_pad};

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
    // Defensive pass-through: a scope with no cells has no delay to
    // attribute. Upstream stages elide empty scopes; this branch is a
    // belt-and-braces for hand-built IRs.
    if source.cells.is_empty() {
        return Ok(source.clone());
    }
    let Some(region) = source.region.clone() else {
        // The placement / routing passes both fire on this branch:
        // placement raises `E_NO_CIRCUIT_REGION` and elides the scope,
        // routing carries a defensive `debug_assert!` for hand-built
        // IRs. Mirror routing's belt-and-braces so a hand-built IR
        // that ships cells without a region trips fast in debug and
        // stays deterministic in release.
        debug_assert!(
            source.cells.is_empty(),
            "delay_scope received a PlacementIr with cells but no region — placement should have elided it",
        );
        return Ok(source.clone());
    };

    let mut ir = source.clone();

    // Snapshot cell coords up front so the `wire_length`-style rewrite
    // that follows can index into `ir.cells` mutably without
    // re-borrowing across the `source_of_net` helper. Duplicated (not
    // extracted) from the routing pass because the closure is 10
    // lines and its `debug_assert!` invariants are pass-local; a
    // shared helper would need to carry `cell_coords` in a struct
    // that saves no runtime work.
    let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();

    let source_of_net = |net: NetRef| -> CellCoord {
        match net {
            NetRef::Input(i) => input_pad(i as usize, &region),
            NetRef::Cell(j) => {
                debug_assert!(
                    (j as usize) < cell_coords.len(),
                    "NetRef::Cell({j}) out of range (cells.len()={}) — topological invariant broken",
                    cell_coords.len(),
                );
                cell_coords
                    .get(j as usize)
                    .copied()
                    .unwrap_or_else(|| *cell_coords.last().expect("cells.is_empty checked above"))
            }
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

    attribute_delay_ticks(&mut ir, &cell_coords, &source_of_net);

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
/// `source_of_net`, then commits in a mutable pass. Asserts loud in
/// debug builds if any cell already carries a `Some(_)` `delay_ticks` —
/// that would mean the caller ran delay insertion twice, which the
/// phase table on `PlacedCellNode` forbids.
fn attribute_delay_ticks<F>(ir: &mut PlacementIr, cell_coords: &[CellCoord], source_of_net: &F)
where
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
    for (cell, ticks) in ir.cells.iter_mut().zip(delay_ticks) {
        debug_assert!(
            cell.delay_ticks.is_none(),
            "delay_scope re-writing a PlacedCellNode whose delay_ticks is already Some({:?}) — delay insertion should run once per routed IR",
            cell.delay_ticks,
        );
        cell.delay_ticks = Some(ticks);
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
        kind = scope_kind_label(entry.kind),
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
        kind = scope_kind_label(entry.kind),
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

fn scope_kind_label(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Struct => "struct",
        ScopeKind::Def => "def",
        ScopeKind::Site => "site",
    }
}
