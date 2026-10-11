//! Abstract syntax tree for the Cairn surface language.
//!
//! Every node keeps the surface form of one `.crn` construct, so error reporting and later layers
//! see what the author wrote. Anything that would merge equivalent spellings (canonicalising
//! tokens, resolving names, type-checking) belongs to a later layer.
//!
//! [`Header`], [`Item`], [`Statement`], [`ThemeRule`], [`MemberRequires`], [`Arg`], [`Value`]
//! and [`TruthRow`] carry the byte `span` of their source, and [`Item`] also a narrower
//! `name_span`. Every span field is `#[serde(skip)]`, and a new one must be too, so the
//! serialized shape carries no positions. [`Expr`] and [`DottedRef`] carry none, because no
//! finding points inside a `logic` or `assert` expression.

use std::num::NonZeroU32;

use serde::Serialize;

use crate::error::Span;

/// A `CalVer` language version string captured verbatim from `@cairn`.
///
/// A newtype so it is not confused with other strings; the `YYYY.M[.PATCH]` shape is not checked
/// here. The parsed [`crate::calver::LanguageVersion`] is deliberately not held beside it: the
/// header is provenance, so consumers want the author's string, and a parsed copy would be a
/// second answer to keep in step. `#[non_exhaustive]` keeps external callers on
/// [`RawVersion::new`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct RawVersion(pub String);

impl RawVersion {
    /// Wrap a raw source string as a [`RawVersion`].
    pub fn new(version: impl Into<String>) -> Self {
        Self(version.into())
    }

    /// Borrow the underlying raw string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A Minecraft requirement expression captured verbatim from `@requires`.
///
/// A newtype and `#[non_exhaustive]` for the same reasons as [`RawVersion`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct RawRequirement(pub String);

impl RawRequirement {
    /// Wrap a raw source string as a [`RawRequirement`].
    pub fn new(requirement: impl Into<String>) -> Self {
        Self(requirement.into())
    }

    /// Borrow the underlying raw string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One `requires version>=X` line inside a `def` or `theme` body.
///
/// The member-level twin of [`Header::Requires`] (`spec/versioning-editions` "Fail-loud and
/// minimum-version inference"): the same verbatim expression, read by the same
/// `parse_requirement`, but a floor on the part that declares it, and so on every build that
/// instantiates the part.
///
/// Held on the item rather than among its body statements because `requires` is not a member
/// keyword: as a statement it would lower to `MemberRole::Other` and be reported as
/// `E_UNKNOWN_KEYWORD`.
///
/// The expression is kept unshaped, as `@requires` keeps it, so that `E_INVALID_REQUIRES` can say
/// which part is wrong; a shape check here would turn that into a bare `E_PARSE`. The parser
/// refuses only an empty expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct MemberRequires {
    /// Requirement expression captured verbatim from the source, without
    /// the leading `requires` keyword. Known to be non-empty; not known to
    /// be a version floor.
    pub requirement: RawRequirement,
    /// Byte range of the whole `requires ...` line.
    #[serde(skip)]
    pub span: Span,
}

impl MemberRequires {
    /// Wrap a raw requirement expression and the line it was written on.
    #[must_use]
    pub fn new(requirement: RawRequirement, span: Span) -> Self {
        Self { requirement, span }
    }
}

/// A whole `.crn` source file: a sequence of leading directives followed by
/// top-level items, both kept in source order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Module {
    /// Leading `@cairn` / `@requires` / `@intended_targets` directives.
    pub headers: Vec<Header>,
    /// Top-level items in source order.
    pub items: Vec<Item>,
}

