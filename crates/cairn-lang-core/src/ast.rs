//! Abstract syntax tree for the Cairn surface language.
//!
//! Every node here preserves the surface form of a single `.crn` construct so
//! that round-tripping, error reporting, and future semantic lifting can all
//! reason about what the author actually wrote. Disambiguation that would
//! collapse equivalent surface forms (canonicalising tokens, resolving names,
//! type-checking) is the responsibility of downstream layers.
//!
//! Source-position propagation: [`Header`], [`Item`], [`Statement`],
//! [`ThemeRule`], [`MemberRequires`], [`Arg`], [`Value`], and [`TruthRow`] each
//! carry a `span` field
//! pointing at the byte range of the originating source, and [`Item`] carries a
//! second, narrower `name_span` for the name token alone — a block's `span`
//! reaches to the end of its indented body, which is too much to underline for a
//! finding about the name. Diagnostic-collecting passes in
//! `crate::check` rely on those spans to point a user at a specific spot in
//! their `.crn` file. Every span field is tagged `#[serde(skip)]`, including
//! any added later, so the externally-visible wire shape is unchanged from
//! before spans were threaded through. The rest of the boolean-expression
//! family ([`Expr`] and [`DottedRef`]) is intentionally span-free for now;
//! those nodes only appear inside `logic`/`assert` lines that the
//! reference-resolution pass will revisit, and spending the bytes there today
//! would not buy any diagnostic the current passes need. [`TruthRow`] left
//! that group once `crate::check` began reporting one row against another:
//! "this row disagrees with an earlier one" names two positions, and neither
//! of them is the statement's.

use std::num::NonZeroU32;

use serde::Serialize;

use crate::error::Span;

/// A `CalVer` language version string captured verbatim from `@cairn`.
///
/// Wrapping the raw string in a newtype prevents callers from confusing it
/// with arbitrary identifiers, requirement expressions, or other free-form
/// labels. Validation of the `YYYY.M[.PATCH]` shape is the responsibility of
/// the semantic layer; this type only fixes the source provenance.
///
/// The semantic layer is expected to introduce a distinct `Version` type that
/// wraps a *parsed* `CalVer`, leaving `RawVersion` to mean "verbatim from
/// source" only. `#[non_exhaustive]` keeps room to add validated constructors
/// (e.g. `RawVersion::from_validated`) without a breaking change, and forces
/// external callers to go through [`RawVersion::new`] rather than depending
/// on the tuple-struct shape.
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

