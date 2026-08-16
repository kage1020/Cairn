//! Netlist IR — edition-neutral cells + nets over the Logic IR DAG.
//!
//! Sits one lowering step above the [Logic IR](crate::logic_ir) and one
//! below the Placement IR that later passes will grow on top. The netlist
//! pass ([`crate::netlist::compile_netlist`]) walks every
//! [`crate::logic_ir::ScopedLogicIrEntry`] once and rewrites each
//! [`crate::logic_ir::GateNode`] into a [`CellNode`] tagged with a
//! [`LogicalCell`] — the top of the three-tier cell library documented in
//! `spec/redstone` §14.6 (`Logical Cell → Edition Cell → Physical Tile`).
//! The Java `ComparatorAND` vs Bedrock `TorchAND` split is *not* decided
//! here; that is the Edition Cell selection a follow-up pass will run
//! against a target [`cairn_lang_core::Edition`].
//!
//! Node identity is arena-based ([`NetRef`] indexes into
//! [`NetlistIr::inputs`] or [`NetlistIr::cells`]); the pipeline
//! deliberately mirrors the Logic IR's [`crate::logic_ir::SignalRef`]
//! shape so a downstream simulator can share the same forward-walk
//! skeleton across both IRs. Delay is not carried — per `spec/redstone`
//! §14.4 / §14.8 delay is first determined in the Placement IR.

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::logic_ir::ScopeKind;

/// A reference to a driving net, resolved to either a top-level input port
/// (a sensor emitting `-> sig.X`) or a cell output.
///
/// Index spaces are disjoint between the two variants so a downstream
/// consumer can dispatch by variant without cross-checking against
/// [`NetlistIr::inputs`] or [`NetlistIr::cells`] lengths — the same
/// invariant [`crate::logic_ir::SignalRef`] carries at the Logic IR layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
#[non_exhaustive]
pub enum NetRef {
    /// Index into [`NetlistIr::inputs`].
    Input(u32),
    /// Index into [`NetlistIr::cells`] — the output net of that cell.
    Cell(u32),
}

/// Logical cell chosen for a [`CellNode`]. Edition-neutral by contract:
/// the same [`LogicalCell::And`] value lowers to Java `ComparatorAND` or
/// Bedrock `TorchAND` at a later pass (`spec/redstone` §14.6).
///
/// `#[non_exhaustive]` for two reasons: (1) the combinational variants
/// `Xor` / `Nand` / `Nor` / `Mux` reserved on `GateKind` today are
/// unreachable until a follow-up parser change teaches the surface
/// call-expression form, and (2) the sequential-macro cells reserved by
/// `spec/redstone` §14.1 (`latch` / `pulse` / `delay` / `edge_rising` /
/// `edge_falling` / `counter`) will join once the synth path grows to
/// emit them. Both add-in paths should stay non-breaking for downstream
/// exhaust matches.
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
    /// Two-input XOR. Reserved — reachable once the parser grows the
    /// call-expression form.
    Xor,
    /// Two-input NAND. Reserved for the same follow-up.
    Nand,
    /// Two-input NOR. Reserved for the same follow-up.
    Nor,
    /// Two-to-one multiplexer (`sel` picks between `a` and `b`).
    /// Reserved for the same follow-up.
    Mux,
}

/// Input-port name on a [`CellNode`]. Kept as a small closed enum so a
/// consumer that renders wiring diagrams does not have to parse strings.
///
/// The port set is a superset over every current [`LogicalCell`]; each
/// cell picks its subset (`Not` uses just `A`; `Mux` uses `Sel` / `A` /
/// `B`). `#[non_exhaustive]` so future cells (e.g. a `counter` with
/// `reset` / `enable` ports) can add ports without breakage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PortName {
    /// First data input (`a` operand).
    A,
    /// Second data input (`b` operand).
    B,
    /// `Mux` select line.
    Sel,
    /// Not an input port: the segment leaving a driver for an
    /// actuator's output pad. A cell's ports name the wires that feed
    /// it, and that wire needs a name too — it carries buffer
    /// repeaters like any other, and [`crate::BufferCoord`] attributes
    /// each buffer to the segment it stands on.
    Out,
}

/// One `(port name, driving net)` pair on a [`CellNode`]. Encoded as a
/// struct rather than a tuple so the JSON wire form carries labelled
/// fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CellPortDriver {
    /// Which input port this driver feeds.
    pub port: PortName,
    /// Net driving the port.
    pub net: NetRef,
}