/// A leading directive line introduced by `@`.
///
/// Values are captured verbatim; parsing them is a later layer's job.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum Header {
    /// `@cairn 2026.7` — Cairn language version this file was written against.
    Cairn {
        /// `CalVer` string captured verbatim from the source.
        version: RawVersion,
        /// Byte range of the whole directive in the original source.
        #[serde(skip)]
        span: Span,
    },
    /// `@requires version>=1.20` — Minecraft version capability floor.
    Requires {
        /// Requirement expression captured verbatim from the source.
        requirement: RawRequirement,
        /// Byte range of the whole directive in the original source.
        #[serde(skip)]
        span: Span,
    },
    /// `@intended_targets ["1.20.4","1.21.4"]` — author hints about Minecraft target version.
    IntendedTargets {
        /// One element per target string in the original list literal.
        targets: Vec<String>,
        /// Byte range of the whole directive in the original source.
        #[serde(skip)]
        span: Span,
    },
}

impl Header {
    /// Byte range covered by this header in the original source.
    #[must_use]
    pub fn span(&self) -> &Span {
        match self {
            Self::Cairn { span, .. }
            | Self::Requires { span, .. }
            | Self::IntendedTargets { span, .. } => span,
        }
    }
}

/// A top-level construct in a `.crn` file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum Item {
    /// `theme NAME[:]` block — slot/selector bindings.
    ///
    /// `#[non_exhaustive]` on the variant as well as the enum, because the enum's attribute does
    /// not cover a field added to an existing variant.
    #[non_exhaustive]
    Theme {
        /// Theme name following the `theme` keyword.
        name: String,
        /// Byte range of the name token alone.
        #[serde(skip)]
        name_span: Span,
        /// `requires version>=X` lines the body declares, in source order.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        requires: Vec<MemberRequires>,
        /// Body rules in source order.
        body: Vec<ThemeRule>,
        /// Byte range covering the whole `theme ...` block including its
        /// indented body.
        #[serde(skip)]
        span: Span,
    },
    /// `def NAME[ ARGS][:]` block — reusable component.
    ///
    /// `#[non_exhaustive]`, for the reason given on [`Self::Theme`].
    #[non_exhaustive]
    Def {
        /// Definition name following the `def` keyword.
        name: String,
        /// Byte range of the name token alone.
        #[serde(skip)]
        name_span: Span,
        /// Inline `key=value` arguments on the definition header.
        args: Vec<Arg>,
        /// `requires version>=X` lines the body declares, in source order.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        requires: Vec<MemberRequires>,
        /// Indented body statements in source order.
        body: Vec<Statement>,
        /// Byte range covering the whole `def ...` block including its
        /// indented body.
        #[serde(skip)]
        span: Span,
    },
    /// `site NAME[:]` block — multi-building placement.
    Site {
        /// Site name following the `site` keyword.
        name: String,
        /// Byte range of the name token alone.
        #[serde(skip)]
        name_span: Span,
        /// Indented body statements in source order.
        body: Vec<Statement>,
        /// Byte range covering the whole `site ...` block including its
        /// indented body.
        #[serde(skip)]
        span: Span,
    },
    /// `struct NAME[ ARGS]` block — single building / structural composition.
    Struct {
        /// Struct name following the `struct` keyword.
        name: String,
        /// Byte range of the name token alone.
        #[serde(skip)]
        name_span: Span,
        /// Inline `key=value` arguments on the struct header.
        args: Vec<Arg>,
        /// Indented body statements in source order.
        body: Vec<Statement>,
        /// Byte range covering the whole `struct ...` block including its
        /// indented body.
        #[serde(skip)]
        span: Span,
    },
}

/// Which top-level construct an [`Item`] is.
///
/// The kind is also the name's namespace: one name may be declared once per kind, because the
/// resolver keys scopes `struct::NAME`, `def::NAME` and `site::NAME::PLACE_ID` and holds themes in
/// a map of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    /// `theme NAME` block.
    Theme,
    /// `def NAME` block.
    Def,
    /// `site NAME` block.
    Site,
    /// `struct NAME` block.
    Struct,
}

impl ItemKind {
    /// Every kind, in the order `keyword` lists them.
    pub const ALL: [Self; 4] = [Self::Theme, Self::Def, Self::Site, Self::Struct];

