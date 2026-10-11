//! Hand-built Placement IR fixtures shared by the in-crate tests of the
//! routing, delay, and crossing passes.

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

/// A cell of kind `cell` standing on `coord`, driven by `drivers`, in
/// `phase`.
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

/// An actuator pad already through routing, as [`placed_cell`] in [`ROUTED`] is for a cell: the
/// delay pass is handed an IR that skipped stages 1 and 2.
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

/// One scope whose reservation is too shallow to hold its pad column,
/// so the actuator pad lands on the only cell.
///
/// `depth = 1` leaves the pad column a single row and [`crate::grid::output_pad`] saturates into
/// it: `z = 0`, `x = width - 1 = 1`, where the cell stands.
///
/// Hand-built because the placement pass refuses a region this small, so only a caller that
/// skips it, like a library consumer of [`crate::compile_delay`] or [`crate::compile_crossing`],
/// meets this. `phase` is what the stage under test expects (`Unrouted` for routing, `Routed` for
/// delay, `Delayed` for crossing); the geometry is the same for all three.
///
/// A second, sound scope rides along so a caller can tell "drops the refused scope" from "drops
/// everything".
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

/// The sound half of [`collapsed_pad_row`]: pad column, cell row and actuator edge all on coords
/// of their own.
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
///
/// Both ends saturate through the same `edge_pad`, but they land on
/// different things — the actuator column onto a cell, the sensor
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

/// A scope carrying cells but no `circuit region=` reservation, beside the sound scope of
/// [`collapsed_pad_row`], for the same reasons: the placement pass refuses such a scope, and
/// `phase` is what the stage under test expects.
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

/// Which `NetRef` variant the fixture below points past the end of the list that answers it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DanglingNet {
    /// A driver naming a cell the IR does not have, which breaks the topological invariant
    /// (`j < i` inside `cells[i]`) as well as the bound.
    Cell,
    /// A driver naming a sensor the netlist does not have.
    Input,
}

impl DanglingNet {
    /// Exactly one index past the end of the list, the one index that tells `< len` from
    /// `<= len`.
    fn net(self) -> NetRef {
        match self {
            Self::Cell => NetRef::Cell(1),
            Self::Input => NetRef::Input(1),
        }
    }

    /// The whole panic message, so a test cannot pass on a different assertion that shares its
    /// opening words.
    pub(crate) fn expected(self) -> &'static str {
        match self {
            Self::Cell => {
                "NetRef::Cell(1) out of range (cells.len()=1) — topological invariant broken by \
                 caller-side hand-built IR"
            }
            Self::Input => {
                "NetRef::Input(1) out of range (inputs.len()=1) — netlist invariant broken by \
                 caller-side hand-built IR"
            }
        }
    }
}

/// One scope whose only cell is driven by a net no list can answer.
///
/// The reservation is roomy and the geometry sound, so a stage that answers this fixture at all
/// answered it with a coord it invented.
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

/// Which kind of sink [`far_sink`] puts at the far edge of the
/// reservation.
///
/// Both, because the attenuation walk handles them in two loops.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FarSink {
    /// A gate body driven by the sensor pad at the near edge.
    Cell,
    /// An actuator pad driven by the same, the shape a `region=` as wide as its `size=` makes.
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

/// One scope whose only net runs the width of the reservation: a sink
/// at `x = width - 1` driven by the sensor pad at `x = 0`.
///
/// Nothing stands between them, so `width - 1` is both the straight-line distance and the routed
/// segment, and one number moves both across the cap.
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

/// One scope whose only net has nowhere for a buffer repeater, beside
/// a sound one.
///
/// The sensor pad at `(0,0,0)` drives a gate at `(9,0,9)` across a
/// `16x16`, `void=1` reservation. Seventeen gate bodies that drive
/// nothing stand on two diagonal lines, `(x, x+1)` for `x` in `0..9`
/// and `(x, x-2)` for `x` in `2..10`, and a path that moves one block
/// at a time cannot cross a diagonal line without stepping onto it. So
/// the one shortest way through — the one the router lays — is the
/// staircase between them, east, south, east, south: 18 blocks, so the
/// signal needs a repeater, and every coord between the two ends turns,
/// which no repeater can carry a signal through.
///
/// Every node carries `phase`; handed to stage 2 unrouted, the staircase is the router's own. The
/// second scope is [`collapsed_pad_row`]'s sound one.
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
