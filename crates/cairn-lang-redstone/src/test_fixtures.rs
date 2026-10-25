//! Hand-built Placement IR fixtures for the routing, delay and crossing tests.

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;

use crate::edition_netlist_ir::EditionCell;
use crate::grid::{PadColumn, output_pad};
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

/// `sig.<name>` as a [`DottedRef`].
pub(crate) fn sig(name: &str) -> DottedRef {
    DottedRef::new("sig".to_owned(), vec![name.to_owned()])
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

/// A cell's phase as stage 2 hands it on, with nothing measured.
pub(crate) const ROUTED: PlacementPhase = PlacementPhase::Routed { wire_length: 0 };

/// A cell's phase as stage 3 hands it on, with nothing measured.
pub(crate) const DELAYED: PlacementPhase = PlacementPhase::Delayed {
    wire_length: 0,
    local_delay_ticks: 0,
};

pub(crate) fn placed_cell(
    cell: EditionCell,
    coord: CellCoord,
    drivers: Vec<CellPortDriver>,
    phase: PlacementPhase,
) -> PlacedCellNode {
    PlacedCellNode {
        cell,
        drivers,
        coord,
        phase,
        span: Span::default(),
    }
}

/// An actuator pad already through routing.
pub(crate) fn routed_output(driver: NetRef, pad: CellCoord) -> PlacedOutputNode {
    let mut output = PlacedOutputNode::new(
        DottedRef::new("sig".into(), vec!["out".into()]),
        driver,
        pad,
        Span::default(),
    );
    output.phase = ROUTED;
    output
}

/// A scope too shallow for its pad column, which the placement pass would refuse, beside a sound
/// one.
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

/// The sound scope that rides beside each refused fixture.
fn roomy_ir(phase: &PlacementPhase) -> PlacementIr {
    let region = reservation(6, 3, 1);
    let pad = output_pad(0, PadColumn::for_cell_count(1), &region);
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
#[derive(Debug, Clone, Copy)]
pub(crate) enum CollapsedRow {
    /// `depth = 1`: output pad #0 lands on the cell at the right-hand edge.
    OutputOntoCell,
    /// `depth = 2` for three inputs: input pad #2 saturates onto #1.
    InputOntoInput,
}

impl CollapsedRow {
    /// The region too small for the row, and how many sensors it holds.
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
    let pad = output_pad(0, PadColumn::for_cell_count(1), &region);
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

/// A scope with cells but no reservation, beside the sound one.
pub(crate) fn regionless_scope(phase: &PlacementPhase) -> ScopedPlacementIr {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: vec![],
        coord: CellCoord::new(0, 0, 0),
        phase: phase.clone(),
        span: Span::default(),
    });
    let mut scoped = ScopedPlacementIr::new();
    scoped.scopes.push(ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "roomless".to_owned(),
        ir,
    });
    scoped.scopes.push(ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "roomy".to_owned(),
        ir: roomy_ir(phase),
    });
    scoped
}

/// Which `NetRef` variant [`dangling_net`] points one past the end of its list.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DanglingNet {
    /// A driver naming a cell the IR does not have.
    Cell,
    /// A driver naming a sensor the netlist does not have.
    Input,
}

impl DanglingNet {
    /// One past the end, the index that tells `< len` from `<= len`.
    fn net(self) -> NetRef {
        match self {
            Self::Cell => NetRef::Cell(1),
            Self::Input => NetRef::Input(1),
        }
    }

    /// The whole panic message.
    pub(crate) fn expected(self) -> &'static str {
        match self {
            Self::Cell => {
                "NetRef::Cell(1) out of range (cells.len()=1) — topological invariant broken by \
                 an IR built or edited outside the passes"
            }
            Self::Input => {
                "NetRef::Input(1) out of range (inputs.len()=1) — netlist invariant broken by \
                 an IR built or edited outside the passes"
            }
        }
    }
}

/// One scope whose only cell is driven by a net no list can answer, in otherwise sound geometry.
pub(crate) fn dangling_net(phase: &PlacementPhase, which: DanglingNet) -> ScopedPlacementIr {
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(6, 3, 1));
    ir.inputs.push(NetlistInput {
        name: DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    ir.cells.push(PlacedCellNode {
        cell: EditionCell::JavaRepeaterOr,
        drivers: vec![CellPortDriver {
            port: PortName::A,
            net: which.net(),
        }],
        coord: CellCoord::new(1, 0, 1),
        phase: phase.clone(),
        span: Span::default(),
    });
    scoped_entry(ir)
}

fn scoped_entry(ir: PlacementIr) -> ScopedPlacementIr {
    scoped(ScopeKind::Struct, "dangling", ir)
}

/// Which kind of sink [`far_sink`] puts at the far edge of the reservation.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FarSink {
    /// A gate body.
    Cell,
    /// An actuator pad.
    OutputPad,
}

impl FarSink {
    /// How a refusal names it, in [`crate::delay`]'s wording.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Cell => "cell #0 port #0",
            Self::OutputPad => "output pad #0",
        }
    }
}

/// One scope whose only net runs straight across the reservation, so `width - 1` is both its
/// straight-line and its routed length.
pub(crate) fn far_sink(phase: &PlacementPhase, width: u32, which: FarSink) -> ScopedPlacementIr {
    let region = reservation(width, 3, 2);
    let far = CellCoord::new(width - 1, 0, 0);
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(region);
    ir.inputs.push(NetlistInput {
        name: DottedRef::new("sig".into(), vec!["a".into()]),
        span: Span::default(),
    });
    match which {
        FarSink::Cell => ir.cells.push(PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers: vec![CellPortDriver {
                port: PortName::A,
                net: NetRef::Input(0),
            }],
            coord: far,
            phase: phase.clone(),
            span: Span::default(),
        }),
        FarSink::OutputPad => {
            let mut output = PlacedOutputNode::new(
                DottedRef::new("sig".into(), vec!["out".into()]),
                NetRef::Input(0),
                far,
                Span::default(),
            );
            output.phase = phase.clone();
            ir.outputs.push(output);
        }
    }
    scoped(ScopeKind::Struct, "wide", ir)
}

/// One scope whose only route is an 18-block staircase between two diagonal walls, where no coord
/// runs straight for the repeater it needs, beside a sound one.
pub(crate) fn staircase(phase: &PlacementPhase) -> ScopedPlacementIr {
    const SIDE: u32 = 10;
    let walls = (0..SIDE - 1)
        .map(|x| (x, x + 1))
        .chain((2..SIDE).map(|x| (x, x - 2)));
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(16, 16, 1));
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
        coord: CellCoord::new(SIDE - 1, 0, SIDE - 1),
        phase: phase.clone(),
        span: Span::default(),
    });
    for (x, z) in walls {
        ir.cells.push(PlacedCellNode {
            cell: EditionCell::JavaRepeaterOr,
            drivers: Vec::new(),
            coord: CellCoord::new(x, 0, z),
            phase: phase.clone(),
            span: Span::default(),
        });
    }
    let mut scoped = scoped(ScopeKind::Struct, "stairs", ir);
    scoped.scopes.push(ScopedPlacementIrEntry {
        kind: ScopeKind::Struct,
        name: "roomy".to_owned(),
        ir: roomy_ir(phase),
    });
    scoped
}
