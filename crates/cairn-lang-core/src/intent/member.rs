//! Named member of an Intent IR.
//!
//! Mirrors the spec's "rich member with invariants" (see `architecture.md`
//! §3.2): every member knows its [`MemberRole`], may carry an `id` / `class` /
//! `mat_slot`, holds an [`IntentState`] of raw `key=value` attributes, and
//! reserves a [`ResolvedState`] slot that M3 fills in once materials and
//! themes resolve.

use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::Value;
use crate::error::Span;

use super::{AssertIr, LogicBinding};

/// One named member produced by lowering a body statement.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Member {
    /// Explicit `id=` value, if any. M2 does not auto-assign addresses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Explicit `class=` grouping, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// What kind of member this is. Classified from the surface command
    /// keyword via [`super::keyword_table::role_of`]; unknown keywords land
    /// in [`MemberRole::Other`].
    pub role: MemberRole,
    /// Explicit `mat_slot=` reference into the theme's slot table, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mat_slot: Option<String>,
    /// `[attr=value]` selector immediately after the keyword
    /// (`door[id=front] ...`). Carried through verbatim; later passes decide
    /// whether the selector is binding a fresh id or referencing an existing
    /// member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<IndexMap<String, ValueWithSpan>>,
    /// Bare positional values appearing before the first `key=` argument.
    /// Empty for almost every command; non-empty only for special forms
    /// such as `connect <ref> to <ref>` whose surface grammar mandates them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub positional: Vec<Value>,
    /// `-> VALUE` tail at the end of a line, e.g.
    /// `pressure_plate ... -> sig.step`. Always carried separately from
    /// [`Self::intent_state`] so the resolver's reference pass can match
    /// it without round-tripping through a string key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<Value>,
    /// `key=value` attributes that were not consumed by the dedicated fields
    /// above. M3 will type these against the per-keyword schema.
    pub intent_state: IntentState,
    /// Per-edition resolved blockstate. Always `None` at semantic level
    /// `Grouped`; populated by the M3 lift pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_state: Option<ResolvedState>,
    /// Nested body indented under this member.
    ///
    /// A `level y=...` (or any other grouping keyword) may declare further
    /// members, `logic` bindings, and `assert` properties beneath it. Holding
    /// the children as a [`MemberBody`] — the same triple the top-level IR
    /// uses — keeps each of those three statement flavours addressable
    /// instead of forcing the caller to either reject or silently drop the
    /// non-member ones. M2's `lower` is total, so the only structural
    /// invariant maintained here is "children sit in their declaration
    /// order, grouped by kind".
    #[serde(default, skip_serializing_if = "MemberBody::is_empty")]
    pub children: MemberBody,
    /// Byte range of the originating `Statement::Generic` line in the source
    /// (children excluded). Diagnostic passes anchor `E_UNKNOWN_KEYWORD` and
    /// similar messages here.
    #[serde(skip)]
    pub span: Span,
}

/// Coarse classification of a member, derived from its source keyword.
///
/// Not marked `#[non_exhaustive]` on purpose: the [`MemberRole::Other`]
/// variant already forces every external `match` to handle the "any keyword
/// we don't know" case, so a non-exhaustive marker would only suppress the
/// internal exhaustiveness check we rely on to catch missed updates when a
/// new keyword is added to the M2 roster.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum MemberRole {
    /// `floor` command.
    Floor,
    /// `walls` command.
    Walls,
    /// `door` command.
    Door,
    /// `window` command.
    Window,
    /// `roof` command.
    Roof,
    /// `stair` command.
    Stair,
    /// `level` grouping block.
    Level,
    /// `pressure_plate` fixture.
    PressurePlate,
    /// `circuit` redstone region marker.
    Circuit,
    /// `place` site placement.
    Place,
    /// `connect` site topology edge.
    Connect,
    /// Any keyword the M2 table does not know about. Validation passes (M2
    /// PR2+) report these via `E_UNKNOWN_KEYWORD`; lower itself never fails.
    Other(String),
}

/// Sub-body indented under a [`Member`] (typically a `level y=...` block).
///
/// Mirrors the three statement flavours [`super::StructIr`] groups at the
/// top level: ordinary member commands, `logic` bindings, and `assert`
/// properties. Defined as a struct rather than a `Vec` of one big sum so a
/// `level` block whose body mixes all three flavours lowers without losing
/// any of them (spec lint.md §11.2 forbids silent dropping).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MemberBody {
    /// Nested members in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<Member>,
    /// `logic` bindings declared inside the sub-body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logic: Vec<LogicBinding>,
    /// `assert` properties declared inside the sub-body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asserts: Vec<AssertIr>,
}

impl MemberBody {
    /// `true` when no members, logic bindings, or asserts have been added.
    /// Used to elide empty bodies from snapshot output.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty() && self.logic.is_empty() && self.asserts.is_empty()
    }
}

/// Raw `key=value` attributes attached to a [`Member`].
///
/// Stored as an [`IndexMap`] so source order is preserved — diagnostic
/// messages emitted in later M2 PRs need stable positions, and any
/// snapshot-based test would otherwise flake. Each entry is wrapped in
/// [`ValueWithSpan`] so the diagnostic-collecting `check` passes can point at
/// the offending byte range without re-walking the AST to recover positions.
/// `Deref` / `DerefMut` against the underlying map let callers write
/// `state.insert(...)` / `state.contains_key(...)` without threading through
/// a `.fields` accessor.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct IntentState {
    /// Attributes in source order.
    pub fields: IndexMap<String, ValueWithSpan>,
}

impl IntentState {
    /// Construct an empty intent state. Equivalent to
    /// `IntentState::default()` but spells out the intent at call sites.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Deref for IntentState {
    type Target = IndexMap<String, ValueWithSpan>;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

impl DerefMut for IntentState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.fields
    }
}

/// A [`Value`] together with its source byte range.
///
/// The `Value` itself already owns a `span` field (`#[serde(skip)]`); this
/// wrapper exists so [`IntentState`] and the IR's per-keyword `args` maps can
/// stay `IndexMap<String, ValueWithSpan>` even after the lowering step copies
/// the value out of the AST. Keeping the span next to the value at the IR
/// level means `check` passes can point at the exact byte range without
/// re-correlating with the AST.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ValueWithSpan {
    /// Underlying value.
    pub value: Value,
    /// Byte range of the value in the original source. Equal to
    /// `self.value.span()` for every value produced by `lower`; carried as a
    /// separate field so future code paths can mint a `ValueWithSpan` that
    /// extends the surrounding `key=value` range without rebuilding the
    /// `Value` tree.
    #[serde(skip)]
    pub span: Span,
}

impl ValueWithSpan {
    /// Build a [`ValueWithSpan`] using the value's own span as the wrapper
    /// span. The common case at the IR boundary.
    #[must_use]
    pub fn from_value(value: Value) -> Self {
        let span = value.span().clone();
        Self { value, span }
    }
}

/// Resolved, per-edition blockstate for a [`Member`].
///
/// Empty in M2 — only the shape is fixed so M3's lift pass can populate
/// concrete state without a breaking change. Uses the same `Deref` /
/// `DerefMut` ergonomics as [`IntentState`].
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ResolvedState {
    /// Resolved state fields in source-stable order.
    pub fields: IndexMap<String, ValueWithSpan>,
}

impl Deref for ResolvedState {
    type Target = IndexMap<String, ValueWithSpan>;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

impl DerefMut for ResolvedState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.fields
    }
}