    /// Keyword introducing this kind, as written in source, and the scope-key prefix of every
    /// kind but `theme`.
    #[must_use]
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Def => "def",
            Self::Site => "site",
            Self::Struct => "struct",
        }
    }
}

impl Item {
    /// Byte range covered by this item in the original source.
    #[must_use]
    pub fn span(&self) -> &Span {
        match self {
            Self::Theme { span, .. }
            | Self::Def { span, .. }
            | Self::Site { span, .. }
            | Self::Struct { span, .. } => span,
        }
    }

    /// Which top-level construct this is.
    #[must_use]
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Theme { .. } => ItemKind::Theme,
            Self::Def { .. } => ItemKind::Def,
            Self::Site { .. } => ItemKind::Site,
            Self::Struct { .. } => ItemKind::Struct,
        }
    }

    /// Name this item declares.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Theme { name, .. }
            | Self::Def { name, .. }
            | Self::Site { name, .. }
            | Self::Struct { name, .. } => name,
        }
    }

    /// Byte range of the name token alone, for a finding about the name; [`Self::span`] runs to
    /// the end of the body.
    #[must_use]
    pub fn name_span(&self) -> &Span {
        match self {
            Self::Theme { name_span, .. }
            | Self::Def { name_span, .. }
            | Self::Site { name_span, .. }
            | Self::Struct { name_span, .. } => name_span,
        }
    }

    /// The `requires version>=X` lines this item declares.
    ///
    /// Empty for `site` and `struct`: `spec/versioning-editions` "Fail-loud and minimum-version
    /// inference" gives the line only to `def` and `theme`, the kinds a build instantiates rather
    /// than is. A slice rather than an `Option`, so a caller folding floors over every item asks
    /// one question.
    #[must_use]
    pub fn requires(&self) -> &[MemberRequires] {
        match self {
            Self::Theme { requires, .. } | Self::Def { requires, .. } => requires,
            Self::Site { .. } | Self::Struct { .. } => &[],
        }
    }
}

/// A line inside a `theme` block: either a slot binding or a selector binding.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum ThemeRule {
    /// `slot NAME -> VALUE` mapping.
    Slot {
        /// Slot name on the LHS.
        slot: String,
        /// Token or value bound to the slot.
        value: Value,
        /// Byte range of the entire `slot NAME -> VALUE` line.
        #[serde(skip)]
        span: Span,
    },
    /// `KEYWORD[ATTRS] -> KEY=VALUE [KEY=VALUE...]` selector binding.
    Selector {
        /// Member keyword on the LHS (`window`, `door`, ...).
        keyword: String,
        /// Selector attributes inside the brackets.
        attrs: Vec<Arg>,
        /// `key=value` bindings on the RHS of the arrow.
        bindings: Vec<Arg>,
        /// Byte range of the entire selector rule line.
        #[serde(skip)]
        span: Span,
    },
}

impl ThemeRule {
    /// Byte range covered by this rule in the original source.
    #[must_use]
    pub fn span(&self) -> &Span {
        match self {
            Self::Slot { span, .. } | Self::Selector { span, .. } => span,
        }
    }
}

