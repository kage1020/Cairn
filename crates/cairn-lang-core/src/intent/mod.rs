//! Intent IR — the first semantic layer above the surface AST.
//!
//! The Intent IR keeps the surface form's structure (a module owns themes,
//! defs, sites, and structs) but reorganises each body into typed groups:
//! [`Member`]s carry roles and an [`IntentState`] of `key=value` attributes,
//! `logic` and `assert` lines are split out of the generic statement stream,
//! and the size header on a struct is hoisted into a dedicated field.
//!
//! This is what the spec calls the "rich member with invariants" layer
//! (`spec/architecture` "The Intent IR is rich and carries invariants"). The
//! current lowering produces it at semantic level [`SemanticLevel::Grouped`];
//! registry-backed resolution (materials, themes, per-edition blockstate)
//! belongs to the later [`SemanticLevel::Lifted`] tier.
//!
//! Each IR node carries a `span: Span` pointing at the originating byte range
//! in the source. The `check` module relies on those spans to emit gcc-style
//! diagnostics; spans are tagged `#[serde(skip)]` so the on-the-wire form is
//! unchanged from the pre-span IR.

mod keyword_table;
mod lower;
mod member;
mod patch;
mod semantic_level;

use std::num::NonZeroU32;

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{DottedRef, Expr, Header, TruthRow, ValueKind};
use crate::error::Span;

pub use self::keyword_table::{
    SENSOR_HOSTS, SelectorArm, SelectorAxis, SelectorValue, UNIVERSAL_ARGUMENTS, known_keywords,
    role_of,
};
pub use self::lower::lower;
pub(crate) use self::member::ConnectEnd;
pub use self::member::{
    BodyKind, IntentState, Member, MemberBody, MemberRole, ResolvedState, ValueWithSpan,
};
pub use self::patch::{PatchTargetError, actuator_patch_target};
pub use self::semantic_level::SemanticLevel;

/// Intent IR for a whole `.crn` module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntentModule {
    /// Maturity of this IR. The current [`lower()`] always returns
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

/// Lifted form of `def NAME[ ARGS] [:]` (reusable component).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DefIr {
    /// Definition name.
    pub name: String,
    /// Hoisted `size=WxH` header argument, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// Remaining header `key=value` arguments: every one but a `size=`
    /// whose value is a `WxH` literal. A `size=` of any other shape stays
    /// here, and `check::type_mismatch` reports it.
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
    /// Remaining header `key=value` arguments: every one but a `size=`
    /// whose value is a `WxH` literal. A `size=` of any other shape stays
    /// here, and `check::type_mismatch` reports it.
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
/// the redstone placement pass (`spec/redstone` "Place-and-route") has one
/// entry point for looking up the reserved area of each scope instead of
/// walking [`Member`]s and re-decoding `intent_state` at every caller.
/// The block-array pass's [`crate::block_array`] recogniser owns the
/// shape validation and per-shape diagnostics; this lift function
/// leaves out any malformed or size-less fixture, and
/// [`rejected_circuit_regions`] hands those to a caller that needs to
/// say why a scope has no reservation.
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
/// no `size` for the routing pass to budget against. A circuit member
/// that is not a usable reservation is left out here and reported by
/// [`rejected_circuit_regions`] instead, with the reason, so a pass
/// that finds a scope with no reservation can point at the line that
/// was meant to be one. (`cairn check` still reports each malformed
/// shape individually via the block-array pass's
/// `recognize_circuit_region`; this function is called from paths that
/// skip the block-array lower, so it cannot rely on that pass firing.)
#[must_use]
pub fn circuit_regions(module: &IntentModule) -> Vec<CircuitRegion> {
    circuit_members(module)
        .into_iter()
        .filter_map(Result::ok)
        .collect()
}

/// Every `circuit` member [`circuit_regions`] leaves out, in source
/// order within each scope, with why it is not a usable reservation.
#[must_use]
pub fn rejected_circuit_regions(module: &IntentModule) -> Vec<RejectedCircuitRegion> {
    circuit_members(module)
        .into_iter()
        .filter_map(Result::err)
        .collect()
}

