//! Intent IR — the first semantic layer above the surface AST.
//!
//! The Intent IR keeps the surface form's structure (a module owns themes,
//! defs, sites, and structs) but reorganises each body into typed groups:
//! [`Member`]s carry roles and an [`IntentState`] of `key=value` attributes,
//! `logic` and `assert` lines are split out of the generic statement stream,
//! and the size header on a struct is hoisted into a dedicated field.
//!
//! This is what the spec calls the "rich member with invariants" layer
//! (`architecture.md` §3.2). The current lowering produces it at semantic
//! level [`SemanticLevel::Grouped`]; registry-backed resolution
//! (materials, themes, per-edition blockstate) belongs to the later
//! [`SemanticLevel::Lifted`] tier.
//!
//! Each IR node carries a `span: Span` pointing at the originating byte range
//! in the source. The `check` module relies on those spans to emit gcc-style
//! diagnostics; spans are tagged `#[serde(skip)]` so the on-the-wire form is
//! unchanged from the pre-span IR.

mod keyword_table;
mod lower;
mod member;
mod semantic_level;

use std::num::NonZeroU32;

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{DottedRef, Expr, Header, TruthRow, ValueKind};
use crate::error::Span;

pub use self::keyword_table::{known_keywords, role_of};
pub use self::lower::lower;
pub(crate) use self::member::ConnectEnd;
pub use self::member::{
    BodyKind, IntentState, Member, MemberBody, MemberRole, ResolvedState, ValueWithSpan,
};
pub use self::semantic_level::SemanticLevel;

/// Intent IR for a whole `.crn` module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntentModule {
    /// Maturity of this IR. The current [`lower`] always returns
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
    /// `duplicate` pass in `crate::check`.
    pub slots: IndexMap<String, ValueWithSpan>,
    /// `KEYWORD[...] -> ...` selector bindings, in source order.
    pub selectors: Vec<SelectorRule>,
    /// Byte range of the originating `theme NAME ...` block.
    #[serde(skip)]
    pub span: Span,
}

/// Lifted form of one `ThemeRule::Selector` row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SelectorRule {
    /// Member keyword on the LHS (`window`, `door`, ...).
    pub keyword: String,
    /// `[attr=...]` selector attributes in source order.
    pub attrs: IndexMap<String, ValueWithSpan>,
    /// `key=value` bindings on the RHS of the arrow.
    pub bindings: IndexMap<String, ValueWithSpan>,
    /// Byte range of the originating selector rule line.
    #[serde(skip)]
    pub span: Span,
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
    pub args: IndexMap<String, ValueWithSpan>,
    /// Member lines from the def body.
    pub members: Vec<Member>,
    /// `logic` bindings from the def body.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub logic: Vec<LogicBinding>,
    /// `assert` properties from the def body.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub asserts: Vec<AssertIr>,
    /// Byte range of the originating `def NAME ...` block.
    #[serde(skip)]
    pub span: Span,
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
    pub args: IndexMap<String, ValueWithSpan>,
    /// Member lines from the struct body.
    pub members: Vec<Member>,
    /// `logic` bindings from the struct body.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub logic: Vec<LogicBinding>,
    /// `assert` properties from the struct body.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub asserts: Vec<AssertIr>,
    /// Byte range of the originating `struct NAME ...` block.
    #[serde(skip)]
    pub span: Span,
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
    /// Byte range of the originating `site NAME ...` block.
    #[serde(skip)]
    pub span: Span,
}

/// `width × height` footprint hoisted out of a struct/def header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Size {
    /// Width in blocks.
    pub w: NonZeroU32,
    /// Height in blocks.
    pub h: NonZeroU32,
    /// Byte range of the originating `WxH` literal in source.
    #[serde(skip)]
    pub span: Span,
}

/// `logic LHS = EXPR` line lifted out of a body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogicBinding {
    /// LHS reference being defined.
    pub lhs: DottedRef,
    /// RHS boolean expression.
    pub rhs: Expr,
    /// Byte range of the originating `logic ...` line in source.
    #[serde(skip)]
    pub span: Span,
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
        /// Byte range of the originating `assert truth(...) { ... }`.
        #[serde(skip)]
        span: Span,
    },
    /// `assert always(ANTECEDENT -> eventually CONSEQUENT within N)`.
    Always {
        /// Antecedent reference.
        antecedent: DottedRef,
        /// Consequent reference.
        consequent: DottedRef,
        /// `within N` bound in ticks.
        within: u32,
        /// Byte range of the originating `assert always(...)`.
        #[serde(skip)]
        span: Span,
    },
}

impl AssertIr {
    /// Byte range of the originating `assert ...` line in source.
    #[must_use]
    pub fn span(&self) -> &Span {
        match self {
            Self::Truth { span, .. } | Self::Always { span, .. } => span,
        }
    }

    /// Every signal this property names, in declaration order.
    ///
    /// Lives here rather than at the consumer because the enum is
    /// `#[non_exhaustive]`: a match written in another crate needs a
    /// wildcard, and a wildcard is where a variant added later would go to
    /// have its references silently unchecked. Written here the match is
    /// exhaustive, so the next variant stops the compile until it says
    /// which of its fields are signals.
    #[must_use]
    pub fn signal_refs(&self) -> Vec<&DottedRef> {
        match self {
            Self::Truth { inputs, output, .. } => {
                inputs.iter().chain(std::iter::once(output)).collect()
            }
            Self::Always {
                antecedent,
                consequent,
                ..
            } => vec![antecedent, consequent],
        }
    }
}