/// One line of a struct / site / def body.
///
/// Most lines are `Generic`: `keyword[selector] positional... key=value... -> binding`. `logic`
/// and `assert` have grammars of their own, and so variants of their own.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum Statement {
    /// Standard command line with optional selector, positional values,
    /// `key=value` arguments, an optional `-> binding`, and indented children.
    Generic {
        /// Leading command keyword (`floor`, `walls`, `door`, ...).
        keyword: String,
        /// Optional bracketed selector immediately after the keyword.
        #[serde(skip_serializing_if = "Option::is_none")]
        selector: Option<Vec<Arg>>,
        /// Bare values on the line, in source order.
        ///
        /// Not only a prefix: a bare value after an argument lands here too (`size=2 x2`),
        /// because it is usually a dropped `=` that `check::positional` reports. Empty for every
        /// well-formed command but `connect <ref> to <ref>` (`spec/syntax` "Lexical").
        #[serde(skip_serializing_if = "Vec::is_empty")]
        positional: Vec<Value>,
        /// `key=value` arguments in source order.
        args: Vec<Arg>,
        /// Optional `-> VALUE` binding at the end of the line, e.g.
        /// `pressure_plate ... -> sig.step`.
        #[serde(skip_serializing_if = "Option::is_none")]
        binding: Option<Value>,
        /// Child statements indented under this one.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        children: Vec<Statement>,
        /// Byte range of the keyword line itself, without its children.
        #[serde(skip)]
        span: Span,
    },
    /// `logic LHS = EXPR` line — binds a boolean signal.
    Logic {
        /// LHS reference being defined.
        lhs: DottedRef,
        /// RHS boolean expression.
        rhs: Expr,
        /// Byte range of the whole `logic ...` line.
        #[serde(skip)]
        span: Span,
    },
    /// `assert truth(INPUTS -> OUTPUT) { ROWS }` line — declarative truth
    /// table over input signals.
    AssertTruth {
        /// Input signal references inside `truth(...)`.
        inputs: Vec<DottedRef>,
        /// Output signal reference after `->`.
        output: DottedRef,
        /// One `bits -> result` per row.
        rows: Vec<TruthRow>,
        /// Byte range of the whole `assert truth(...) { ... }` form.
        #[serde(skip)]
        span: Span,
    },
    /// `assert always(ANTECEDENT -> eventually CONSEQUENT within N)` line —
    /// liveness property bounded by `within N` ticks.
    AssertAlways {
        /// Antecedent reference on the LHS of `->`.
        antecedent: DottedRef,
        /// Consequent reference after `eventually`.
        consequent: DottedRef,
        /// `within N` bound in ticks.
        within: u32,
        /// Byte range of the whole `assert always(...)` form.
        #[serde(skip)]
        span: Span,
    },
}

impl Statement {
    /// Byte range covered by this statement in the original source. For
    /// `Generic`, this is the keyword line only — child statements have their
    /// own spans.
    #[must_use]
    pub fn span(&self) -> &Span {
        match self {
            Self::Generic { span, .. }
            | Self::Logic { span, .. }
            | Self::AssertTruth { span, .. }
            | Self::AssertAlways { span, .. } => span,
        }
    }
}

/// A `key=value` pair as it appears in an argument list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Arg {
    /// Argument name on the LHS of `=`.
    pub key: String,
    /// Value on the RHS of `=`.
    pub value: Value,
    /// Byte range of the whole `key=value` pair.
    #[serde(skip)]
    pub span: Span,
}

/// Discriminant of a [`Value`], in its surface form.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value")]
#[non_exhaustive]
pub enum ValueKind {
    /// Bare identifier (`outer`, `center`, `gable`, ...).
    Ident(String),
    /// Boolean literal.
    Bool(bool),
    /// Integer literal (`4`, `0`, ...).
    Int(i64),
    /// Size literal `WxH` (`9x7`), in blocks. Non-zero, since a zero extent is no footprint.
    Size {
        /// Width in blocks.
        w: NonZeroU32,
        /// Height in blocks.
        h: NonZeroU32,
    },
    /// Canonical or abstract token, stored *without* the leading `@` sigil
    /// (source `@oak_planks` → `Token("oak_planks")`,
    /// `@floor.wood.broadleaf` → `Token("floor.wood.broadleaf")`).
    Token(String),
    /// Dotted reference (`home1.entry`, `sig.step`, `inside.front`).
    DotRef(DottedRef),
    /// Double-quoted string literal.
    Str(String),
    /// List literal of values.
    List(Vec<Value>),
}

/// A value on the right of `key=`, inside a list, or as a positional argument, with its byte
/// range. Serializes as its [`ValueKind`] alone.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Value {
    /// What kind of value this is.
    pub kind: ValueKind,
    /// Byte range covered by this value in the original source. Not part of equality.
    #[serde(skip)]
    pub span: Span,
}

