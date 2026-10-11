//! Netlist IR: edition-neutral cells and nets over the Logic IR DAG.
//!
//! The netlist pass ([`crate::netlist::compile_netlist`]) rewrites each
//! [`crate::logic_ir::GateNode`] into a [`CellNode`] tagged with a [`LogicalCell`], the top tier of
//! the three-tier cell library (`Logical Cell → Edition Cell → Physical Tile`, `spec/redstone`
//! "Edition differences"). The Java/Bedrock split is decided one tier down, in
//! [`crate::edition_netlist_ir`].
//!
//! [`NetRef`] has the shape of the Logic IR's [`crate::logic_ir::SignalRef`]: an index into
//! [`NetlistIr::inputs`] or [`NetlistIr::cells`], which is in topological order. Delay is not
//! carried; per `spec/redstone` "Time model" it is first determined in the Placement IR.

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::logic_ir::{ScopeKind, Scoped, ScopedEntry};

/// A reference to a driving net, resolved to either a top-level input port
/// (a sensor emitting `-> sig.X`) or a cell output.
///
/// The two variants index separate arenas, so a consumer dispatches on the variant alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "index",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[non_exhaustive]
pub enum NetRef {
    /// Index into [`NetlistIr::inputs`].
    Input(u32),
    /// Index into [`NetlistIr::cells`] — the output net of that cell.
    Cell(u32),
}

/// Logical cell chosen for a [`CellNode`].
///
/// Edition-neutral: the same [`LogicalCell::And`] becomes Java `ComparatorAND` or Bedrock
/// `TorchAND` in edition selection (`spec/redstone` "Edition differences"). `#[non_exhaustive]` so
/// the sequential macros of `spec/redstone` "Two tiers, and the v1 boundary" (`latch`, `pulse`,
/// `delay`, `edge_rising`, `edge_falling`, `counter`) can join without breaking a downstream match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LogicalCell {
    /// Two-input AND.
    And,
    /// Two-input OR.
    Or,
    /// Single-input inverter.
    Not,
    /// Two-input XOR. Reserved: no surface syntax produces it.
    Xor,
    /// Two-input NAND. Reserved, like `Xor`.
    Nand,
    /// Two-input NOR. Reserved, like `Xor`.
    Nor,
    /// Two-to-one multiplexer (`sel` picks between `a` and `b`). Reserved, like `Xor`.
    Mux,
}

/// Input-port name on a [`CellNode`].
///
/// The union over every [`LogicalCell`]: `Not` uses `A`, the two-input cells `A` and `B`, `Mux`
/// `Sel`, `A` and `B`. `#[non_exhaustive]` so a sequential cell can add ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::EnumIter))]
#[non_exhaustive]
pub enum PortName {
    /// First data input (`a` operand).
    A,
    /// Second data input (`b` operand).
    B,
    /// `Mux` select line.
    Sel,
}

/// One `(port name, driving net)` pair on a [`CellNode`]; a struct so the JSON form has labelled
/// fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellPortDriver {
    /// Which input port this driver feeds.
    pub port: PortName,
    /// Net driving the port.
    pub net: NetRef,
}

/// One combinational cell in the DAG of [`NetlistIr::cells`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CellNode {
    /// Logical cell this node computes.
    pub cell: LogicalCell,
    /// Port → net drivers, in each cell's canonical port order:
    /// two-input gates emit `[A, B]`; `Not` emits `[A]`;
    /// `Mux` emits `[Sel, A, B]`.
    pub drivers: Vec<CellPortDriver>,
    /// Byte range inherited from the source [`crate::logic_ir::GateNode`], which anchors later
    /// passes' diagnostics.
    #[serde(skip)]
    pub span: Span,
}

/// A sensor-driven net feeding this scope (`pressure_plate ... -> sig.step`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetlistInput {
    /// Dotted signal reference the sensor emits.
    pub name: DottedRef,
    /// Byte range of the originating `-> sig.X` binding in source.
    #[serde(skip)]
    pub span: Span,
}

/// An actuator-driven net sink (`door[id=front] opened_by=sig.open`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetlistOutput {
    /// Dotted signal reference the actuator consumes.
    pub name: DottedRef,
    /// The net driving this port, from [`crate::logic_ir::OutputPort::driver`].
    pub driver: NetRef,
    /// Byte range of the originating `opened_by=` (or equivalent) argument.
    #[serde(skip)]
    pub span: Span,
}

/// The Netlist IR for one struct/def/site body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetlistIr {
    /// Sensor-driven nets, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<NetlistInput>,
    /// Actuator-driven nets, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<NetlistOutput>,
    /// Combinational cells in topological (definition) order: every driver is an input or an
    /// earlier entry of this vector, so a consumer maps the DAG in one forward pass.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<CellNode>,
    /// `sig.NAME` → the net that defines it: [`crate::logic_ir::LogicIr::signal_defs`] with each
    /// signal rewritten to a [`NetRef`], serialised the same way.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "crate::logic_ir::serialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, NetRef>,
}

impl NetlistIr {
    /// An empty Netlist IR.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            cells: Vec::new(),
            signal_defs: IndexMap::new(),
        }
    }

    /// `true` when the IR has no inputs, outputs or cells.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty() && self.cells.is_empty()
    }
}

impl Default for NetlistIr {
    fn default() -> Self {
        Self::new()
    }
}

/// Every scope's [`NetlistIr`], in source order across the whole module.
pub type ScopedNetlistIr = Scoped<NetlistIr>;

/// One entry in a [`ScopedNetlistIr`]: the Netlist IR synthesised from
/// one scope's Logic IR.
pub type ScopedNetlistIrEntry = ScopedEntry<NetlistIr>;

impl ScopedNetlistIr {
    /// Appends one scope's Netlist IR, skipping an empty one.
    pub fn push(&mut self, kind: ScopeKind, name: String, ir: NetlistIr) {
        if ir.is_empty() {
            // Synth writes `signal_defs` only alongside an input, output or gate, so an entry here
            // has no owner.
            debug_assert!(
                ir.signal_defs.is_empty(),
                "empty NetlistIr carries orphan signal_defs entries: {:?}",
                ir.signal_defs.keys().collect::<Vec<_>>(),
            );
            return;
        }
        self.scopes.push(ScopedNetlistIrEntry { kind, name, ir });
    }
}
