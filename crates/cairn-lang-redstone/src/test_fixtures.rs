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
/// before any later stage sees it, and the CLI always runs that pass.
/// A caller assembling the IR itself is the only shape the refusal
/// exists for — an in-crate test, or a library consumer calling
/// [`crate::compile_delay`] or [`crate::compile_crossing`] — so the
/// fixture has to build it directly.
///
/// `phase` is what the stage under test expects to be handed:
/// `Unrouted` for routing, `Routed` for delay, `Delayed` for crossing.
/// The geometry does not vary with it, which is the point — one
/// collapsed pad row, put to three stages.
///
/// A second, sound scope rides along so a caller can tell "elides the
/// scope that earned the refusal" from "elides everything". On a
/// one-scope fixture those two are the same assertion.
pub(crate) fn collapsed_pad_row(phase: &PlacementPhase, row: CollapsedRow) -> ScopedPlacementIr {
    let mut scoped = ScopedPlacementIr::new();
    scoped.scopes.push(ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "shallow".to_owned(),
        ir: shallow_ir(phase, row),
    });
    scoped.scopes.push(ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "roomy".to_owned(),
        ir: roomy_ir(phase),
    });
    scoped
}

/// The sound half of [`collapsed_pad_row`]: pad column, cell row and
/// actuator edge all on coords of their own.
fn roomy_ir(phase: &PlacementPhase) -> PlacementIr {
    let region = reservation(6, 3, 1);
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
        coord: CellCoord::new(1, 0, 1),
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
    ir
}

/// Which end of the pad column runs out of rows.
///
/// Both ends saturate through the same `edge_pad`, but they land on
/// different things — the actuator column onto the cell row, the sensor
/// column onto its own previous pad — and the refusal names which.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CollapsedRow {
    /// `depth = 1` leaves the actuator column no row of its own, so
    /// output pad #0 lands on the cell at the right-hand edge.
    OutputOntoCell,
    /// `depth = 2` leaves the sensor column two rows for three inputs,
    /// so input pad #2 saturates onto input pad #1.
    InputOntoInput,
}

impl CollapsedRow {
    /// `(reservation, inputs)`: the region too small for the row, and
    /// how many sensors it is asked to hold.
    fn shape(self) -> (CircuitRegionReservation, usize) {
        match self {
            Self::OutputOntoCell => (reservation(2, 1, 1), 1),
            Self::InputOntoInput => (reservation(4, 2, 1), 3),
        }
    }

    /// What the refusal must name: pad kind, index, and coord.
    pub(crate) fn names(self) -> (&'static str, usize, &'static str) {
        match self {
            Self::OutputOntoCell => ("output", 0, "(1,0,0)"),
            Self::InputOntoInput => ("input", 2, "(0,0,1)"),
        }
    }

    /// The reservation span the refusal anchors on.
    pub(crate) fn span(self) -> Span {
        self.shape().0.span
    }
}

fn shallow_ir(phase: &PlacementPhase, row: CollapsedRow) -> PlacementIr {
    let (region, inputs) = row.shape();
    let pad = crate::routing_geometry::output_pad(0, &region);
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(region);
    for i in 0..inputs {
        ir.inputs.push(NetlistInput {
            name: DottedRef::new("sig".into(), vec![format!("s{i}")]),
            span: Span::default(),
        });
    }
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
    ir
}