/// Which family of Intent IR scope a downstream pass is describing.
///
/// Introduced so passes that hand data off across crates (redstone
/// placement, future routing) can key on scope identity without
/// depending on the surface AST or on a downstream crate's local
/// discriminator. The three variants intentionally mirror the shape of
/// [`IntentModule::structs`] / [`IntentModule::defs`] / [`IntentModule::sites`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    /// A `struct NAME` body.
    Struct,
    /// A `def NAME[ ARGS]` body.
    Def,
    /// A `site NAME` body.
    Site,
}

/// One recognised `circuit region=<label> void=<N>` fixture together
/// with the footprint of its enclosing scope.
///
/// Produced by [`circuit_regions`] out of a lowered [`IntentModule`] so
/// the redstone placement pass (`spec/redstone` §14.5) has one entry
/// point for looking up the reserved area of each scope instead of
/// walking [`Member`]s and re-decoding `intent_state` at every caller.
/// The block-array pass's [`crate::block_array`] recogniser owns the
/// shape validation and per-shape diagnostics; this lift function
/// silently filters out any malformed or size-less fixture so the two
/// sides cannot both fire diagnostics for the same source line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct CircuitRegion {
    /// Which scope family the circuit member was declared under.
    pub scope_kind: ScopeKind,
    /// Source-level name of the scope (`struct gatehouse` → `"gatehouse"`).
    pub scope_name: String,
    /// `region=<label>` value the circuit member declared.
    pub label: String,
    /// `void=<N>` service-layer height (`>= 1`).
    pub void: u32,
    /// Width of the enclosing scope's footprint, copied from `size=WxH`.
    pub width: u32,
    /// Depth of the enclosing scope's footprint, copied from `size=WxH`.
    pub depth: u32,
    /// Byte range of the originating `circuit region=...` line.
    #[serde(skip)]
    pub span: Span,
}

/// Lift every well-formed `circuit region=<label> void=<N>` fixture out
/// of `module`, tagged with the enclosing scope's footprint.
///
/// Walks `module.structs` / `module.defs` at the top level (children
/// under `level` blocks are not descended into for v1 — a circuit
/// member nested inside a `level` stays a follow-up for the routing
/// pass that actually needs it). Sites are skipped because they carry
/// no `size` for the routing pass to budget against. Any circuit member
/// whose `region=` value is missing, non-label, or empty, or whose
/// `void=` is missing, non-integer, or zero, is dropped silently —
/// downstream passes see the same "no reservation for this scope" state
/// they would on a truly missing line and are expected to surface a
/// diagnostic that names the malformed-fixture case alongside the
/// missing-line one. (`cairn check` still reports each malformed shape
/// individually via the block-array pass's `recognize_circuit_region`;
/// this function is called from paths that skip the block-array lower,
/// so it cannot rely on that pass firing.)
#[must_use]
pub fn circuit_regions(module: &IntentModule) -> Vec<CircuitRegion> {
    let mut out = Vec::new();
    for s in &module.structs {
        collect_circuit_regions(
            ScopeKind::Struct,
            &s.name,
            s.size.as_ref(),
            &s.members,
            &mut out,
        );
    }
    for d in &module.defs {
        collect_circuit_regions(
            ScopeKind::Def,
            &d.name,
            d.size.as_ref(),
            &d.members,
            &mut out,
        );
    }
    out
}

fn collect_circuit_regions(
    scope_kind: ScopeKind,
    scope_name: &str,
    size: Option<&Size>,
    members: &[Member],
    out: &mut Vec<CircuitRegion>,
) {
    let Some(size) = size else {
        return;
    };
    for m in members {
        if !matches!(m.role, MemberRole::Circuit) {
            continue;
        }
        let Some((label, void)) = parse_circuit_region_fixture(m) else {
            continue;
        };
        out.push(CircuitRegion {
            scope_kind,
            scope_name: scope_name.to_owned(),
            label,
            void,
            width: size.w.get(),
            depth: size.h.get(),
            span: m.span.clone(),
        });
    }
}

/// Parse the `region=<label>` / `void=<N>` payload of a `circuit`
/// [`Member`] into `(label, void)` when both sides are well-formed.
/// Returns `None` for any missing or malformed key so callers cannot
/// silently accept a partial fixture. Callers that surface a
/// diagnostic on `None` should mention every rejection cause the
/// block-array pass's `recognize_circuit_region` distinguishes
/// (`region=` absent / non-label / empty; `void=` absent / non-integer
/// / zero) because this function is called from paths that skip
/// `cairn check` and cannot rely on the per-shape `W_DEFERRED_MEMBER`
/// stream to disambiguate.
fn parse_circuit_region_fixture(member: &Member) -> Option<(String, u32)> {
    let raw_region = member.intent_state.get("region")?;
    let label = raw_region.value.as_label_str()?;
    if label.is_empty() {
        return None;
    }
    let raw_void = member.intent_state.get("void")?;
    let void = match &raw_void.value.kind {
        ValueKind::Int(v) if *v >= 1 => u32::try_from(*v).ok()?,
        _ => return None,
    };
    Some((label.to_owned(), void))
}
