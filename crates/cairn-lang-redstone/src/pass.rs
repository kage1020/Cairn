//! Plumbing shared by the routing, delay, and crossing passes: opening a
//! scope, laying its nets, measuring each node against the routed trees,
//! and folding the per-scope results into a pass output.
//!
//! The three passes rebuild the same trees from the same IR, so the
//! steps that decide what those trees are live here once. What each
//! pass writes into the phase, and what it refuses over, stays with the
//! pass.

use std::collections::HashMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{
    CellCoord, CellIdentity, CircuitRegionReservation, PlacedCellNode, PlacedOutputNode,
    PlacementIr, PlacementPhase, ScopedPlacementIr, ScopedPlacementIrEntry,
};
use crate::routing_geometry::{
    BlockSite, NetTree, Router, block_sites, collapsed_block, collect_nets, input_pad, net_trees,
    unroutable,
};
use crate::saturating_index;

/// Why [`open_scope`] found nothing to lay out.
pub(crate) enum Skipped {
    /// Neither cells nor actuator pads, so the pass hands the scope back
    /// verbatim. `ScopedPlacementIr::push` elides these on the input
    /// side; this covers a hand-built IR.
    Empty,
    /// Cells or pads but no reservation. The placement pass fires
    /// `E_NO_CIRCUIT_REGION` and elides such a scope before it can reach
    /// a later pass, so this is a caller-side hand-built IR.
    MissingRegion,
}

/// Refuse a scope that reached `stage` carrying cells or output drivers
/// but no `circuit region=` reservation. Reuses `E_NO_CIRCUIT_REGION`
/// because the failure mode is the one the placement pass catches, so a
/// reader matching on the code sees one taxonomy across stages. The span
/// anchors on the first cell, falling back to a default span when the
/// scope carries only outputs.
pub(crate) fn missing_region_diagnostic(
    entry: &ScopedPlacementIrEntry,
    netlist: &str,
    stage: &str,
) -> Diagnostic {
    let span = entry
        .ir
        .cells
        .first()
        .map(|c| c.span.clone())
        .unwrap_or_default();
    let primary = format!(
        "{netlist} netlist for {kind} `{name}` reached {stage} carrying cells or output drivers but no `circuit region=<label> void=<N>` reservation — the placement pass should have elided this scope",
        kind = entry.kind.label(),
        name = entry.name,
    );
    error_with_footer(
        DiagnosticCode::NoCircuitRegion,
        span,
        primary,
        "Fix: add a `circuit region=<label> void=<N>` line to the enclosing scope, or run `--stage placement` first to see the underlying error",
    )
}

/// One scope a pass is working on: the IR it will write into, cloned
/// from the entry, and the geometry every pass measures against.
pub(crate) struct OpenScope {
    pub(crate) ir: PlacementIr,
    pub(crate) region: CircuitRegionReservation,
    /// Snapshot of every cell's coord, so `ir.cells` can be borrowed
    /// mutably while a `NetRef → source coord` closure reads these.
    pub(crate) cell_coords: Vec<CellCoord>,
    /// `ir.inputs.len()`, for the same reason and read by the same
    /// closure: the bound `NetRef::Input(i)` is checked against.
    pub(crate) inputs: usize,
    /// Every block standing in the reservation, per [`block_sites`].
    pub(crate) blocks: Vec<BlockSite>,
}

pub(crate) fn open_scope(entry: &ScopedPlacementIrEntry) -> Result<OpenScope, Skipped> {
    let source = &entry.ir;
    if source.cells.is_empty() && source.outputs.is_empty() {
        return Err(Skipped::Empty);
    }
    let Some(region) = source.region.clone() else {
        return Err(Skipped::MissingRegion);
    };
    let ir = source.clone();
    let cell_coords: Vec<CellCoord> = ir.cells.iter().map(|c| c.coord).collect();
    let inputs = ir.inputs.len();
    let blocks = block_sites(&ir, &region);
    Ok(OpenScope {
        ir,
        region,
        cell_coords,
        inputs,
        blocks,
    })
}

