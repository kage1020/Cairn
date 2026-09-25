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

/// A scope carrying cells but no `circuit region=` reservation, beside
/// the sound scope [`collapsed_pad_row`] uses.
///
/// Hand-built for the same reason: the placement pass fires
/// `E_NO_CIRCUIT_REGION` and elides such a scope before any later
/// stage sees it, so a caller assembling the IR itself is the only way
/// to produce one. `phase` is what the stage under test expects to be
/// handed, and the second scope is what distinguishes "elides the
/// scope that earned the refusal" from "elides everything".
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

/// Which `NetRef` variant the fixture below points past the end of the
/// list that answers it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DanglingNet {
    /// A driver naming a cell the IR does not have. Breaks the
    /// topological invariant (`j < i` inside `cells[i]`) as well as the
    /// bound, since the only cell is the one carrying the driver, and
    /// `cells[0]` can satisfy `j < 0` for no `j` at all.
    Cell,
    /// A driver naming a sensor the netlist does not have.
    Input,
}

impl DanglingNet {
    /// Exactly one index past the end of the list that answers it,
    /// rather than comfortably past.
    ///
    /// An index far past the end is answered by any bound that is
    /// roughly right, so it cannot tell `< len` from `<= len`. The
    /// first index the list does not have is the one that can.
    fn net(self) -> NetRef {
        match self {
            Self::Cell => NetRef::Cell(1),
            Self::Input => NetRef::Input(1),
        }
    }

    /// The whole panic message, not a prefix of it.
    ///
    /// The routing pass used to carry a `debug_assert!` whose wording
    /// was the first half of this one, so a test keyed on the index
    /// alone would have passed against the shape being replaced — in
    /// debug builds, and only there.
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
/// The reservation is roomy and the geometry sound, so nothing but the
/// dangling index is wrong: a stage that answers this fixture at all
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
/// Both arms of the attenuation walk, because they are two loops and a
/// gate that grew only one of them would still answer the case a
/// `.crn` produces.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FarSink {
    /// A gate body driven by the sensor pad at the near edge.
    Cell,
    /// An actuator pad driven by the same, which is the shape a
    /// `region=` as wide as its `size=` makes on its own.
    OutputPad,
}

impl FarSink {
    /// How a refusal names it, in the wording [`crate::delay`] uses for
    /// the same sink one stage later.
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
/// Nothing stands between them, so the route is the straight line and
/// `width - 1` is both the distance and the segment the cap is measured
/// against. That is what lets a caller put the two sides of the cap —
/// the straight-line floor and the routed length — on one axis by
/// moving one number.
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
