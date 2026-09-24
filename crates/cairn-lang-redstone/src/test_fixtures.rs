//! Hand-built Placement IR fixtures shared by the in-crate tests of the
//! routing, delay, and crossing passes.

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;

use crate::edition_netlist_ir::EditionCell;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, PortName};
use crate::placement_ir::{
    CellCoord, CircuitRegionReservation, PlacedCellNode, PlacedOutputNode, PlacementIr,
    PlacementPhase, ScopedPlacementIr, ScopedPlacementIrEntry,
};

pub(crate) fn reservation(width: u32, depth: u32, void: u32) -> CircuitRegionReservation {
    CircuitRegionReservation {
        label: "floor".to_owned(),
        void,
        width,
        depth,
        span: Span::default(),
    }
}

pub(crate) fn scoped(kind: ScopeKind, name: &str, ir: PlacementIr) -> ScopedPlacementIr {
    let mut scoped = ScopedPlacementIr::new();
    scoped.scopes.push(ScopedPlacementIrEntry {
        kind,
        name: name.to_owned(),
        ir,
    });
    scoped
}

/// One scope whose reservation is too shallow to hold its pad column,
/// so the actuator pad lands on the only cell.
///
/// `depth = 1` leaves the pad column a single row and
/// [`crate::routing_geometry::output_pad`] saturates into it: `z = 0`,
/// `x = width - 1 = 1`, which is where the cell stands. Two blocks,
/// one coord.
///
/// Hand-built because the placement pass refuses a region this small
/// before any later stage sees it. That is the only caller shape the
/// refusal exists for — a caller assembling the IR itself, or one
/// resuming from a dump at `--stage delay` — so the fixture has to
/// build it directly.
///
/// `phase` is what the stage under test expects to be handed:
/// `Unrouted` for routing, `Routed` for delay, `Delayed` for crossing.
/// The geometry does not vary with it, which is the point — one
/// collapsed pad row, put to three stages.
pub(crate) fn collapsed_pad_row(phase: &PlacementPhase) -> ScopedPlacementIr {
    let region = reservation(2, 1, 1);
    let pad = crate::routing_geometry::output_pad(0, &region);
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(region);
    ir.inputs.push(NetlistInput {
        name: DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: vec![CellPortDriver {
            port: PortName::A,
            net: NetRef::Input(0),
        }],
        coord: CellCoord::new(1, 0, 0),
        phase: phase.clone(),
        span: Span::default(),
    });
    let mut output = PlacedOutputNode::new(
        DottedRef::new("sig".into(), vec!["out".into()]),
        NetRef::Cell(0),
        pad,
        Span::default(),
    );
    output.phase = phase.clone();
    ir.outputs.push(output);
    scoped(ScopeKind::Struct, "shallow", ir)
}
