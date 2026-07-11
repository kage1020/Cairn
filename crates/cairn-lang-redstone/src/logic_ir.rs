//! Logic IR — edition-neutral, zero-delay signal dependency graph.
//!
//! Sits one lowering step above the Intent IR (`cairn-lang-core::intent`) and
//! one below the Netlist IR that later PRs will grow on top. The synth pass
//! collects sensor bindings as [`InputPort`]s, actuator bindings as
//! [`OutputPort`]s, and lowers every `logic sig.X = <expr>` line into a DAG
//! of [`GateNode`]s. Delay is not carried here; per `spec/redstone` §14.4 it
//! is determined for the first time in the Placement IR, several PRs down the
//! pipeline.
//!
//! The IR is `Serialize` so callers can dump it as JSON — `cairn synth
//! --experimental-logic-synth` uses that path — and every field is spanned
//! back to source so future LSP go-to-def / rename passes can round-trip.
//!
//! Node identity is arena-based ([`SignalRef`] indexes into
//! [`LogicIr::inputs`] or [`LogicIr::nodes`]) so cycles are impossible to
//! encode structurally; the synth pass rejects cyclic `logic` definitions
//! before construction ever reaches this type.

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

/// A reference to a signal source, resolved to either a top-level input port
/// (a sensor emitting `-> sig.X`) or a gate output (a `logic sig.X = ...` line).
///
/// The index space is disjoint between the two variants so a downstream
/// consumer can dispatch by variant without cross-checking against
/// `LogicIr.inputs.len()` / `LogicIr.nodes.len()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", content = "index", rename_all = "snake_case")]
pub enum SignalRef {
    /// Index into [`LogicIr::inputs`].
    Input(u32),
    /// Index into [`LogicIr::nodes`].
    Gate(u32),
}

/// Boolean primitive a [`GateNode`] represents.
///
/// Only combinational primitives land in M6-PR1; sequential macros (`latch`,
/// `pulse`, `delay`, ...) join in a later PR once the Netlist IR and cell
/// library are in place (`spec/redstone` §14.1). `And2` / `Or2` / `Not` are
/// reachable from the current AST directly; `Xor2` / `Nand2` / `Nor2` / `Mux`
/// are pre-allocated on this enum so the follow-up PR that teaches the parser
/// call-expression syntax can slot them in without a breaking change to the
/// Logic IR shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum GateKind {
    /// `a and b`. Two inputs.
    And2,
    /// `a or b`. Two inputs.
    Or2,
    /// `not a`. One input.
    Not,
    /// `xor(a, b)`. Two inputs. Reserved for a follow-up parser PR.
    Xor2,
    /// `nand(a, b)`. Two inputs. Reserved for a follow-up parser PR.
    Nand2,
    /// `nor(a, b)`. Two inputs. Reserved for a follow-up parser PR.
    Nor2,
    /// `mux(sel=..., a=..., b=...)`. Three inputs, in that order. Reserved
    /// for a follow-up parser PR.
    Mux,
}

impl GateKind {
    /// Number of input operands this kind expects.
    ///
    /// The synth pass uses the return value to size [`GateNode::inputs`] up
    /// front and to reject a lowered `Expr` whose shape does not match a
    /// primitive's arity. Kept as a `usize` so downstream comparisons against
    /// `Vec::len` do not have to widen.
    #[must_use]
    pub const fn arity(self) -> usize {
        match self {
            Self::Not => 1,
            Self::And2 | Self::Or2 | Self::Xor2 | Self::Nand2 | Self::Nor2 => 2,
            Self::Mux => 3,
        }
    }
}

/// One combinational gate in the DAG.
///
/// The DAG is stored as a topologically ordered `Vec<GateNode>` on
/// [`LogicIr::nodes`]; `inputs[i]` is guaranteed to reference either a
/// [`SignalRef::Input`] or an earlier `SignalRef::Gate(j)` where `j <
/// this node's index`. That invariant makes a downstream simulator or
/// netlist mapping a single forward pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateNode {
    /// Boolean primitive this node computes.
    pub kind: GateKind,
    /// Operands, in the order dictated by [`GateKind`]. Length equals
    /// `kind.arity()` — the synth pass enforces the invariant at
    /// construction.
    pub inputs: Vec<SignalRef>,
    /// Byte range of the originating `logic ...` sub-expression in source.
    /// Multiple source spans that lower to the same gate (via CSE) surface
    /// the first-seen span; the deduplicated siblings can be recovered from
    /// `signal_defs` if a future LSP pass needs them.
    #[serde(skip)]
    pub span: Span,
}

/// A sensor-driven signal source (`pressure_plate ... -> sig.step`, etc.).
///
/// Owns the signal *name* rather than the sensor member itself so a later
/// PR can extend the input inventory to other sensor kinds (`lever`,
/// `button`, `observer`, ...) without changing the port shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputPort {
    /// Dotted signal reference the sensor emits (e.g. `sig.step`).
    pub name: DottedRef,
    /// Byte range of the originating `-> sig.X` binding in source. Anchors
    /// diagnostics that flag the sensor row rather than the `logic` line that
    /// consumes it.
    #[serde(skip)]
    pub span: Span,
}

