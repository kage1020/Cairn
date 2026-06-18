//! Intent IR — the first semantic layer above the surface AST.
//!
//! The Intent IR keeps the surface form's structure (a module owns themes,
//! defs, sites, and structs) but reorganises each body into typed groups:
//! [`Member`]s carry roles and an [`IntentState`] of `key=value` attributes,
//! `logic` and `assert` lines are split out of the generic statement stream,
//! and the size header on a struct is hoisted into a dedicated field.
//!
//! This is what the spec calls the "rich member with invariants" layer
//! (`architecture.md` §3.2). M2 produces it at semantic level
//! [`SemanticLevel::Grouped`] — registry-backed resolution (materials, themes,
//! per-edition blockstate) is M3's job.
//!
//! Source position information is not propagated yet; the AST does not carry
//! per-node spans, and rather than back-fill them here a follow-up PR (M2-PR2
//! diagnostic) will add positions to both layers in one coordinated change.

mod keyword_table;
mod lower;
mod member;
mod semantic_level;

use std::num::NonZeroU32;

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{DottedRef, Expr, Header, TruthRow, Value};

pub use self::keyword_table::role_of;
pub use self::lower::lower;
pub use self::member::{IntentState, Member, MemberRole, ResolvedState};
pub use self::semantic_level::SemanticLevel;

/// Intent IR for a whole `.crn` module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntentModule {
    /// Maturity of this IR. M2's [`lower`] always returns
    /// [`SemanticLevel::Grouped`].
    pub semantic_level: SemanticLevel,
    /// Headers carried through verbatim from the AST.
    pub headers: Vec<Header>,
    /// `theme` items, in source order.
    pub themes: Vec<ThemeIr>,
    /// `def` items, in source order.
    pub defs: Vec<DefIr>,
    /// `site` items, in source order.
    pub sites: Vec<SiteIr>,
    /// `struct` items, in source order.
    pub structs: Vec<StructIr>,
}

/// `theme NAME:` block, normalised so slot bindings live in a map and
/// selector bindings keep their full surface shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeIr {
    /// Theme name.
    pub name: String,
    /// `slot NAME -> VALUE` bindings. Source order is preserved; if a slot is
    /// declared twice the last wins here and the duplicate is flagged by the
    /// M2-PR2 `duplicate` pass.
    pub slots: IndexMap<String, Value>,
    /// `KEYWORD[...] -> ...` selector bindings, in source order.
    pub selectors: Vec<SelectorRule>,
}

/// Lifted form of one `ThemeRule::Selector` row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SelectorRule {
    /// Member keyword on the LHS (`window`, `door`, ...).
    pub keyword: String,
    /// `[attr=...]` selector attributes in source order.
    pub attrs: IndexMap<String, Value>,
    /// `key=value` bindings on the RHS of the arrow.
    pub bindings: IndexMap<String, Value>,
}

/// Lifted form of `def NAME[ ARGS] [:]` (reusable parameterised component).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DefIr {
    /// Definition name.
    pub name: String,
    /// Hoisted `size=WxH` header argument, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// Remaining header `key=value` arguments (excluding `size`).
    pub args: IndexMap<String, Value>,
    /// Member lines from the def body.
    pub members: Vec<Member>,
    /// `logic` bindings from the def body.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub logic: Vec<LogicBinding>,
    /// `assert` properties from the def body.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub asserts: Vec<AssertIr>,
}

/// Lifted form of `struct NAME[ ARGS]` (single-building structural
/// composition). Structurally identical to [`DefIr`] but kept as a distinct
/// type so downstream passes can match on intent.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StructIr {
    /// Struct name.
    pub name: String,
    /// Hoisted `size=WxH` header argument, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// Remaining header `key=value` arguments (excluding `size`).
    pub args: IndexMap<String, Value>,
    /// Member lines from the struct body.
    pub members: Vec<Member>,
    /// `logic` bindings from the struct body.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub logic: Vec<LogicBinding>,
    /// `assert` properties from the struct body.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub asserts: Vec<AssertIr>,
}

/// Lifted form of `site NAME[:]` (multi-building placement).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteIr {
    /// Site name.
    pub name: String,
    /// `place` / `connect` lines from the site body, kept as
    /// [`Member`]s so role-based passes treat them uniformly with struct
    /// members.
    pub placements: Vec<Member>,
    /// `logic` bindings from the site body. Rare, but legal at the site
    /// scope so we don't drop them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub logic: Vec<LogicBinding>,
    /// `assert` properties from the site body.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub asserts: Vec<AssertIr>,
}

/// `width × height` footprint hoisted out of a struct/def header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Size {
    /// Width in blocks.
    pub w: NonZeroU32,
    /// Height in blocks.
    pub h: NonZeroU32,
}

/// `logic LHS = EXPR` line lifted out of a body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogicBinding {
    /// LHS reference being defined.
    pub lhs: DottedRef,
    /// RHS boolean expression.
    pub rhs: Expr,
}

/// `assert ...` line lifted out of a body.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
#[non_exhaustive]
pub enum AssertIr {
    /// `assert truth(INPUTS -> OUTPUT) { ROWS }`.
    Truth {
        /// Input signal references in declaration order.
        inputs: Vec<DottedRef>,
        /// Output signal reference.
        output: DottedRef,
        /// One `bits -> result` per row.
        rows: Vec<TruthRow>,
    },
    /// `assert always(ANTECEDENT -> eventually CONSEQUENT within N)`.
    Always {
        /// Antecedent reference.
        antecedent: DottedRef,
        /// Consequent reference.
        consequent: DottedRef,
        /// `within N` bound in ticks.
        within: u32,
    },
}
