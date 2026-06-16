//! Abstract syntax tree for the Cairn surface language.
//!
//! Every node here preserves the surface form of a single `.crn` construct so
//! that round-tripping, error reporting, and future semantic lifting can all
//! reason about what the author actually wrote. Disambiguation that would
//! collapse equivalent surface forms (canonicalising tokens, resolving names,
//! type-checking) is the responsibility of downstream layers.

use serde::Serialize;

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
        version: String,
    },
    /// `@requires version>=1.20` — Minecraft version capability floor.
    Requires {
        /// Requirement expression captured verbatim from the source.
        requirement: String,
    },
    /// `@intended_targets ["1.20.4","1.21.4"]` — author hints about Minecraft target version.
    IntendedTargets {
        /// One element per target string in the original list literal.
        targets: Vec<String>,
    },
}

/// A top-level construct in a `.crn` file.
///
/// Each variant stores its identifier name and, where applicable, an `args`
/// list and a `body`. Bodies are always kept in source order so that semantic
/// passes can reason about declaration order without re-walking the source.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum Item {
    /// `theme NAME[:]` block — slot/selector bindings.
    #[allow(missing_docs)]
    Theme { name: String, body: Vec<ThemeRule> },
    /// `def NAME[ ARGS][:]` block — reusable parameterised component.
    #[allow(missing_docs)]
    Def {
        name: String,
        args: Vec<Arg>,
        body: Vec<Command>,
    },
    /// `site NAME[:]` block — multi-building placement.
    #[allow(missing_docs)]
    Site { name: String, body: Vec<Command> },
    /// `struct NAME[ ARGS]` block — single building / structural composition.
    #[allow(missing_docs)]
    Struct {
        name: String,
        args: Vec<Arg>,
        body: Vec<Command>,
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

/// A generic indented command line — the workhorse AST node.
///
/// `selector`, `positional`, `binding`, and `extra` are all `Option`/empty in
/// the common case; they capture the optional grammar pieces that only certain
/// commands use (`connect <ref> to <ref>`, `pressure_plate ... -> sig.step`,
/// `logic`, `assert`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Command {
    /// Leading command keyword.
    pub keyword: String,
    /// Optional bracketed selector immediately after the keyword.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<Vec<Arg>>,
    /// Bare positional values consumed before any `key=value` argument.
    /// Empty for the overwhelming majority of commands; non-empty only for
    /// special forms such as `connect <ref> to <ref>`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub positional: Vec<Value>,
    /// `key=value` arguments in source order.
    pub args: Vec<Arg>,
    /// Optional `-> VALUE` binding at the end of the line, e.g.
    /// `pressure_plate ... -> sig.step`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<Value>,
    /// Optional structured extension for commands with custom grammar
    /// (`logic`, `assert`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Extra>,
    /// Child commands indented under this one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Command>,
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
    /// Size literal `WxH` (`9x7`).
    #[allow(missing_docs)]
    Size { w: u32, h: u32 },
    /// Canonical or abstract token, stored *without* the leading `@` sigil
    /// (source `@oak_planks` → `Token("oak_planks")`,
    /// `@floor.wood.broadleaf` → `Token("floor.wood.broadleaf")`).
    Token(String),
    /// Dotted reference (`home1.entry`, `sig.step`, `inside.front`).
    DotRef(Vec<String>),
    /// Double-quoted string literal.
    Str(String),
    /// List literal of values.
    List(Vec<Value>),
}

/// Structured extension carried by special-form commands (`logic`, `assert`).
///
/// These variants only appear in `Command::extra` for the corresponding
/// keyword; mismatch is rejected by the parser.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum Extra {
    /// `logic LHS = EXPR` line.
    #[allow(missing_docs)]
    LogicEq { lhs: Vec<String>, rhs: Expr },
    /// `assert truth(INPUTS -> OUTPUT) { ROWS }` line.
    AssertTruth {
        /// Input signal references inside `truth(...)`.
        inputs: Vec<Vec<String>>,
        /// Output signal reference after `->`.
        output: Vec<String>,
        /// One `bits -> result` per row.
        rows: Vec<TruthRow>,
    },
    /// `assert always(ANTECEDENT -> eventually CONSEQUENT within N)` line.
    ///
    /// `antecedent` and `consequent` map straight to the named references in
    /// the grammar above and need no further commentary; only `within`
    /// carries a non-obvious unit (ticks), which is why it is the one field
    /// that keeps an explicit doc line.
    #[allow(missing_docs)]
    AssertAlways {
        antecedent: Vec<String>,
        consequent: Vec<String>,
        /// `within N` bound in ticks.
        within: u32,
    },
}

/// One row of an `assert truth(...)` table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TruthRow {
    /// Input bit pattern, e.g. `01` (preserved with its leading zeros).
    pub inputs: String,
    /// Output bit, `0` or `1`.
    pub output: u8,
}

/// A boolean expression used inside `logic` lines.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value")]
#[non_exhaustive]
pub enum Expr {
    /// Signal reference.
    Ref(Vec<String>),
    /// `a and b`.
    And(Box<Expr>, Box<Expr>),
    /// `a or b`.
    Or(Box<Expr>, Box<Expr>),
    /// `not a`.
    Not(Box<Expr>),
}
