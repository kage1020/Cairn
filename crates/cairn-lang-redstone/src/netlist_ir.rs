//! Netlist IR: edition-neutral cells and nets over the Logic IR DAG, the top tier of the cell
//! library in `spec/redstone` "Edition differences". Delay is first fixed in the Placement IR.

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::logic_ir::{ScopeKind, Scoped, ScopedEntry};

/// A driving net: a top-level input port or a cell output.
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
    /// Index into [`NetlistIr::cells`].
    Cell(u32),
}

/// Edition-neutral logical cell chosen for a [`CellNode`].
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

/// Input-port name on a [`CellNode`], the union over every [`LogicalCell`].
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

impl PortName {
    /// The port's name as the source and the JSON form spell it.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::Sel => "sel",
        }
    }
}

/// One `(port name, driving net)` pair on a [`CellNode`].
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
    /// Port → net drivers in canonical order: `[A, B]`, `[A]` for `Not`, `[Sel, A, B]` for `Mux`.
    pub drivers: Vec<CellPortDriver>,
    /// Byte range of the source [`crate::logic_ir::GateNode`].
    #[serde(skip)]
    pub span: Span,
}

/// A sensor-driven net feeding this scope (`pressure_plate ... -> sig.step`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetlistInput {
    /// Dotted signal reference the sensor emits.
    #[serde(deserialize_with = "crate::logic_ir::deserialize_dotted_ref")]
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
    /// Cells in topological order: every driver is an input or an earlier cell.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<CellNode>,
    /// `sig.NAME` → the net that defines it.
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

/// One entry in a [`ScopedNetlistIr`].
pub type ScopedNetlistIrEntry = ScopedEntry<NetlistIr>;

impl ScopedNetlistIr {
    /// Appends one scope's Netlist IR, skipping an empty one.
    pub fn push(&mut self, kind: ScopeKind, name: String, ir: NetlistIr) {
        if ir.is_empty() {
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