/// `NetRef → source coord`: the input pad for a sensor, the cell body
/// for a gate.
///
/// Both arms are in range for any IR a prior stage built. `Cell(j)` is
/// in range by the topological invariant carried across every stage
/// (`j < i` inside `cells[i]`), and `Input(i)` by `inputs` being copied
/// verbatim from the netlist the nets were collected out of.
///
/// # Panics
///
/// Panics on either index out of range, which is a caller-side
/// hand-built IR and nothing an author can edit. The alternative is a
/// source coord the pass then lays a wire from: an out-of-range `Cell`
/// saturated onto another cell roots the net in the wrong corridor, and
/// an out-of-range `Input` answers with a pad coord that stands in no
/// block [`block_sites`] emitted. Both are a layout, not a number, so
/// they reach the author as a plausible circuit rather than as a bug.
pub(crate) fn source_of_net<'a>(
    region: &'a CircuitRegionReservation,
    cell_coords: &'a [CellCoord],
    inputs: usize,
) -> impl Fn(NetRef) -> CellCoord + Copy + 'a {
    move |net| {
        match net {
        NetRef::Input(i) => {
            assert!(
                (i as usize) < inputs,
                "NetRef::Input({i}) out of range (inputs.len()={inputs}) — netlist invariant broken by caller-side hand-built IR",
            );
            input_pad(i as usize, region)
        }
        NetRef::Cell(j) => *cell_coords.get(j as usize).unwrap_or_else(|| {
            panic!(
                "NetRef::Cell({j}) out of range (cells.len()={}) — topological invariant broken by caller-side hand-built IR",
                cell_coords.len(),
            )
        }),
    }
    }
}

/// The router over this scope, every net and its routed tree, or the
/// first refusal the reservation earns.
///
/// Two refusals, and both are `E_ROUTE_CONGESTION`: a reservation too
/// shallow to hold its pad row, and a sink no route reaches. Only one
/// comes out, so which one is a decision rather than a race — the pad
/// row first, because it is the cause the stranded sink is a symptom
/// of, and the shared code leaves the author nothing else to tell them
/// apart by.
///
/// Both are asked by all three passes, and for one reason. Stage 2
/// elides the scope that earns either, and the CLI always runs it, so
/// through `cairn` stages 3 and 4 never see one. What reaches them is
/// a caller that assembled the IR itself: an in-crate test, or a
/// library consumer calling [`crate::compile_delay`] or
/// [`crate::compile_crossing`] directly, both of which the crate root
/// re-exports. Neither failure announces itself downstream.
/// A stranded sink's route is one step: under every cap, worth no
/// repeater, and indistinguishable in the dump from a circuit that
/// works. A collapsed pad makes one of the two blocks something the
/// router routes *to* and the other a coord it routes *through*, and
/// the dump that comes out reads as a layout.
///
/// The router is built here rather than by each caller, so the refusals
/// cannot be reached around: a pass holding a [`Router`] is a pass that
/// has been through both.
pub(crate) fn lay_nets<F>(
    ir: &PlacementIr,
    blocks: &[BlockSite],
    entry: &ScopedPlacementIrEntry,
    region: &CircuitRegionReservation,
    source_of_net: F,
) -> Result<Nets, Diagnostic>
where
    F: Fn(NetRef) -> CellCoord + Copy,
{
    if let Some(site) = collapsed_block(blocks) {
        return Err(pad_overlap_diagnostic(entry, region, site));
    }
    let router = Router::new(region, blocks);
    let sinks = collect_nets(ir);
    let trees = net_trees(&sinks, &router, source_of_net);
    if let Some(diagnostic) = unroutable(&sinks, &trees, entry, region, source_of_net) {
        return Err(diagnostic);
    }
    Ok(Nets {
        router,
        sinks,
        trees,
    })
}

/// Refuse a scope whose reservation collapses two blocks onto one
/// coord, naming the block that landed second and the reservation that
/// could not hold it.
///
/// `E_ROUTE_CONGESTION`, because the cause is the reserved area: the
/// pad row wants a row per sensor or actuator and has fewer.
///
/// `depth >= max(inputs, outputs)`, not `+ 1`. `edge_pad` saturates at
/// `z = min(index, depth - 1)`, so N pads collide only once `N > depth`
/// — and the placement pass guards on exactly that number
/// (`pad_rows > reservation.depth`). Two stages under one code have to
/// hand the author the same arithmetic.
fn pad_overlap_diagnostic(
    entry: &ScopedPlacementIrEntry,
    reservation: &CircuitRegionReservation,
    site: &BlockSite,
) -> Diagnostic {
    let primary = format!(
        "routed netlist for {kind} `{name}` cannot fit its {pad_kind} pad #{pad_index} at ({x},{y},{z}) — the reserved area (void={void}, region {width}x{depth}) collapses I/O pads onto a cell coord or another pad",
        kind = entry.kind.label(),
        name = entry.name,
        pad_kind = site.kind.as_str(),
        pad_index = site.index,
        x = site.coord.x,
        y = site.coord.y,
        z = site.coord.z,
        void = reservation.void,
        width = reservation.width,
        depth = reservation.depth,
    );
    error_with_footer(
        DiagnosticCode::RouteCongestion,
        reservation.span.clone(),
        primary,
        "Fix: enlarge `size=WxH` so `depth >= max(inputs, outputs)` — one row per sensor or actuator — or split into multiple `circuit` blocks",
    )
}