/// A Minecraft-side requirement expression captured verbatim from
/// `@requires`. Same role as [`RawVersion`]: keep the raw form distinct from
/// other strings so downstream layers know it is meant to be parsed as a
/// constraint, not displayed as-is.
///
/// `#[non_exhaustive]` plays the same role as on [`RawVersion`] — see that
/// type's documentation for the rationale.
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
/// The member-level twin of [`Header::Requires`] (`spec/versioning-editions.md`
/// §10.4). It carries the same verbatim expression, read by the same
/// `parse_requirement`, and differs only in what it constrains: a module-level
/// `@requires` is a floor on the file, while this one is a floor on the part
/// that declares it and therefore on every build that instantiates that part.
///
/// Held on the item rather than in its body statements because the word is
/// not a member keyword and must not become one. `crate::intent`'s role
/// table has no arm for it, so a `requires` line left among the statements
/// lowers to `MemberRole::Other("requires")` and `check::keyword_allowlist`
/// reports every one of them as `E_UNKNOWN_KEYWORD`. Lifting it is what
/// keeps the floor out of the member vocabulary; the alternative was not a
/// line the geometry passes skip but a new role for four passes to learn.
///
/// What it holds is one line's expression, verbatim — the same slice
/// `@requires` keeps, and shaped no further. `E_INVALID_REQUIRES` is what
/// reads it, and it says which part of the expression is wrong and carries
/// the fragment for a quick-fix; a shape gate here would replace that with
/// `E_PARSE` and no fragment, and would hold the two spellings of one
/// constraint to two different grammars. The one thing the parser does
/// refuse is an empty expression, because there is nothing there for either
/// to report on.
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
/// Values inside each variant are captured verbatim from the source: parsing
/// the version string, requirement expression, or target list into structured
/// data happens in a later layer so this AST stays free of validation policy.
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
    /// `#[non_exhaustive]` on the variant, not only on the enum: the enum's
    /// own attribute makes adding a *variant* non-breaking and does nothing
    /// for a field added to an existing one, which is what the `requires`
    /// field was. This variant and [`Self::Def`] carry it because this
    /// change already broke their struct patterns, so the next field costs
    /// nothing; [`Self::Site`] and [`Self::Struct`] do not, because giving
    /// it to them would be a breakage this change does not need.
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
    /// `def NAME[ ARGS][:]` block — reusable parameterised component.
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
/// The kind is also the name's namespace. The resolver keys scopes
/// `struct::NAME`, `def::NAME`, and `site::NAME::PLACE_ID`, and holds
/// themes in a map of their own, so one name may be declared once per
/// kind without colliding. `check::duplicate` and the resolver both
/// depend on that fact; a `&'static str` in each would be two tables to
/// keep in step, and nothing would catch a typo in either.
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
    /// Keyword introducing this kind, as written in source.
    ///
    /// Doubles as the scope-key prefix for the three kinds that have
    /// one (`struct::`, `def::`, `site::`); themes are held by name in
    /// a map of their own and so have no key.
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

    /// Byte range of the name token alone.
    ///
    /// [`Self::span`] covers the whole block including its indented
    /// body, which is the wrong thing to underline when the finding is
    /// about the name.
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
    /// Empty for `site` and `struct`, which refuse a line that reads as a
    /// floor — `spec/versioning-editions.md` §10.4 gives it to `def` and
    /// `theme`, the two kinds a build instantiates rather than *is*. An
    /// empty slice rather than an `Option`, because a caller folding
    /// floors over every item asks the same question of all four and the
    /// answer for the other two is "none", not "not applicable".
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
/// Most lines are `Generic` and follow the standard
/// `keyword[selector] positional... key=value... -> binding` grammar. The
/// special forms `logic` and `assert` have their own surface grammar and
/// therefore appear as dedicated variants — making the discriminant a type
/// invariant rather than an `Option<Extra>` carried by every generic line.
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
        /// Not a prefix: the parse loop appends here whenever the next token
        /// is not `key=`, `-> binding`, or a selector, so `size=2 x2` leaves
        /// one entry *after* an argument. `check::positional` relies on that
        /// — an interleaved bare value is usually a dropped `=`, which is the
        /// shape worth catching.
        ///
        /// Empty for every well-formed command but `connect <ref> to <ref>`,
        /// the one form with a reader for it (`spec/syntax.md` §5.1).
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
        /// Byte range of the keyword line itself (excluding children). Used by
        /// the `keyword_allowlist` and `type_mismatch` passes to point at a
        /// whole statement.
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
    /// Byte range covering the whole `key=value` pair. Used by the
    /// `duplicate` pass to point at a repeated argument and by
    /// `keyword_allowlist` notes that fold an argument span into the
    /// surrounding diagnostic.
    #[serde(skip)]
    pub span: Span,
}

/// Discriminant of a [`Value`].
///
/// Keeps the original surface-form variants. The wrapper [`Value`] then
/// attaches a `span` next to the kind, so internally-tagged enum
/// serialisation (`{kind, value}`) stays byte-identical to the pre-span form.
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
    /// Size literal `WxH` (`9x7`). Both dimensions are blocks, and `0xN`
    /// or `Nx0` is not meaningful as a building footprint — the type rules
    /// it out without per-field validation downstream.
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