/// One combinational cell in the DAG.
///
/// The DAG is stored as a topologically ordered `Vec<CellNode>` on
/// [`NetlistIr::cells`]; every driver [`NetRef`] is either a
/// [`NetRef::Input`] or an earlier [`NetRef::Cell(j)`] where `j` is
/// strictly less than this node's index. That mirrors the Logic IR
/// invariant and makes any downstream simulator or placer a single
/// forward pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CellNode {
    /// Logical cell this node computes.
    pub cell: LogicalCell,
    /// Port → net drivers, in each cell's canonical port order:
    /// two-input gates emit `[A, B]`; `Not` emits `[A]`;
    /// `Mux` emits `[Sel, A, B]`.
    pub drivers: Vec<CellPortDriver>,
    /// Byte range of the originating `logic ...` sub-expression, inherited
    /// from the source [`crate::logic_ir::GateNode`]. Anchors diagnostics
    /// a later placement or route pass may emit.
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
    /// Which net drives this port. Resolved from the source
    /// [`crate::logic_ir::OutputPort::driver`].
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
    /// Combinational cells in topological (definition) order — every
    /// driver references either an input or an earlier entry in this
    /// vector.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<CellNode>,
    /// `sig.NAME` → the net that defines it, mirroring
    /// [`crate::logic_ir::LogicIr::signal_defs`] after the
    /// [`crate::logic_ir::SignalRef`] → [`NetRef`] rewrite.
    ///
    /// [`IndexMap`] preserves declaration order so a serialised dump
    /// reads top-to-bottom the same way the source does. Serialises as a
    /// JSON object keyed by the flattened dotted name (e.g.
    /// `"sig.step": {"kind":"input","index":0}`) — the same custom
    /// serialisation the Logic IR uses for its `signal_defs` field, kept
    /// in sync so downstream JSON consumers see a uniform shape across
    /// both IRs.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "serialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, NetRef>,
}

/// Serialise `signal_defs` as a JSON object keyed by the dotted signal
/// name flattened with `.`. Relies on [`DottedRef::to_string`] being
/// injective on the value space that reaches this map — the synth pass
/// only inserts distinct `sig.X` names (a second insert would already
/// have surfaced `E_LOGIC_MULTIPLE_DRIVERS`), so distinct
/// [`DottedRef`] keys map to distinct string keys and no entry is
/// silently overwritten.
fn serialize_signal_defs<S: Serializer>(
    defs: &IndexMap<DottedRef, NetRef>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(defs.len()))?;
    for (name, net) in defs {
        map.serialize_entry(&name.to_string(), net)?;
    }
    map.end()
}

impl NetlistIr {
    /// Empty Netlist IR — no inputs, no outputs, no cells.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            cells: Vec::new(),
            signal_defs: IndexMap::new(),
        }
    }

    /// `true` when this scope produced zero inputs, zero outputs, and
    /// zero cells.
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

/// Ordered list of `(scope kind, scope name)` → [`NetlistIr`] entries
/// covering an entire `.crn` module.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ScopedNetlistIr {
    /// Per-scope Netlist IR, in source order across the whole module.
    pub scopes: Vec<ScopedNetlistIrEntry>,
}

impl ScopedNetlistIr {
    /// Empty list.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one scope's Netlist IR. Empty scopes are elided so a
    /// module without any redstone renders as `[]`, matching the Logic
    /// IR's elision.
    pub fn push(&mut self, kind: ScopeKind, name: String, ir: NetlistIr) {
        if ir.is_empty() {
            // The synth pass only ever writes to `signal_defs` alongside
            // an input / output / gate insertion, so a leftover
            // `signal_defs` entry with no owner would be a synth-side
            // regression. Cheap to check here so a stale IR doesn't
            // silently reach downstream passes.
            debug_assert!(
                ir.signal_defs.is_empty(),
                "empty NetlistIr carries orphan signal_defs entries: {:?}",
                ir.signal_defs.keys().collect::<Vec<_>>(),
            );
            return;
        }
        self.scopes.push(ScopedNetlistIrEntry { kind, name, ir });
    }

    /// `true` when no scope produced a non-empty Netlist IR.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

/// One entry in a [`ScopedNetlistIr`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopedNetlistIrEntry {
    /// Which family of scope this Netlist IR came from.
    pub kind: ScopeKind,
    /// Source-level name of the scope.
    pub name: String,
    /// Netlist IR synthesised from the scope's Logic IR.
    pub ir: NetlistIr,
}