/// The nets of one scope: the router they were laid with, driver →
/// sinks, and driver → routed tree.
pub(crate) struct Nets {
    /// Carried out so a caller that needs the obstacle set again — the
    /// routing pass, for its tile-layer advisory — does not rebuild one
    /// and get a second chance to build it differently.
    pub(crate) router: Router,
    pub(crate) sinks: HashMap<NetRef, Vec<CellCoord>>,
    pub(crate) trees: HashMap<NetRef, NetTree>,
}

impl Nets {
    /// The routed length from `net`'s source to `sink` — the measure
    /// stage 2 records as wire and stage 3 counts buffer repeaters
    /// against.
    ///
    /// `route_to` answers `None` only for a sink that is not a terminal
    /// of the net, which [`collect_nets`] rules out: it built the tree's
    /// terminal list from the same driver list.
    pub(crate) fn segment(&self, net: NetRef, sink: CellCoord) -> u32 {
        let route = self
            .trees
            .get(&net)
            .and_then(|tree| tree.route_to(sink))
            .unwrap_or_else(|| {
                panic!(
                    "sink ({x},{y},{z}) is not a terminal of the net driving it — the driver list and the collected nets disagree",
                    x = sink.x,
                    y = sink.y,
                    z = sink.z,
                )
            });
        saturating_index(route.len().saturating_sub(1))
    }
}

