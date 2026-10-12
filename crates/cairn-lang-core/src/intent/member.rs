//! Named member of an Intent IR, the rich member of `spec/architecture`
//! "The Intent IR is rich and carries invariants".

use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{Value, ValueKind};
use crate::error::Span;

use super::{AssertIr, LogicBinding, MemberRole};

/// One named member produced by lowering a body statement.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Member {
    /// Explicit `id=` value, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Explicit `class=` grouping, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// What kind of member this is; unknown keywords are [`MemberRole::Other`].
    pub role: MemberRole,
    /// Explicit `mat_slot=` slot name, if any; [`Self::read_slot`] says whether it is read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mat_slot: Option<Label>,
    /// `[attr=value]` selector immediately after the keyword, verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<IndexMap<String, ValueWithSpan>>,
    /// Bare values on the line in source order, wherever they sit.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub positional: Vec<Value>,
    /// `-> VALUE` tail at the end of the line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<Value>,
    /// `key=value` attributes the fields above did not consume.
    pub intent_state: IntentState,
    /// Per-edition resolved blockstate; `None` until the member is `Lifted`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_state: Option<ResolvedState>,
    /// Nested body indented under this member.
    #[serde(default, skip_serializing_if = "MemberBody::is_empty")]
    pub children: MemberBody,
    /// Byte range of the member's own line, children excluded.
    #[serde(skip)]
    pub span: Span,
}

impl Member {
    /// The slot name this member reads from a theme: its [`Self::mat_slot`], unless its role's
    /// [`MemberRole::unread_arguments`] lists `mat_slot`. Ask this, not the field.
    #[must_use]
    pub fn read_slot(&self) -> Option<&str> {
        self.mat_slot
            .as_ref()
            .filter(|_| self.role.unread_argument("mat_slot").is_none())
            .map(|slot| slot.name.as_str())
    }

    /// Read `key=` as a non-negative `u32`, or `None`.
    #[must_use]
    pub fn nonneg_u32(&self, key: &str) -> Option<u32> {
        match &self.intent_state.get(key)?.value.kind {
            ValueKind::Int(v) if *v >= 0 => u32::try_from(*v).ok(),
            _ => None,
        }
    }

    /// Read `key=` as a bare identifier, or `None`.
    #[must_use]
    pub fn ident_value(&self, key: &str) -> Option<&str> {
        match &self.intent_state.get(key)?.value.kind {
            ValueKind::Ident(name) => Some(name.as_str()),
            _ => None,
        }
    }
}

/// Which top-level body a member sits in, and so which roles the later passes have a reader for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// A `struct` or `def` body — one building's geometry.
    Geometry,
    /// A `site` body — a flat list of `place` and `connect` rows.
    Site,
}

impl BodyKind {
    /// Surface keyword(s) naming this body, for prose.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Geometry => "`struct` or `def`",
            Self::Site => "`site`",
        }
    }

    /// Keywords this body has a reader for, in [`super::known_keywords`] order.
    #[must_use]
    pub fn allowed_keywords(self) -> Vec<&'static str> {
        super::known_keywords()
            .iter()
            .copied()
            .filter(|kw| super::role_of(kw).is_read_in(self))
            .collect()
    }
}

impl MemberRole {
    /// Whether a body of this kind has anything that reads this role; `false` for
    /// [`MemberRole::Other`], which `check::keyword_allowlist` reports instead.
    #[must_use]
    pub fn is_read_in(&self, body: BodyKind) -> bool {
        match (body, self) {
            (
                BodyKind::Geometry,
                Self::Floor
                | Self::Walls
                | Self::Door
                | Self::Window
                | Self::Roof
                | Self::Stair
                | Self::Level
                | Self::PressurePlate
                | Self::Circuit,
            )
            | (BodyKind::Site, Self::Place | Self::Connect) => true,
            (BodyKind::Geometry, Self::Place | Self::Connect)
            | (
                BodyKind::Site,
                Self::Floor
                | Self::Walls
                | Self::Door
                | Self::Window
                | Self::Roof
                | Self::Stair
                | Self::Level
                | Self::PressurePlate
                | Self::Circuit,
            )
            | (_, Self::Other(_)) => false,
        }
    }

    /// Surface keyword this role was classified from, the inverse of [`super::role_of`].
    #[must_use]
    pub fn keyword(&self) -> &str {
        match self {
            Self::Floor => "floor",
            Self::Walls => "walls",
            Self::Door => "door",
            Self::Window => "window",
            Self::Roof => "roof",
            Self::Stair => "stair",
            Self::Level => "level",
            Self::PressurePlate => "pressure_plate",
            Self::Circuit => "circuit",
            Self::Place => "place",
            Self::Connect => "connect",
            Self::Other(keyword) => keyword,
        }
    }
}

/// Which end of a `connect FROM.PORT to TO.PORT` row a positional value occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectEnd {
    /// The `FROM.PORT` slot, before the `to` keyword.
    From,
    /// The `TO.PORT` slot, after the `to` keyword.
    To,
}

impl ConnectEnd {
    /// Bare name of the end, for prose that supplies its own framing (`the {} endpoint`).
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::From => "from",
            Self::To => "to",
        }
    }
}

/// Sub-body indented under a [`Member`], keeping members, `logic` and `assert` lines apart.
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
    /// `true` when it holds no members, logic bindings or asserts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty() && self.logic.is_empty() && self.asserts.is_empty()
    }
}

/// Raw `key=value` attributes attached to a [`Member`], in source order.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct IntentState {
    /// Attributes in source order.
    pub fields: IndexMap<String, ValueWithSpan>,
}

impl IntentState {
    /// Construct an empty intent state.
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

/// A [`Value`] with its source byte range; unlike [`Value`]'s, this equality includes the span.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ValueWithSpan {
    /// Underlying value.
    pub value: Value,
    /// Byte range of the value in the original source.
    #[serde(skip)]
    pub span: Span,
}

impl ValueWithSpan {
    /// Wrap `value` with its own span.
    #[must_use]
    pub fn from_value(value: Value) -> Self {
        let span = value.span().clone();
        Self { value, span }
    }
}

/// An identifier or string hoisted out of a member's arguments, with where it was written.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Label {
    /// The text of the label, without quotes for a string.
    pub name: String,
    /// Byte range of the value in the original source.
    #[serde(skip)]
    pub span: Span,
}

/// Resolved, per-edition blockstate for a [`Member`]; always empty for now.
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
