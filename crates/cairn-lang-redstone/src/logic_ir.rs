//! Logic IR: the edition-neutral, zero-delay signal dependency graph.
//!
//! Sits between the Intent IR (`cairn-lang-core::intent`) and the Netlist IR. The synth pass
//! collects sensor bindings as [`InputPort`]s, actuator bindings as [`OutputPort`]s, and lowers
//! every `logic sig.X = <expr>` line into a DAG of [`GateNode`]s. Delay is not carried here: per
//! `spec/redstone` "Time model" it is first determined in the Placement IR.
//!
//! The IR is `Serialize` for the CLI's `synth --experimental-logic-synth` JSON dump. Nodes live in
//! arenas ([`SignalRef`] indexes [`LogicIr::inputs`] or [`LogicIr::nodes`]) in topological order,
//! so a cycle cannot be encoded; the synth pass rejects cyclic `logic` definitions before building
//! one.

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

/// A reference to a signal source, resolved to either a top-level input port
/// (a sensor emitting `-> sig.X`) or a gate output (a `logic sig.X = ...` line).
///
/// The two variants index separate arenas, so a consumer dispatches on the variant alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum SignalRef {
    /// Index into [`LogicIr::inputs`].
    Input(u32),
    /// Index into [`LogicIr::nodes`].
    Gate(u32),
}

/// Boolean primitive a [`GateNode`] represents, together with its operands.
///
/// Only `And2`, `Or2` and `Not` have surface syntax; the other variants are reserved and no synth
/// path produces them. Every variant owns its operands inline, so a `Not` gate with two inputs is
/// not a representable value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum GateKind {
    /// `a and b`.
    And2 {
        /// Left operand.
        a: SignalRef,
        /// Right operand.
        b: SignalRef,
    },
    /// `a or b`.
    Or2 {
        /// Left operand.
        a: SignalRef,
        /// Right operand.
        b: SignalRef,
    },
    /// `not a`.
    Not {
        /// Sole operand.
        a: SignalRef,
    },
    /// `xor(a, b)`. Reserved: no surface syntax produces it.
    Xor2 {
        /// Left operand.
        a: SignalRef,
        /// Right operand.
        b: SignalRef,
    },
    /// `nand(a, b)`. Reserved: no surface syntax produces it.
    Nand2 {
        /// Left operand.
        a: SignalRef,
        /// Right operand.
        b: SignalRef,
    },
    /// `nor(a, b)`. Reserved: no surface syntax produces it.
    Nor2 {
        /// Left operand.
        a: SignalRef,
        /// Right operand.
        b: SignalRef,
    },
    /// `mux(sel=..., a=..., b=...)`. Reserved: no surface syntax produces it.
    Mux {
        /// Select operand — chooses between `a` and `b`.
        sel: SignalRef,
        /// Value forwarded when `sel` is low.
        a: SignalRef,
        /// Value forwarded when `sel` is high.
        b: SignalRef,
    },
}

impl GateKind {
    /// Passes each operand of this gate to `sink`, in declaration order (`sel` first for `Mux`).
    ///
    /// A callback rather than an iterator, so the reachability walk can push onto one shared stack.
    pub fn each_input(self, mut sink: impl FnMut(SignalRef)) {
        match self {
            Self::And2 { a, b }
            | Self::Or2 { a, b }
            | Self::Xor2 { a, b }
            | Self::Nand2 { a, b }
            | Self::Nor2 { a, b } => {
                sink(a);
                sink(b);
            }
            Self::Not { a } => sink(a),
            Self::Mux { sel, a, b } => {
                sink(sel);
                sink(a);
                sink(b);
            }
        }
    }
}

/// One combinational gate in the DAG of [`LogicIr::nodes`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateNode {
    /// Boolean primitive this node computes, with its operands inline.
    pub kind: GateKind,
    /// Byte range of the enclosing `logic` line, since the AST carries no per-expression spans.
    #[serde(skip)]
    pub span: Span,
}

/// A sensor-driven signal source (`pressure_plate ... -> sig.step`, etc.).
///
/// Holds the signal name rather than the sensor member, so the port shape does not depend on the
/// sensor kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputPort {
    /// Dotted signal reference the sensor emits (e.g. `sig.step`).
    pub name: DottedRef,
    /// Byte range of the `-> sig.X` binding, which anchors a diagnostic about the sensor row
    /// rather than the `logic` line that reads it.
    #[serde(skip)]
    pub span: Span,
}

/// An actuator-driven signal sink (`door[id=front] opened_by=sig.open`, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputPort {
    /// Dotted signal reference the actuator consumes (e.g. `sig.open`).
    pub name: DottedRef,
    /// The signal driving this port, resolved through [`LogicIr::signal_defs`]. A port with no
    /// driver is never stored; synth reports `E_LOGIC_UNBOUND_SIGNAL` instead.
    pub driver: SignalRef,
    /// Byte range of the originating `opened_by=` (or equivalent) argument.
    #[serde(skip)]
    pub span: Span,
}