/// Measure every cell and every actuator pad, then commit each figure
/// through `commit` with the identity a phase-transition panic names.
///
/// Measured into a side vector first so `ir` is borrowed immutably while
/// the routes are read, then committed in a mutable pass. The commit is
/// loud in release too: the `*_at` transitions panic on a phase the pass
/// does not consume, which is what a caller who ran a pass twice hands
/// us.
pub(crate) fn attribute_nodes<T>(
    ir: &mut PlacementIr,
    entry: &ScopedPlacementIrEntry,
    measure_cell: impl Fn(&PlacedCellNode) -> T,
    measure_output: impl Fn(&PlacedOutputNode) -> T,
    commit: impl Fn(&mut PlacementPhase, T, CellIdentity<'_>),
) {
    let cell_values: Vec<T> = ir.cells.iter().map(measure_cell).collect();
    for (index, (cell, value)) in ir.cells.iter_mut().zip(cell_values).enumerate() {
        let identity = CellIdentity::new(index, cell.coord, entry);
        commit(&mut cell.phase, value, identity);
    }
    let output_values: Vec<T> = ir.outputs.iter().map(measure_output).collect();
    for (index, (output, value)) in ir.outputs.iter_mut().zip(output_values).enumerate() {
        let identity = CellIdentity::output(index, output.pad, entry);
        commit(&mut output.phase, value, identity);
    }
}

/// Run `lower` over every scope, keeping the IR of each one that
/// succeeds and collecting the findings in scope order. A refused scope
/// contributes its refusal and no IR, so a partial result never reaches
/// the next pass.
pub(crate) fn lower_scopes<F>(
    input: &ScopedPlacementIr,
    mut lower: F,
) -> (ScopedPlacementIr, Vec<Diagnostic>)
where
    F: FnMut(&ScopedPlacementIrEntry) -> Result<(PlacementIr, Vec<Diagnostic>), Diagnostic>,
{
    let mut scoped = ScopedPlacementIr::new();
    let mut diagnostics = Vec::new();
    for entry in &input.scopes {
        match lower(entry) {
            Ok((ir, advisories)) => {
                diagnostics.extend(advisories);
                scoped.scopes.push(ScopedPlacementIrEntry {
                    kind: entry.kind,
                    name: entry.name.clone(),
                    ir,
                });
            }
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    (scoped, diagnostics)
}

#[cfg(test)]
mod tests {
    //! The refusals this module owns, asked of every stage that calls
    //! [`lay_nets`].

    use cairn_lang_core::Edition;
    use cairn_lang_core::error::Span;

    use crate::diagnostic::DiagnosticCode;
    use crate::edition_netlist_ir::EditionCell;
    use crate::logic_ir::ScopeKind;
    use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, PortName};
    use crate::placement_ir::{
        CellCoord, PlacedCellNode, PlacementIr, PlacementPhase, ScopedPlacementIr,
    };
    use crate::test_fixtures::{
        CollapsedRow, DanglingNet, collapsed_pad_row, dangling_net, regionless_scope, reservation,
        scoped,
    };

    /// A reservation too shallow to hold its pad row is refused by
    /// stage 2, stage 3 and stage 4 alike, and the scope is elided
    /// rather than measured against a collapsed layout.
    ///
    /// What the refusal prevents is an IR whose actuator pad and cell
    /// body are one voxel carrying a `wire_length` and a tick count
    /// measured against itself. Nothing downstream says so: the
    /// numbers are small and plausible, and the dump reads as a
    /// layout. The stages that can be handed one without stage 2
    /// having run are stages 3 and 4, reached by a caller that
    /// assembled the IR itself — an in-crate test, or a library
    /// consumer calling `compile_delay` or `compile_crossing`, which
    /// the crate root re-exports.
    ///
    /// Written per stage rather than per pass because what is being
    /// held is that the three answer one geometry the same way; a case
    /// that asked only the stage under test could not fail on two of
    /// them at once. Both ends of the pad column are rowed, because
    /// they saturate onto different things — the actuator column onto
    /// the cell row, the sensor column onto its own previous pad — and
    /// the message names which. `input_pad` and `output_pad` are
    /// tested as pure functions in `routing_geometry`; what their
    /// saturation feeds into is this refusal.
    ///
    /// Hand-built, because the placement pass refuses a region this
    /// small one stage earlier and the CLI always runs it. That is not
    /// a reason to leave the shape untested: it is the shape a library
    /// consumer reaches, and until this change two of the three stages
    /// took it without a word.
    #[test]
    fn every_stage_refuses_a_reservation_too_shallow_for_its_pad_row() {
        let stages = stages();
        let mut said: Vec<(CollapsedRow, Vec<String>)> = Vec::new();
        for row in [CollapsedRow::OutputOntoCell, CollapsedRow::InputOntoInput] {
            let (kind, index, coord) = row.names();
            let mut primaries = Vec::new();
            for (stage, phase, run) in &stages {
                let (diagnostics, kept) = run(&collapsed_pad_row(phase, row));
                let refusal = diagnostics
                    .iter()
                    .find(|d| d.code == DiagnosticCode::RouteCongestion)
                    .unwrap_or_else(|| {
                        panic!("the {stage} pass must refuse a collapsed pad row: {diagnostics:?}")
                    });
                assert!(
                    refusal
                        .primary
                        .contains(&format!("{kind} pad #{index} at {coord}"))
                        && refusal.primary.contains("collapses I/O pads"),
                    "the {stage} pass names which pad could not fit, where, and why: {}",
                    refusal.primary,
                );
                primaries.push(refusal.primary.clone());
                assert_eq!(
                    kept,
                    vec!["roomy".to_owned()],
                    "the {stage} pass elides the refused scope and only that one",
                );
                // The fix line and the span are what an author acts on, and
                // nothing else in the crate asserts either. The span is the
                // reservation's, because `size=` is the edit.
                let footer = refusal
                    .notes
                    .iter()
                    .find(|n| n.message.starts_with("Fix:"))
                    .unwrap_or_else(|| panic!("the {stage} refusal carries a fix line"));
                assert!(
                    footer.message.contains("depth >= max(inputs, outputs)")
                        && !footer.message.contains("+ 1"),
                    "the fix line gives the same arithmetic as the placement pass: {}",
                    footer.message,
                );
                assert_eq!(
                    refusal.span,
                    row.span(),
                    "the {stage} refusal anchors on the `circuit region=` line, not on a cell",
                );
            }
            said.push((row, primaries));
        }

        // One geometry, one sentence. A stage that refused for the
        // right reason in different words would still leave an author
        // comparing two messages to see they are the same finding.
        for (row, primaries) in said {
            assert!(
                primaries.windows(2).all(|w| w[0] == w[1]),
                "{row:?}: the three stages word the refusal identically, got {primaries:?}",
            );
        }
    }

    /// A scope that collapses its pad row *and* strands a sink is
    /// refused over the pad row.
    ///
    /// Both are `E_ROUTE_CONGESTION`, so the code tells the author
    /// nothing about which they have, and both have one cause and one
    /// edit: the reserved area is too small. The pad-row message says
    /// that and names the `size=` line. The stranded-sink message
    /// describes a sink the router gave up on, which is a symptom of
    /// the same shortage and sends the author looking at their cells.
    /// Only one comes out — the pass returns at the first — so which
    /// one is a decision, made here by asking the pad row before the
    /// trees are grown.
    ///
    /// The one-input row is the control: the same walls, the same
    /// boxed sinks, a pad column that fits. It has to keep reporting
    /// the stranded sink, or the rule above collapses into "always say
    /// congestion".
    #[test]
    fn a_scope_that_earns_both_refusals_is_refused_over_the_pad_row() {
        for (inputs, names, why) in [
            // `cannot reach (x,y,z)`, not `cannot be reached`: the
            // latter is only in the plural suffix, so a row keyed on
            // it would pass on the count of stranded sinks rather than
            // on the finding being the stranded-sink one.
            (1, "cannot reach (2,0,0)", "a pad column that fits"),
            (3, "input pad #2", "input #2 saturating onto input #1"),
        ] {
            let mut ir = PlacementIr::new(Edition::Java);
            ir.region = Some(reservation(4, 2, 1));
            for i in 0..inputs {
                ir.inputs.push(NetlistInput {
                    name: cairn_lang_core::ast::DottedRef::new("sig".into(), vec![format!("s{i}")]),
                    span: Span::default(),
                });
            }
            // Input pad #0 lands at (0,0,0). Walling the row at
            // (1,0,0) and the course above the two sinks leaves them
            // nothing to be fed from, and `void=1` reserves no layer
            // to come in over.
            for coord in [
                CellCoord::new(1, 0, 0),
                CellCoord::new(2, 0, 1),
                CellCoord::new(3, 0, 1),
            ] {
                ir.cells.push(cell(coord, Vec::new()));
            }
            for coord in [CellCoord::new(2, 0, 0), CellCoord::new(3, 0, 0)] {
                ir.cells.push(cell(
                    coord,
                    vec![CellPortDriver {
                        port: PortName::A,
                        net: NetRef::Input(0),
                    }],
                ));
            }

            let routed = crate::routing::compile_routing(&scoped(ScopeKind::Struct, "boxed", ir));
            let primaries: Vec<&str> = routed
                .diagnostics
                .iter()
                .map(|d| d.primary.as_str())
                .collect();
            assert!(
                primaries.len() == 1 && primaries[0].contains(names),
                "{inputs} input(s), {why}: expected the one refusal naming `{names}`, got {primaries:?}",
            );
            // The premise of the paragraph above: one code for both, so
            // the message is all the author has to tell them apart.
            assert_eq!(
                routed.diagnostics[0].code,
                DiagnosticCode::RouteCongestion,
                "{inputs} input(s): both refusals are E_ROUTE_CONGESTION",
            );
        }
    }

    /// The three stages that lay nets, each with the phase it expects
    /// to be handed and a reader for what it produced.
    ///
    /// The shared shape of the cross-stage tests below: a scope's
    /// geometry does not vary with the phase, so one fixture can be put
    /// to all three, and a case that asked only one stage could not
    /// fail on two of them at once — which is the whole subject here,
    /// three passes that were answering one shape three ways.
    type Stage = (
        &'static str,
        PlacementPhase,
        fn(&ScopedPlacementIr) -> (Vec<crate::diagnostic::Diagnostic>, Vec<String>),
    );

    fn stages() -> [Stage; 3] {
        [
            ("routing", PlacementPhase::Unrouted, |scoped| {
                let out = crate::routing::compile_routing(scoped);
                (
                    out.diagnostics,
                    out.scoped.scopes.iter().map(|e| e.name.clone()).collect(),
                )
            }),
            (
                "delay",
                PlacementPhase::Routed { wire_length: 0 },
                |scoped| {
                    let out = crate::delay::compile_delay(scoped);
                    (
                        out.diagnostics,
                        out.scoped.scopes.iter().map(|e| e.name.clone()).collect(),
                    )
                },
            ),
            (
                "crossing",
                PlacementPhase::Delayed {
                    wire_length: 0,
                    local_delay_ticks: 0,
                },
                |scoped| {
                    let out = crate::crossing::compile_crossing(scoped);
                    (
                        out.diagnostics,
                        out.scoped.scopes.iter().map(|e| e.name.clone()).collect(),
                    )
                },
            ),
        ]
    }

    /// A scope with cells and no reservation is `E_NO_CIRCUIT_REGION`
    /// at whichever stage meets it, and the scope is elided.
    ///
    /// The routing pass used to pass this through: a `debug_assert!`
    /// that the scope was empty, and in release a scope handed on
    /// carrying cells that were never routed. Nothing downstream said
    /// so — the delay pass met the same scope and refused it there, one
    /// stage later, naming its own stage. What each pass writes is
    /// promised by the producer↔variant table on `PlacementPhase` after
    /// its own stage, so the pass that cannot write it is the pass that
    /// has to say why.
    #[test]
    fn every_stage_refuses_a_scope_with_cells_and_no_region() {
        for (stage, phase, run) in &stages() {
            let (diagnostics, kept) = run(&regionless_scope(phase));
            let refusal = diagnostics
                .iter()
                .find(|d| d.code == DiagnosticCode::NoCircuitRegion)
                .unwrap_or_else(|| {
                    panic!("the {stage} pass must refuse a regionless scope: {diagnostics:?}")
                });
            assert!(
                refusal.primary.contains("struct `roomless`") && refusal.primary.contains(*stage),
                "the {stage} pass names the scope and its own stage: {}",
                refusal.primary,
            );
            assert_eq!(
                kept,
                vec!["roomy".to_owned()],
                "the {stage} pass elides the refused scope and only that one",
            );
        }
    }

    /// And a scope with no reservation *and* nothing to lay out is
    /// still handed back, at every stage.
    ///
    /// The guard above fires on cells or outputs, not on the absent
    /// region, so this is the case that says the refusal is keyed on
    /// what the scope carries rather than on what it lacks.
    #[test]
    fn every_stage_passes_an_empty_regionless_scope_through() {
        for (stage, _, run) in &stages() {
            let (diagnostics, kept) = run(&scoped(
                ScopeKind::Struct,
                "harmless",
                PlacementIr::new(Edition::Java),
            ));
            assert!(
                diagnostics.is_empty(),
                "the {stage} pass has nothing to say about an empty scope: {diagnostics:?}",
            );
            assert_eq!(kept, vec!["harmless".to_owned()]);
        }
    }

    /// A driver naming a net no list can answer panics at every stage,
    /// naming the variant, the index, and the length it ran past.
    ///
    /// Not a diagnostic: no edit to the source produces or repairs it,
    /// because no source produces it. It is a caller-side hand-built IR,
    /// and the crate's convention for one is to assert.
    ///
    /// Both variants, because they used to fail differently and one of
    /// them did not fail at all. `Cell(j)` saturated onto the last cell
    /// in the routing pass's release build, rooting the net in the wrong
    /// corridor and laying its dust down the wrong one; `Input(i)`
    /// answered for any index in all three, with a pad coord standing in
    /// no block `block_sites` emitted.
    #[test]
    fn every_stage_panics_on_a_net_index_the_ir_cannot_answer() {
        for which in [DanglingNet::Cell, DanglingNet::Input] {
            for (stage, phase, run) in &stages() {
                let fixture = dangling_net(phase, which);
                let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&fixture);
                }))
                .expect_err(&format!(
                    "the {stage} pass must not answer a dangling {which:?} net with a coord",
                ));
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("<non-string panic>");
                assert!(
                    message.contains(which.expected()),
                    "the {stage} pass names the index and the list it ran past, got {message:?}",
                );
            }
        }
    }

    /// An unrouted cell at `coord`, driven by `drivers`.
    fn cell(coord: CellCoord, drivers: Vec<CellPortDriver>) -> PlacedCellNode {
        PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers,
            coord,
            phase: PlacementPhase::Unrouted,
            span: Span::default(),
        }
    }
}
