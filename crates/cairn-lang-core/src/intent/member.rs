//! Named member of an Intent IR.
//!
//! Mirrors the spec's "rich member with invariants" (see `spec/architecture`
//! "The Intent IR is rich and carries invariants"): every member knows its
//! [`MemberRole`], may carry an `id` / `class` / `mat_slot`, holds an
//! [`IntentState`] of raw `key=value` attributes, and reserves a
//! [`ResolvedState`] slot that the lift pass fills in once materials and themes
//! resolve.

use std::ops::{Deref, DerefMut};

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{Value, ValueKind};
use crate::error::Span;

use super::{AssertIr, LogicBinding};

/// One named member produced by lowering a body statement.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Member {
    /// Explicit `id=` value, if any. Lowering assigns no addresses, so an unset `id` stays unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Explicit `class=` grouping, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// What kind of member this is. Classified from the surface command
    /// keyword via [`super::keyword_table::role_of`]; unknown keywords land
    /// in [`MemberRole::Other`].
    pub role: MemberRole,
    /// Explicit `mat_slot=` slot name, if any, with where its value was written.
    ///
    /// On a role that reads one, the name the resolver looks up in the bound theme's slot table. On
    /// a role whose [`MemberRole::unread_arguments`] lists `mat_slot` it refers to nothing: the
    /// resolver does not look it up, and `check::arguments` reports it as ignored, at the position
    /// this carries. [`Self::read_slot`] says which a member is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mat_slot: Option<Label>,
    /// `[attr=value]` selector immediately after the keyword
    /// (`door[id=front] ...`). Carried through verbatim; later passes decide
    /// whether the selector is binding a fresh id or referencing an existing
    /// member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<IndexMap<String, ValueWithSpan>>,
    /// Bare values on the line, in source order.
    ///
    /// Not a prefix; see [`crate::ast::Statement::Generic`]'s field of the same name. That matters
    /// here because `check::positional` reads this copy and spans the run first to last. Non-empty
    /// only for special forms such as `connect <ref> to <ref>`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub positional: Vec<Value>,
    /// `-> VALUE` tail at the end of a line, e.g.
    /// `pressure_plate ... -> sig.step`. Always carried separately from
    /// [`Self::intent_state`] so the resolver's reference pass can match
    /// it without round-tripping through a string key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<Value>,
    /// `key=value` attributes the fields above did not consume. Untyped: no per-keyword schema pass
    /// exists.
    pub intent_state: IntentState,
    /// Per-edition resolved blockstate: always `None` at `Grouped`, filled when the lift pass
    /// advances the member to `Lifted`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_state: Option<ResolvedState>,
    /// Nested body indented under this member.
    ///
    /// A `level y=...`, or any other grouping keyword, may hold members, `logic` bindings and
    /// `assert` properties. A [`MemberBody`], the triple the top-level IR uses, keeps each
    /// addressable instead of forcing a caller to reject or drop the non-member ones. Lowering is
    /// total, so the only invariant is declaration order within each kind.
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
/// Not `#[non_exhaustive]` on purpose: [`MemberRole::Other`] already makes every external `match`
/// handle unknown keywords, and the marker would only suppress the in-crate exhaustiveness check
/// that catches a missed update when a keyword joins the roster.
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
    /// Any keyword the role table does not know. `check::keyword_allowlist` reports it as
    /// `E_UNKNOWN_KEYWORD`; lowering never fails.
    Other(String),
}