/// The Logic IR for one struct/def/site body.
///
/// One per scope where `logic`, sensor and actuator bindings may appear (`spec/redstone` "Signal
/// binding"), collected for a module in a [`ScopedLogicIr`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogicIr {
    /// Sensor-emitted signals feeding this scope, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<InputPort>,
    /// Actuator-consumed signals in this scope, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<OutputPort>,
    /// Combinational gates in topological (definition) order: every operand is an [`InputPort`] or
    /// an earlier entry of this vector, so a consumer maps the DAG in one forward pass. The synth
    /// pass is the only builder that keeps this.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<GateNode>,
    /// `sig.NAME` → the input or gate that defines it, for sensors and `logic` lines alike.
    ///
    /// Kept in declaration order so the dump reads in source order. Serialises as a JSON object
    /// keyed by the dotted name, e.g. `"sig.step": {"kind":"input","index":0}`.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "serialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, SignalRef>,
}

/// Serialises a `signal_defs` map as a JSON object keyed by the dotted name, because a
/// [`DottedRef`] key would serialise as an array, which JSON does not allow as a key.
///
/// Relies on `DottedRef::to_string` being injective on the keys, which holds because synth inserts
/// each `sig.X` once (`E_LOGIC_MULTIPLE_DRIVERS` refuses a second).
pub(crate) fn serialize_signal_defs<S: Serializer, V: Serialize>(
    defs: &IndexMap<DottedRef, V>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(defs.len()))?;
    for (name, value) in defs {
        map.serialize_entry(&name.to_string(), value)?;
    }
    map.end()
}

impl LogicIr {
    /// An empty Logic IR.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            nodes: Vec::new(),
            signal_defs: IndexMap::new(),
        }
    }

    /// `true` when the IR has no inputs, outputs or gates; [`ScopedLogicIr::push`] drops such a
    /// scope.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty() && self.nodes.is_empty()
    }
}

impl Default for LogicIr {
    fn default() -> Self {
        Self::new()
    }
}

/// The kind of scope an IR came from, which tells apart a struct and a def that share a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    /// `struct NAME` scope.
    Struct,
    /// `def NAME` scope.
    Def,
    /// `site NAME` scope.
    Site,
}

impl ScopeKind {
    /// The scope's keyword: `"struct"`, `"def"` or `"site"`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Def => "def",
            Self::Site => "site",
        }
    }
}

/// One IR per scope of a `.crn` module that produced a non-empty one, in source order.
///
/// A list rather than a map: `(kind, name)` is not unique, since duplicate scope names are
/// `cairn-lang-core`'s to report. A consumer that needs lookup builds its own map and picks its
/// duplicate policy.
///
/// Each IR layer names its own instance ([`ScopedLogicIr`], [`ScopedNetlistIr`],
/// [`ScopedEditionNetlistIr`], [`ScopedPlacementIr`]) and gives it the `push` that knows which of
/// its IRs count as empty.
///
/// [`ScopedNetlistIr`]: crate::netlist_ir::ScopedNetlistIr
/// [`ScopedEditionNetlistIr`]: crate::edition_netlist_ir::ScopedEditionNetlistIr
/// [`ScopedPlacementIr`]: crate::placement_ir::ScopedPlacementIr
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Scoped<T> {
    /// Per-scope IR, in source order across the whole module.
    pub scopes: Vec<ScopedEntry<T>>,
}

// Written out because the derive would ask `T: Default`, which a
// `PlacementIr` (it needs an edition) does not offer.
impl<T> Default for Scoped<T> {
    fn default() -> Self {
        Self { scopes: Vec::new() }
    }
}

impl<T> Scoped<T> {
    /// An empty list.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` when no scope produced a non-empty IR.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

/// One entry in a [`Scoped`] list; a struct so the JSON form has labelled fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopedEntry<T> {
    /// Which family of scope this IR came from.
    pub kind: ScopeKind,
    /// Source-level name of the scope (`gatehouse`, `entry`, ...).
    pub name: String,
    /// The scope's IR.
    pub ir: T,
}

/// Every scope's [`LogicIr`], in source order across the whole module.
pub type ScopedLogicIr = Scoped<LogicIr>;

/// One entry in a [`ScopedLogicIr`]: the Logic IR synthesised from one
/// scope's body.
pub type ScopedLogicIrEntry = ScopedEntry<LogicIr>;

impl ScopedLogicIr {
    /// Appends one scope's Logic IR, skipping an empty one so a module without redstone dumps as
    /// `[]`.
    pub fn push(&mut self, kind: ScopeKind, name: String, ir: LogicIr) {
        if ir.is_empty() {
            return;
        }
        self.scopes.push(ScopedLogicIrEntry { kind, name, ir });
    }
}