/// Two values are equal when they say the same thing, wherever they were
/// written.
///
/// Hand-written so `span` is ignored at every depth of a [`ValueKind::List`]: a theme selector and
/// the duplicate-selector pass ask what a value is, not where it was written.
///
/// Two traits stay absent:
///
/// - `Hash`: a derived hash would include `span` and break `a == b ⇒ hash(a) == hash(b)`.
///   Clippy's `derived_hash_with_manual_eq` catches a derive but not a hand-written impl.
/// - `Eq`: it would hold, but `ValueKind` is `#[non_exhaustive]` and the spec already shows a
///   decimal literal (`scale=2.0`). A float variant would then mean withdrawing `Eq`, a major
///   break, and nothing needs it.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Value {
    /// Build a [`Value`] from a kind and span.
    #[must_use]
    pub fn new(kind: ValueKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// Byte range covered by this value in the original source.
    #[must_use]
    pub fn span(&self) -> &Span {
        &self.span
    }

    /// The inner string when this value is a label: an identifier or a string literal, as in
    /// `use=cottage` or `theme=medieval`. The one place that decides what counts as a label.
    #[must_use]
    pub fn as_label_str(&self) -> Option<&str> {
        match &self.kind {
            ValueKind::Ident(s) | ValueKind::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Source text that would parse back to this value, or `None` for a list, whose text is
    /// unbounded and so cannot be rendered inline.
    ///
    /// A string is quoted and otherwise left alone: string literals have no escapes, so `Debug`'s
    /// escaping would show text the author did not write.
    fn surface_form(&self) -> Option<String> {
        match &self.kind {
            ValueKind::Ident(s) => Some(s.clone()),
            ValueKind::Str(s) => Some(format!("\"{s}\"")),
            ValueKind::Bool(b) => Some(b.to_string()),
            ValueKind::Int(i) => Some(i.to_string()),
            ValueKind::Size { w, h } => Some(format!("{w}x{h}")),
            ValueKind::Token(t) => Some(format!("@{t}")),
            ValueKind::DotRef(d) => Some(d.to_string()),
            ValueKind::List(_) => None,
        }
    }

    /// Kind and surface form together, for a message that says what it found:
    /// `` identifier `a` ``, `` reference `foo.bar` ``, `a list`.
    ///
    /// A noun phrase, article included where there is no surface form, so it drops into a
    /// sentence whichever arm it took.
    #[must_use]
    pub fn describe(&self) -> String {
        match self.surface_form() {
            Some(text) => format!("{} `{text}`", self.kind_name()),
            None => format!("a {}", self.kind_name()),
        }
    }

    /// One-word name of the kind, for diagnostic messages.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match &self.kind {
            ValueKind::Ident(..) => "identifier",
            ValueKind::Bool(..) => "boolean",
            ValueKind::Int(..) => "integer",
            ValueKind::Size { .. } => "size",
            ValueKind::Token(..) => "token",
            ValueKind::DotRef(..) => "reference",
            ValueKind::Str(..) => "string",
            ValueKind::List(..) => "list",
        }
    }
}

/// One row of an `assert truth(...)` table.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct TruthRow {
    /// Input pattern, one character per input signal: `0`, `1`, or `-` for a don't-care, which
    /// stands for both values. Leading zeros are kept, since `01` and `1` are rows of differently
    /// sized tables.
    pub inputs: String,
    /// Output bit, or `None` where the row's output is `-`: the combinations it covers count as
    /// assigned for coverage, but nothing is asserted about the output there.
    pub output: Option<bool>,
    /// Byte range of `PATTERN -> BIT`, without the separator that follows, which is punctuation
    /// between rows.
    #[serde(skip)]
    pub span: Span,
}

/// A boolean expression used inside `logic` lines.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value")]
#[non_exhaustive]
pub enum Expr {
    /// Signal reference.
    Ref(DottedRef),
    /// `a and b`.
    And(Box<Expr>, Box<Expr>),
    /// `a or b`.
    Or(Box<Expr>, Box<Expr>),
    /// `not a`.
    Not(Box<Expr>),
}