impl Member {
    /// The slot name this member reads from a theme: its hoisted [`Self::mat_slot`], unless its
    /// role's [`MemberRole::unread_arguments`] lists `mat_slot`, where the name changes nothing in
    /// the build.
    ///
    /// The one definition of reading a `mat_slot=`: every question of that shape asks this rather
    /// than [`Self::mat_slot`]. The resolver looks up exactly these names, and reports
    /// `E_THEME_VARIANT_MISSING` for the module's theme only when a `struct` or `def` member reads
    /// one. `block_array` reports `W_NO_THEME_BOUND` on a body only when a member surviving its
    /// `level` flattening reads one. A hoisted `mat_slot=` this answers `None` for is the
    /// `W_IGNORED_ARGUMENT` that `check::arguments` reports from the same table.
    ///
    /// An unknown keyword ([`MemberRole::Other`]) lists nothing there, so its slot reads: the line
    /// is refused with `E_UNKNOWN_KEYWORD` either way, and a misspelt slot on it is one the author
    /// will need once the keyword is repaired.
    #[must_use]
    pub fn read_slot(&self) -> Option<&str> {
        self.mat_slot
            .as_ref()
            .filter(|_| self.role.unread_argument("mat_slot").is_none())
            .map(|slot| slot.name.as_str())
    }

    /// Read `key=` as a non-negative `u32`, or `None` when the argument is absent, is not an
    /// integer, is negative, or would not fit.
    ///
    /// Shared because `block_array` places a `level`'s children at this offset and `check::nesting`
    /// reports a `level` that has none, so the two must agree on a usable value. Overflow is `None`
    /// so that no member lands at `u32::MAX` because the author asked for `2^33`.
    #[must_use]
    pub fn nonneg_u32(&self, key: &str) -> Option<u32> {
        match &self.intent_state.get(key)?.value.kind {
            ValueKind::Int(v) if *v >= 0 => u32::try_from(*v).ok(),
            _ => None,
        }
    }

    /// Read `key=` as a bare identifier, or `None` when the argument is absent or its value is of
    /// any other shape.
    ///
    /// Shared for the same reason as [`Self::nonneg_u32`]: a `kind=` picks which lowering rule
    /// runs, and `check::arguments` decides from the same value which of that rule's arguments it
    /// reads. A quoted `kind="shed"` names no rule in either place: the member does not lower, and
    /// the check pass leaves the line to the deferral.
    #[must_use]
    pub fn ident_value(&self, key: &str) -> Option<&str> {
        match &self.intent_state.get(key)?.value.kind {
            ValueKind::Ident(name) => Some(name.as_str()),
            _ => None,
        }
    }
}

/// Which top-level body a member sits in, and so which roles the later passes have a reader for.
///
/// The two bodies are read by different code and share no keyword. `block_array`'s phase buckets
/// voxelise a `struct` / `def` body; the resolver's placement loop walks a `site` body, matching
/// only [`MemberRole::Place`] and [`MemberRole::Connect`] and dropping everything else silently.
/// Naming the split once keeps `check::member_scope` (a member the body has no reader for) and
/// `check::nesting` (an indented body nothing unwraps) from encoding it twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// A `struct` or `def` body — one building's geometry.
    Geometry,
    /// A `site` body — a flat list of `place` and `connect` rows.
    Site,
}

impl BodyKind {
    /// Surface keyword(s) naming this body, for prose that has to say
    /// where the author is.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Geometry => "`struct` or `def`",
            Self::Site => "`site`",
        }
    }

    /// Keywords this body has a reader for, in the order
    /// [`super::known_keywords`] declares them.
    ///
    /// Rendered as the "expected one of" note, so it is the closed set the
    /// author picks a replacement from.
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
    /// Whether a body of this kind has anything that reads this role.
    ///
    /// Derived from the readers, since the parser accepts every keyword in every body:
    ///
    /// - Geometry: `block_array`'s `member_disposition` buckets `floor`, `walls`, `roof`, `stair`,
    ///   `door`, `window` and `pressure_plate`; `recognize_circuit_region` reads `circuit`;
    ///   `flatten_members` unwraps `level`.
    /// - Site: `resolve::resolve_site_placements` and `block_array::lower_site` match `place` and
    ///   `connect`.
    ///
    /// [`MemberRole::Other`] answers `false` for both, but no pass should report it on that basis:
    /// `check::keyword_allowlist` owns it, and the repair is the word rather than the body.
    ///
    /// Matched on the pair with no wildcard, so a new [`BodyKind`] must be answered for every role
    /// rather than reading as "no reader", which would empty [`BodyKind::allowed_keywords`] and
    /// report every member of that body.
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

    /// Surface keyword this role was classified from.
    ///
    /// The inverse of [`super::role_of`], so a diagnostic can quote the
    /// word the author typed without carrying it separately on every
    /// [`Member`]. `keyword_table`'s round-trip test keeps the two
    /// sides from drifting.
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
///
/// `check::connect_arity` and the resolver check both ends identically and differ only in naming
/// the one at fault, so they share this rather than passing `"from"` / `"to"` around. An end is a
/// slot with no argument key, and naming it in the `use=` / `east_of=` style would offer advice
/// that does not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectEnd {
    /// The `FROM.PORT` slot, before the `to` keyword.
    From,
    /// The `TO.PORT` slot, after the `to` keyword.
    To,
}

