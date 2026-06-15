//! Abstract syntax tree for the Cairn surface language.
//!
//! M1 carries every example through the parser unchanged: the AST is shaped to
//! preserve the surface form of each command, not to disambiguate semantics.
//! Semantic lifting (Intent IR) is the responsibility of M2.

use serde::Serialize;

/// A whole `.crn` source file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Module {
    /// Leading `@cairn` / `@requires` / `@intended_targets` directives.
    pub headers: Vec<Header>,
    /// Top-level items in source order.
    pub items: Vec<Item>,
}

/// A leading directive line.
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
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum Item {
    /// `theme NAME[:]` block — slot/selector bindings.
    Theme {
        /// Theme name.
        name: String,
        /// Body rules in source order.
        body: Vec<ThemeRule>,
    },
    /// `def NAME[ ARGS][:]` block — reusable parameterised component.
    Def {
        /// Definition name.
        name: String,
        /// Inline `key=value` arguments on the definition header.
        args: Vec<Arg>,
        /// Indented body commands in source order.
        body: Vec<Command>,
    },
    /// `site NAME[:]` block — multi-building placement.
    Site {
        /// Site name.
        name: String,
        /// Indented body commands in source order.
        body: Vec<Command>,
    },
    /// `struct NAME[ ARGS]` block — single building / structural composition.
    Struct {
        /// Struct name.
        name: String,
        /// Inline `key=value` arguments on the struct header.
        args: Vec<Arg>,
        /// Indented body commands in source order.
        body: Vec<Command>,
    },
}

/// A line inside a `theme` block.
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

/// A `key=value` pair.
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
    Size {
        /// Width component.
        w: u32,
        /// Height component.
        h: u32,
    },
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

/// Structured extension carried by special-form commands.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum Extra {
    /// `logic LHS = EXPR` line.
    LogicEq {
        /// LHS reference being defined.
        lhs: Vec<String>,
        /// RHS boolean expression.
        rhs: Expr,
    },
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
    AssertAlways {
        /// Antecedent reference.
        antecedent: Vec<String>,
        /// Consequent reference.
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
