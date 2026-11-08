//! Logic IR: the edition-neutral signal DAG, without ticks, the synth pass builds from sensor,
//! actuator and `logic` bindings. Nodes are in topological order, so a cycle is not encodable; a
//! macro's feedback stays inside its gate.

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::de;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A signal source: a top-level input port or a gate output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum SignalRef {
    /// Index into [`LogicIr::inputs`].
    Input(u32),
    /// Index into [`LogicIr::nodes`].
    Gate(u32),
}

/// Primitive a [`GateNode`] represents, with its operands and stage count inline.
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
    /// `a xor b`.
    Xor2 {
        /// Left operand.
        a: SignalRef,
        /// Right operand.
        b: SignalRef,
    },
    /// `a nand b`.
    Nand2 {
        /// Left operand.
        a: SignalRef,
        /// Right operand.
        b: SignalRef,
    },
    /// `a nor b`.
    Nor2 {
        /// Left operand.
        a: SignalRef,
        /// Right operand.
        b: SignalRef,
    },
    /// `mux(sel=…, a=…, b=…)`.
    Mux {
        /// Select operand — chooses between `a` and `b`.
        sel: SignalRef,
        /// Value forwarded when `sel` is low.
        a: SignalRef,
        /// Value forwarded when `sel` is high.
        b: SignalRef,
    },
    /// `latch(set=…, reset=…)`.
    Latch {
        /// Turns the latch on.
        set: SignalRef,
        /// Turns the latch off, winning over `set`.
        reset: SignalRef,
    },
    /// `delay(a, stages)`.
    Delay {
        /// Sole operand.
        a: SignalRef,
        /// Repeaters in series.
        stages: u32,
    },
    /// `pulse(a, stages)`.
    Pulse {
        /// Sole operand.
        a: SignalRef,
        /// Stages the pulse lasts at most.
        stages: u32,
    },
    /// `edge_rising(a)`.
    EdgeRising {
        /// Sole operand.
        a: SignalRef,
    },
    /// `edge_falling(a)`.
    EdgeFalling {
        /// Sole operand.
        a: SignalRef,
    },
    /// `counter(a, stages)`.
    Counter {
        /// Sole operand, counted as it turns on.
        a: SignalRef,
        /// Times `a` turns on between two toggles.
        stages: u32,
    },
}

impl GateKind {
    /// Passes each operand of this gate to `sink`, `sel` first for `Mux` and `set` for `Latch`.
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
            Self::Not { a }
            | Self::Delay { a, .. }
            | Self::Pulse { a, .. }
            | Self::EdgeRising { a }
            | Self::EdgeFalling { a }
            | Self::Counter { a, .. } => sink(a),
            Self::Mux { sel, a, b } => {
                sink(sel);
                sink(a);
                sink(b);
            }
            Self::Latch { set, reset } => {
                sink(set);
                sink(reset);
            }
        }
    }
}

/// One gate in the DAG of [`LogicIr::nodes`], a combinational primitive or a macro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateNode {
    /// Boolean primitive this node computes, with its operands inline.
    pub kind: GateKind,
    /// Byte range of the enclosing `logic` line; the AST has no per-expression spans.
    #[serde(skip)]
    pub span: Span,
}

/// A sensor-driven signal source (`pressure_plate ... -> sig.step`, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputPort {
    /// Dotted signal reference the sensor emits (e.g. `sig.step`).
    pub name: DottedRef,
    /// Byte range of the `-> sig.X` binding.
    #[serde(skip)]
    pub span: Span,
}

/// An actuator-driven signal sink (`door[id=front] opened_by=sig.open`, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputPort {
    /// Dotted signal reference the actuator consumes (e.g. `sig.open`).
    pub name: DottedRef,
    /// The signal driving this port, resolved through [`LogicIr::signal_defs`].
    pub driver: SignalRef,
    /// Byte range of the originating `opened_by=` (or equivalent) argument.
    #[serde(skip)]
    pub span: Span,
}

/// The Logic IR for one struct/def/site body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogicIr {
    /// Sensor-emitted signals feeding this scope, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<InputPort>,
    /// Actuator-consumed signals in this scope, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<OutputPort>,
    /// Gates in topological order: every operand is an input or an earlier gate.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<GateNode>,
    /// `sig.NAME` → the input or gate that defines it, in declaration order.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "serialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, SignalRef>,
}

/// Serialises `signal_defs` as a JSON object keyed by the dotted name; JSON keys cannot be arrays.
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

/// Reads back a [`DottedRef`] written as its list of segments.
pub(crate) fn deserialize_dotted_ref<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<DottedRef, D::Error> {
    dotted_ref_from_segments(Vec::deserialize(deserializer)?)
}

/// The [`DottedRef`] of `segments`, refusing no segment at all, an empty one, or one holding a `.`.
pub(crate) fn dotted_ref_from_segments<E: de::Error>(
    segments: Vec<String>,
) -> Result<DottedRef, E> {
    if segments.iter().any(String::is_empty) {
        return Err(E::custom(format_args!(
            "the dotted name {segments:?} has an empty segment, and every segment holds at least \
             one character"
        )));
    }
    if segments.iter().any(|segment| segment.contains('.')) {
        return Err(E::custom(format_args!(
            "the dotted name {segments:?} has a segment holding a `.`, which only ever separates \
             segments"
        )));
    }
    DottedRef::try_from_segments(segments)
        .ok_or_else(|| E::invalid_length(0, &"a dotted name of one segment or more"))
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

    /// `true` when the IR has no inputs, outputs or gates.
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

/// The kind of scope an IR came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// Per-scope IRs of a `.crn` module, in source order. `(kind, name)` is not unique: duplicate
/// scope names are core's to report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Scoped<T> {
    /// Per-scope IR, in source order.
    pub scopes: Vec<ScopedEntry<T>>,
}

// Not derived: the derive would require `T: Default`, which `CircuitLayout` lacks.
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

    /// `true` when the list holds no scope.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

/// One entry in a [`Scoped`] list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// One entry in a [`ScopedLogicIr`].
pub type ScopedLogicIrEntry = ScopedEntry<LogicIr>;

impl ScopedLogicIr {
    /// Appends one scope's Logic IR, skipping an empty one.
    pub fn push(&mut self, kind: ScopeKind, name: String, ir: LogicIr) {
        if ir.is_empty() {
            return;
        }
        self.scopes.push(ScopedLogicIrEntry { kind, name, ir });
    }
}
