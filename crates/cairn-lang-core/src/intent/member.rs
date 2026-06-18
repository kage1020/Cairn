//! Named member of an Intent IR.
//!
//! Mirrors the spec's "rich member with invariants" (see `architecture.md`
//! §3.2): every member knows its [`MemberRole`], may carry an `id` / `class` /
//! `mat_slot`, holds an [`IntentState`] of raw `key=value` attributes, and
//! reserves a [`ResolvedState`] slot that M3 fills in once materials and
//! themes resolve.

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::Value;

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
    pub selector: Option<IndexMap<String, Value>>,
    /// Bare positional values appearing before the first `key=` argument.
    /// Empty for almost every command; non-empty only for special forms
    /// such as `connect <ref> to <ref>` whose surface grammar mandates them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub positional: Vec<Value>,
    /// `-> VALUE` tail at the end of a line, e.g.
    /// `pressure_plate ... -> sig.step`. Always carried separately from
    /// [`Self::intent_state`] so the M2-PR3 `reference` pass can resolve it
    /// without round-tripping through a string key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<Value>,
    /// `key=value` attributes that were not consumed by the dedicated fields
    /// above. M3 will type these against the per-keyword schema.
    pub intent_state: IntentState,
    /// Per-edition resolved blockstate. Always `None` at semantic level
    /// `Grouped`; populated by the M3 lift pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_state: Option<ResolvedState>,
    /// Nested members from indented child statements (e.g. members declared
    /// under a `level y=...` block).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Member>,
}

/// Coarse classification of a member, derived from its source keyword.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value")]
#[non_exhaustive]
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

/// Raw `key=value` attributes attached to a [`Member`].
///
/// Stored as an [`IndexMap`] so source order is preserved — diagnostic
/// messages emitted in later M2 PRs need stable positions, and any
/// snapshot-based test would otherwise flake.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct IntentState {
    /// Attributes in source order.
    pub fields: IndexMap<String, Value>,
}

impl IntentState {
    /// Construct an empty intent state. Equivalent to
    /// `IntentState::default()` but spells out the intent at call sites.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Resolved, per-edition blockstate for a [`Member`].
///
/// Empty in M2 — only the shape is fixed so M3's lift pass can populate
/// concrete state without a breaking change.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ResolvedState {
    /// Resolved state fields in source-stable order.
    pub fields: IndexMap<String, Value>,
}