/// Head segment marking a [`DottedRef`] as a redstone signal name rather than a member id or a
/// place reference.
///
/// Kept beside the type it classifies, because `block_array`, `check` and the redstone crate all
/// ask the question.
pub const SIGNAL_HEAD: &str = "sig";

/// A non-empty dotted name path such as `home1.entry`, `sig.step`, or a bare
/// `outer` (single segment).
///
/// Never empty: [`DottedRef::new`] takes the head segment apart from the tail. Serializes as the
/// bare segment list, `["sig", "step"]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct DottedRef {
    segments: Vec<String>,
}

impl DottedRef {
    /// Build a [`DottedRef`] from a `head` segment and a possibly empty `tail`.
    #[must_use]
    pub fn new(head: String, tail: Vec<String>) -> Self {
        let mut segments = Vec::with_capacity(tail.len() + 1);
        segments.push(head);
        segments.extend(tail);
        Self { segments }
    }

    /// Build a [`DottedRef`] from a segment vector, or `None` when it is empty.
    #[must_use]
    pub fn try_from_segments(segments: Vec<String>) -> Option<Self> {
        if segments.is_empty() {
            None
        } else {
            Some(Self { segments })
        }
    }

    /// Borrow the full segment list in source order.
    #[must_use]
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// First segment — always present.
    #[must_use]
    pub fn head(&self) -> &str {
        &self.segments[0]
    }

    /// All segments after the head; may be empty for a single-segment path.
    #[must_use]
    pub fn tail(&self) -> &[String] {
        &self.segments[1..]
    }

    /// Number of segments, never zero.
    ///
    /// # Panics
    /// Never: every constructor keeps the path non-empty.
    #[must_use]
    pub fn len(&self) -> std::num::NonZeroUsize {
        std::num::NonZeroUsize::new(self.segments.len()).expect("DottedRef is non-empty")
    }

    /// Iterate over the segments in source order.
    pub fn iter(&self) -> std::slice::Iter<'_, String> {
        self.segments.iter()
    }
}

impl std::fmt::Display for DottedRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for segment in &self.segments {
            if !first {
                f.write_str(".")?;
            }
            f.write_str(segment)?;
            first = false;
        }
        Ok(())
    }
}

impl<'a> IntoIterator for &'a DottedRef {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.segments.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value of `kind` spanning one byte at `offset`.
    fn at(offset: usize, kind: ValueKind) -> Value {
        Value::new(kind, offset..offset + 1)
    }

    #[test]
    fn a_value_is_equal_to_the_same_value_written_somewhere_else() {
        // A derived `PartialEq` would compare spans at every depth of a list.
        let ident = |s: &str| ValueKind::Ident(s.to_owned());
        let flat =
            |offset| ValueKind::List(vec![at(offset, ident("a")), at(offset + 2, ident("b"))]);
        let nested = |offset| ValueKind::List(vec![at(offset, flat(offset + 1))]);

        assert_eq!(at(0, ident("a")), at(90, ident("a")), "scalar");
        assert_eq!(at(0, flat(0)), at(90, flat(90)), "list");
        assert_eq!(at(0, nested(0)), at(90, nested(90)), "list of lists");

        // Still unequal when the contents differ, at any depth.
        assert_ne!(at(0, ident("a")), at(0, ident("b")), "different idents");
        assert_ne!(
            at(0, ValueKind::List(vec![at(0, ident("a"))])),
            at(0, ValueKind::List(vec![at(0, ident("b"))])),
            "different list contents",
        );
        assert_ne!(
            at(0, nested(0)),
            at(
                0,
                ValueKind::List(vec![at(
                    1,
                    ValueKind::List(vec![at(1, ident("a")), at(3, ident("z"))])
                )])
            ),
            "different contents one level down",
        );
    }
}
