//! Abstract syntax tree for the Cairn surface language.
//!
//! Every node here preserves the surface form of a single `.crn` construct so
//! that round-tripping, error reporting, and future semantic lifting can all
//! reason about what the author actually wrote. Disambiguation that would
//! collapse equivalent surface forms (canonicalising tokens, resolving names,
//! type-checking) is the responsibility of downstream layers.

use std::num::NonZeroU32;

use serde::Serialize;

/// A `CalVer` language version string captured verbatim from `@cairn`.
///
/// Wrapping the raw string in a newtype prevents callers from confusing it
/// with arbitrary identifiers, requirement expressions, or other free-form
/// labels. Validation of the `YYYY.0M[.PATCH]` shape is the responsibility of
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
    /// `@cairn 2026.06` — Cairn language version this file was written against.
    Cairn {
        /// `CalVer` string captured verbatim from the source.
        version: RawVersion,
    },
    /// `@requires version>=1.20` — Minecraft version capability floor.
    Requires {
        /// Requirement expression captured verbatim from the source.
        requirement: RawRequirement,
    },
    /// `@intended_targets ["1.20.4","1.21.4"]` — author hints about Minecraft target version.
    IntendedTargets {
        /// One element per target string in the original list literal.
        targets: Vec<String>,
    },
}

/// A top-level construct in a `.crn` file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum Item {
    /// `theme NAME[:]` block — slot/selector bindings.
    Theme {
        /// Theme name following the `theme` keyword.
        name: String,
        /// Body rules in source order.
        body: Vec<ThemeRule>,
    },
    /// `def NAME[ ARGS][:]` block — reusable parameterised component.
    Def {
        /// Definition name following the `def` keyword.
        name: String,
        /// Inline `key=value` arguments on the definition header.
        args: Vec<Arg>,
        /// Indented body statements in source order.
        body: Vec<Statement>,
    },
    /// `site NAME[:]` block — multi-building placement.
    Site {
        /// Site name following the `site` keyword.
        name: String,
        /// Indented body statements in source order.
        body: Vec<Statement>,
    },
    /// `struct NAME[ ARGS]` block — single building / structural composition.
    Struct {
        /// Struct name following the `struct` keyword.
        name: String,
        /// Inline `key=value` arguments on the struct header.
        args: Vec<Arg>,
        /// Indented body statements in source order.
        body: Vec<Statement>,
    },
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
    },
    /// `KEYWORD[ATTRS] -> KEY=VALUE [KEY=VALUE...]` selector binding.
    Selector {
        /// Member keyword on the LHS (`window`, `door`, ...).
        keyword: String,
        /// Selector attributes inside the brackets.
        attrs: Vec<Arg>,
        /// `key=value` bindings on the RHS of the arrow.
        bindings: Vec<Arg>,
    },
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
        /// Bare positional values consumed before any `key=value` argument.
        /// Empty for the overwhelming majority of commands; non-empty only for
        /// special forms such as `connect <ref> to <ref>`.
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
    },
    /// `logic LHS = EXPR` line — binds a boolean signal.
    Logic {
        /// LHS reference being defined.
        lhs: DottedRef,
        /// RHS boolean expression.
        rhs: Expr,
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
    },
}

/// A `key=value` pair as it appears in an argument list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Arg {
    /// Argument name on the LHS of `=`.
    pub key: String,
    /// Value on the RHS of `=`.
    pub value: Value,
}

/// A value occurring on the RHS of `key=`, inside a list, or as a positional
/// argument.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value")]
#[non_exhaustive]
pub enum Value {
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

/// One row of an `assert truth(...)` table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TruthRow {
    /// Input bit pattern, e.g. `01` (preserved with its leading zeros).
    pub inputs: String,
    /// Output bit. The truth-table grammar permits only `0` or `1`, so the
    /// AST stores the value as a plain `bool` rather than a `u8` that could
    /// also represent illegal values like `7`.
    pub output: bool,
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