/// A value occurring on the RHS of `key=`, inside a list, or as a positional
/// argument.
///
/// Carries the underlying [`ValueKind`] together with the byte range of the
/// originating literal in source. The wrapper is `#[serde(transparent)]` over
/// the kind so the wire shape is identical to serialising the bare
/// `ValueKind` — `span` does not appear in JSON/YAML output.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Value {
    /// What kind of value this is.
    pub kind: ValueKind,
    /// Byte range covered by this value in the original source.
    ///
    /// Not part of equality — see the `PartialEq` impl below.
    #[serde(skip)]
    pub span: Span,
}

/// Two values are equal when they say the same thing, wherever they were
/// written.
///
/// Hand-written rather than derived. The derive compared `span` as well,
/// and [`ValueKind::List`] holds `Value`s — so `ValueKind`'s own derived
/// equality recursed back through this impl, and two lists spelled
/// identically on two lines were never equal at any depth. The comparisons
/// this exists for ask what the value *is*: a theme selector asking
/// whether a member carries the attribute it names, and the
/// duplicate-selector pass asking whether two rows select alike. The
/// `#[serde(transparent)]` above already says the value is its kind, and a
/// caller that needs the position has `span` in hand.
///
/// Two traits are deliberately absent, and stay absent:
///
/// - `Hash` must not be derived here. This equality ignores `span`, so a
///   derived hash would break `k1 == k2 ⇒ hash(k1) == hash(k2)`. Clippy's
///   `derived_hash_with_manual_eq` catches the derive; a hand-written
///   `impl Hash` would slip past it.
/// - `Eq` would hold today — nothing in [`ValueKind`] is a float, so the
///   relation is a true equivalence — but `ValueKind` is
///   `#[non_exhaustive]` and the spec already shows a decimal literal
///   (`scale=2.0`). A later `Float(f64)` would mean withdrawing `Eq`, and
///   removing a trait impl is a major break where adding one is not.
///   Nothing needs it: no map is keyed on a `Value`, and `assert_eq!`
///   wants only `PartialEq`.
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

    /// Borrow the inner string when this value lowers as a *label* — an
    /// identifier or a string literal. Used by passes that consume
    /// `key=label` arguments (`use=cottage`, `theme=medieval`,
    /// `east_of=home1`, ...) so the same coercion does not have to be
    /// re-implemented at every call site, and so a future relaxation of
    /// what counts as a label (e.g. accepting bare tokens) lands in one
    /// place. Returns `None` for non-label kinds; callers raise a
    /// targeted diagnostic in that case.
    #[must_use]
    pub fn as_label_str(&self) -> Option<&str> {
        match &self.kind {
            ValueKind::Ident(s) | ValueKind::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Source text that would parse back to this value, or `None` when
    /// the reconstruction is unbounded — a list nests, and a diagnostic
    /// that grows with the input is not one an editor can render inline.
    ///
    /// A string is quoted and otherwise left alone. Cairn string literals
    /// have no escape mechanism — the lexer takes the raw slice between
    /// the quotes, and a newline inside one is an unterminated-string
    /// error — so `Debug`'s escaping would show the author text they did
    /// not write and that would not parse back to the same value.
    /// Private, because the round-trip claim is a contract and `describe`
    /// is the one caller that has to keep it.
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

    /// Kind and surface form together, for a message that says what it
    /// found: `` identifier `a` ``, `` reference `foo.bar` ``, `a list`.
    ///
    /// The surface form matters most for the shapes that render
    /// identically under a bare [`Self::kind_name`]: a message that
    /// printed a string's contents unquoted read as rejecting the very
    /// word it had asked for.
    ///
    /// A noun phrase, article included, so it drops into a sentence
    /// without the caller knowing which arm it took: `names a list`
    /// rather than `names list`. Only the unbounded kinds reach the
    /// article — the rest carry their own text and read as a phrase
    /// already.
    #[must_use]
    pub fn describe(&self) -> String {
        match self.surface_form() {
            Some(text) => format!("{} `{text}`", self.kind_name()),
            None => format!("a {}", self.kind_name()),
        }
    }

    /// One-word rendering of the kind. Used in diagnostic messages such as
    /// "expected a label (identifier or string), got `token`" so callers do
    /// not have to match on the kind themselves.
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
    /// Input bit pattern, e.g. `01` (preserved with its leading zeros).
    pub inputs: String,
    /// Output bit. The truth-table grammar permits only `0` or `1`, so the
    /// AST stores the value as a plain `bool` rather than a `u8` that could
    /// also represent illegal values like `7`.
    pub output: bool,
    /// Byte range of `PATTERN -> BIT`, ending at the output bit: the
    /// separator that follows is punctuation between rows rather than part
    /// of one, and underlining it would put the caret past the text the
    /// finding is about.
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

/// Head segment marking a [`DottedRef`] as a redstone signal name rather
/// than a member id or a place reference.
///
/// Lives here, next to the type it classifies, because three layers ask the
/// same question: `block_array` decides whether an actuator argument is
/// wired, `check::member_scope` decides whether a misplaced member is
/// carrying a signal worth mentioning, and `redstone::synth` collects the
/// sensors and actuators themselves.
pub const SIGNAL_HEAD: &str = "sig";

/// A non-empty dotted name path such as `home1.entry`, `sig.step`, or a bare
/// `outer` (single segment).
///
/// Non-emptiness is encoded in the [`DottedRef::new`] constructor signature
/// — a head segment is mandatory, the tail may be empty — so downstream code
/// never has to special-case an empty path. Serialises as the bare segment
/// list so wire consumers see `["sig", "step"]`, not a wrapped object.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct DottedRef {
    segments: Vec<String>,
}

impl DottedRef {
    /// Build a non-empty [`DottedRef`] from a mandatory `head` segment and a
    /// (possibly empty) `tail` of additional segments. The two-arg signature
    /// makes the non-empty invariant a structural property of the call rather
    /// than something the caller has to remember to satisfy.
    #[must_use]
    pub fn new(head: String, tail: Vec<String>) -> Self {
        let mut segments = Vec::with_capacity(tail.len() + 1);
        segments.push(head);
        segments.extend(tail);
        Self { segments }
    }

    /// Try to build a [`DottedRef`] from an arbitrary segment vector,
    /// returning `None` if the vector is empty. Use this when the caller
    /// already has a `Vec<String>` and would prefer to push the emptiness
    /// check to the type rather than the call site.
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

    /// Number of segments. Always ≥ 1, hence the `NonZeroUsize` return —
    /// callers never need a separate "is empty" check.
    ///
    /// # Panics
    /// Never: the non-empty invariant is enforced by every constructor, so
    /// the inner `NonZeroUsize::new` call always succeeds.
    #[must_use]
    pub fn len(&self) -> std::num::NonZeroUsize {
        std::num::NonZeroUsize::new(self.segments.len()).expect("DottedRef is non-empty")
    }

    /// Iterate over the segments in source order. Equivalent to
    /// `(&dr).into_iter()` but discoverable as a method.
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

    /// A value of `kind` at `offset`, spanning one byte. The offset is
    /// what varies between two otherwise identical values.
    fn at(offset: usize, kind: ValueKind) -> Value {
        Value::new(kind, offset..offset + 1)
    }

    #[test]
    fn a_value_is_equal_to_the_same_value_written_somewhere_else() {
        // The guard against re-deriving `PartialEq`. The derive compared
        // `span`, and a list holds `Value`s, so the recursion made two
        // identically-spelled lists unequal at every depth — which is how
        // a list-valued theme selector came to match no member at all.
        let ident = |s: &str| ValueKind::Ident(s.to_owned());
        let flat =
            |offset| ValueKind::List(vec![at(offset, ident("a")), at(offset + 2, ident("b"))]);
        let nested = |offset| ValueKind::List(vec![at(offset, flat(offset + 1))]);

        assert_eq!(at(0, ident("a")), at(90, ident("a")), "scalar");
        assert_eq!(at(0, flat(0)), at(90, flat(90)), "list");
        assert_eq!(at(0, nested(0)), at(90, nested(90)), "list of lists");

        // And still discriminating on what the value says, to the same
        // depth: a difference buried inside a nested list is a difference.
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