/// A `circuit` member that reserves nothing, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct RejectedCircuitRegion {
    /// Which scope family the circuit member was declared under.
    pub scope_kind: ScopeKind,
    /// Source-level name of the scope.
    pub scope_name: String,
    /// What is wrong with the line.
    pub defect: CircuitRegionDefect,
    /// Byte range of the originating `circuit ...` line.
    #[serde(skip)]
    pub span: Span,
}

/// Why a `circuit` member is not a usable reservation. The first that
/// applies, in the order listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CircuitRegionDefect {
    /// The enclosing scope has no `size=WxH` header to reserve within.
    NoSize,
    /// No `region=` key.
    RegionMissing,
    /// `region=` holds a value that is not an identifier or a string;
    /// `found` is the kind of value it is.
    RegionNotLabel {
        /// The value kind, as [`crate::ast::Value::kind_name`] names it.
        found: &'static str,
    },
    /// `region=` is an empty string.
    RegionEmpty,
    /// No `void=` key.
    VoidMissing,
    /// `void=` holds a value that is not an integer; `found` is the
    /// kind of value it is.
    VoidNotInteger {
        /// The value kind, as [`crate::ast::Value::kind_name`] names it.
        found: &'static str,
    },
    /// `void=` is an integer below 1, which reserves no service layer.
    VoidBelowOne {
        /// The integer written.
        value: i64,
    },
    /// `void=` is an integer too large for a `u32`.
    VoidTooLarge {
        /// The integer written.
        value: i64,
    },
}

/// Every `circuit` member of every struct and def, read into a
/// reservation or the reason it is not one.
fn circuit_members(module: &IntentModule) -> Vec<Result<CircuitRegion, RejectedCircuitRegion>> {
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
    out: &mut Vec<Result<CircuitRegion, RejectedCircuitRegion>>,
) {
    for m in members {
        if !matches!(m.role, MemberRole::Circuit) {
            continue;
        }
        let read = size
            .ok_or(CircuitRegionDefect::NoSize)
            .and_then(|size| parse_circuit_region_fixture(m).map(|fixture| (size, fixture)));
        out.push(match read {
            Ok((size, (label, void))) => Ok(CircuitRegion {
                scope_kind,
                scope_name: scope_name.to_owned(),
                label,
                void,
                width: size.w.get(),
                depth: size.h.get(),
                span: m.span.clone(),
            }),
            Err(defect) => Err(RejectedCircuitRegion {
                scope_kind,
                scope_name: scope_name.to_owned(),
                defect,
                span: m.span.clone(),
            }),
        });
    }
}

/// Parse the `region=<label>` / `void=<N>` payload of a `circuit`
/// [`Member`] into `(label, void)` when both sides are well-formed, or
/// the first defect when not, so callers cannot silently accept a
/// partial fixture.
fn parse_circuit_region_fixture(member: &Member) -> Result<(String, u32), CircuitRegionDefect> {
    let raw_region = member
        .intent_state
        .get("region")
        .ok_or(CircuitRegionDefect::RegionMissing)?;
    let label = raw_region
        .value
        .as_label_str()
        .ok_or(CircuitRegionDefect::RegionNotLabel {
            found: raw_region.value.kind_name(),
        })?;
    if label.is_empty() {
        return Err(CircuitRegionDefect::RegionEmpty);
    }
    let raw_void = member
        .intent_state
        .get("void")
        .ok_or(CircuitRegionDefect::VoidMissing)?;
    let void = match &raw_void.value.kind {
        ValueKind::Int(value) if *value < 1 => {
            return Err(CircuitRegionDefect::VoidBelowOne { value: *value });
        }
        ValueKind::Int(value) => u32::try_from(*value)
            .map_err(|_| CircuitRegionDefect::VoidTooLarge { value: *value })?,
        _ => {
            return Err(CircuitRegionDefect::VoidNotInteger {
                found: raw_void.value.kind_name(),
            });
        }
    };
    Ok((label.to_owned(), void))
}