/// An actuator-driven signal sink (`door[id=front] opened_by=sig.open`, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputPort {
    /// Dotted signal reference the actuator consumes (e.g. `sig.open`).
    pub name: DottedRef,
    /// Which signal drives this port. Resolved from [`LogicIr::signal_defs`]
    /// during synth; a port whose driver could not be resolved is not stored
    /// here — the synth pass emits `E_LOGIC_UNBOUND_SIGNAL` instead so an
    /// incomplete Logic IR is never handed downstream.
    pub driver: SignalRef,
    /// Byte range of the originating `opened_by=` (or equivalent) argument.
    #[serde(skip)]
    pub span: Span,
}

/// The Logic IR for one struct/def/site body.
///
/// One instance per scope where `logic` / sensor / actuator bindings can
/// legally appear (`spec/redstone` §14.2). Multi-scope files produce a
/// [`ScopedLogicIr`] map; the CLI's `synth` subcommand walks that map to
/// emit one entry per scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogicIr {
    /// Sensor-emitted signals feeding this scope, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<InputPort>,
    /// Actuator-consumed signals in this scope, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<OutputPort>,
    /// Combinational gates, in topological (definition) order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<GateNode>,
    /// `sig.NAME` → the source that defines it. Populated by both sensor
    /// registration and `logic` lowering, so downstream tooling has one
    /// place to look up "where does this signal come from".
    ///
    /// [`IndexMap`] preserves declaration order so a serialised dump reads
    /// top-to-bottom the same way the source does; a plain `HashMap` would
    /// scramble it and make snapshot testing flake.
    ///
    /// Serialises as a JSON object whose keys are the dotted name flattened
    /// with `.` (e.g. `"sig.step": {"kind":"input","index":0}`). A raw
    /// [`IndexMap<DottedRef, _>`] would serialise the key as a JSON array
    /// (the transparent form of [`DottedRef`]), which is not a legal JSON
    /// object key.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "serialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, SignalRef>,
}

fn serialize_signal_defs<S: Serializer>(
    defs: &IndexMap<DottedRef, SignalRef>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(defs.len()))?;
    for (name, sig) in defs {
        map.serialize_entry(&name.to_string(), sig)?;
    }
    map.end()
}

impl LogicIr {
    /// Empty Logic IR — no inputs, no outputs, no gates. Used as the
    /// starting point for the synth pass's accumulator and as the "nothing
    /// to synthesise" answer for scopes that carry neither sensors nor
    /// `logic` lines.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            nodes: Vec::new(),
            signal_defs: IndexMap::new(),
        }
    }

    /// `true` when the scope produced zero inputs, zero outputs, and zero
    /// gates. Used to drop empty scopes from the CLI JSON dump so a project
    /// without any redstone still emits `{}` rather than one entry per
    /// unrelated struct.
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

/// Kind of scope that produced a [`LogicIr`], used only to key the
/// [`ScopedLogicIr`] map. Kept as a distinct enum so a future collision
/// between a struct and a def sharing a name still round-trips as two
/// distinct entries.
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

/// Map of `(scope kind, scope name)` → [`LogicIr`] covering an entire
/// `.crn` module.
///
/// A module can host redstone at multiple scopes at once (a `site` has
/// `logic` lines coordinating placements, a `struct` has its own local
/// circuit). Serialising each scope separately keeps the CLI dump machine-
/// parseable without a downstream consumer having to unify the DAGs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ScopedLogicIr {
    /// Per-scope Logic IR, keyed by `(kind, name)` so the same identifier
    /// used across scope kinds stays disambiguated in the wire form.
    pub scopes: Vec<ScopedLogicIrEntry>,
}

impl ScopedLogicIr {
    /// Empty map. Equivalent to `ScopedLogicIr::default()` but spells out
    /// the intent at call sites.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one scope's Logic IR. Empty scopes are elided so a module
    /// without any redstone renders as `[]`.
    pub fn push(&mut self, kind: ScopeKind, name: String, ir: LogicIr) {
        if ir.is_empty() {
            return;
        }
        self.scopes.push(ScopedLogicIrEntry { kind, name, ir });
    }

    /// `true` when no scope produced a non-empty Logic IR.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

/// One entry in a [`ScopedLogicIr`]. Kept as a struct rather than a tuple
/// so the JSON wire form carries labelled fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopedLogicIrEntry {
    /// Which family of scope this Logic IR came from.
    pub kind: ScopeKind,
    /// Source-level name of the scope (`gatehouse`, `entry`, ...).
    pub name: String,
    /// Logic IR synthesised from the scope's body.
    pub ir: LogicIr,
}