impl ConnectEnd {
    /// Bare name of the end, for prose that supplies its own framing
    /// (`the {} endpoint`).
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::From => "from",
            Self::To => "to",
        }
    }
}

/// Sub-body indented under a [`Member`] (typically a `level y=...` block).
///
/// Mirrors the three statement flavours [`super::StructIr`] groups at the
/// top level: ordinary member commands, `logic` bindings, and `assert`
/// properties. Defined as a struct rather than a `Vec` of one big sum so a
/// `level` block whose body mixes all three flavours lowers without losing
/// any of them (`spec/lint` "Error vs warning" forbids silent dropping).
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
    /// `true` when it holds no members, logic bindings or asserts, so serialization can elide it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty() && self.logic.is_empty() && self.asserts.is_empty()
    }
}

/// Raw `key=value` attributes attached to a [`Member`].
///
/// An [`IndexMap`] so source order is preserved, which diagnostics and snapshots depend on. Each
/// entry is a [`ValueWithSpan`] so `check` passes point at the byte range without re-walking the
/// AST. `Deref` / `DerefMut` to the map spare callers a `.fields` accessor.
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

/// A [`Value`] together with its source byte range.
///
/// The `Value` already owns a skipped `span`; this wrapper keeps [`IntentState`] and the IR's
/// `args` maps `IndexMap<String, ValueWithSpan>` after lowering copies the value out of the AST, so
/// `check` passes point at the exact range without re-correlating with the AST.
///
/// Equality includes the wrapper's `span`, unlike [`crate::ast::Value`]'s, which is its kind's. The
/// two agree for anything `lower` builds, since the wrapper copies the value's own span (see
/// [`Self::span`]), but they answer different questions.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ValueWithSpan {
    /// Underlying value.
    pub value: Value,
    /// Byte range of the value in the original source. Equal to `self.value.span()` for everything
    /// `lower` produces; a separate field so a `ValueWithSpan` can cover the surrounding
    /// `key=value` range without rebuilding the `Value`.
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

/// A label-shaped value, an identifier or a string, hoisted out of a member's arguments into a
/// dedicated field, with where it was written.
///
/// Hoisting takes the key out of [`IntentState`], whose entries carry their own spans, so the
/// position travels with the name rather than in a field beside it. `#[serde(transparent)]` over
/// the name, with the span skipped as in [`ValueWithSpan`], so the IR serialises a bare string.
/// [`ValueWithSpan`] itself does not fit: it serialises the value's kind, and a hoisted label is
/// already reduced to its text.
///
/// Equality includes the span, as [`Member`]'s does.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Label {
    /// The text of the label, without quotes for a string.
    pub name: String,
    /// Byte range of the value in the original source.
    #[serde(skip)]
    pub span: Span,
}

/// Resolved, per-edition blockstate for a [`Member`].
///
/// Always empty: the shape is fixed so a `Lifted` pass can fill it without a breaking change.
/// Derefs to its map as [`IntentState`] does.
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
