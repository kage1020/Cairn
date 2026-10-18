//! Intent IR, the first semantic layer above the surface AST: each body sorted into typed
//! [`Member`]s with roles, and `logic` and `assert` lines split out.

mod keyword_table;
mod lower;
mod member;
mod patch;
mod semantic_level;

use std::fmt;
use std::num::NonZeroU32;

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{DottedRef, Expr, Header, TruthRow, ValueKind};
use crate::error::Span;

pub use self::keyword_table::{
    MemberRole, SENSOR_HOSTS, SelectorArm, SelectorAxis, SelectorValue, UNIVERSAL_ARGUMENTS,
    Unread, UnreadArgument, known_keywords, role_of,
};
pub use self::lower::lower;
pub(crate) use self::member::ConnectEnd;
pub use self::member::{
    BodyKind, IntentState, Label, Member, MemberBody, ResolvedState, ValueWithSpan,
};
pub use self::patch::{PatchTargetError, actuator_patch_target};
pub use self::semantic_level::SemanticLevel;

/// Intent IR for a whole `.crn` module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntentModule {
    /// Maturity of this IR; [`lower()`] always produces [`SemanticLevel::Grouped`].
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

impl IntentModule {
    /// Every `struct`, `def` and `site` body, in that order, each in source order.
    pub(crate) fn bodies(&self) -> impl Iterator<Item = ItemBody<'_>> {
        let structs = self.structs.iter().map(|s| ItemBody {
            kind: BodyKind::Geometry,
            members: &s.members,
            asserts: &s.asserts,
        });
        let defs = self.defs.iter().map(|d| ItemBody {
            kind: BodyKind::Geometry,
            members: &d.members,
            asserts: &d.asserts,
        });
        let sites = self.sites.iter().map(|s| ItemBody {
            kind: BodyKind::Site,
            members: &s.placements,
            asserts: &s.asserts,
        });
        structs.chain(defs).chain(sites)
    }
}

/// One `struct`, `def` or `site` body, as [`IntentModule::bodies`] yields it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ItemBody<'a> {
    /// Which roles the body has a reader for.
    pub(crate) kind: BodyKind,
    /// The body's top-level members: a `site`'s are its placements.
    pub(crate) members: &'a [Member],
    /// The body's top-level `assert` properties.
    pub(crate) asserts: &'a [AssertIr],
}

/// `theme NAME:` block, with its slot bindings in a map.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeIr {
    /// Theme name.
    pub name: String,
    /// `slot NAME -> VALUE` bindings in source order; a repeated slot keeps its last value.
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
    /// Header `key=value` arguments other than a `size=WxH` hoisted into `size`.
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

/// Lifted form of `struct NAME[ ARGS]` (single-building structural composition).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StructIr {
    /// Struct name.
    pub name: String,
    /// Hoisted `size=WxH` header argument, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    /// Header `key=value` arguments other than a `size=WxH` hoisted into `size`.
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
    /// `place` / `connect` lines from the site body, as [`Member`]s.
    pub placements: Vec<Member>,
    /// `logic` bindings from the site body.
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

/// `assert ...` command lifted out of a body.
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
    /// Byte range of the originating `assert ...` command in source, through a table's `}`.
    #[must_use]
    pub fn span(&self) -> &Span {
        match self {
            Self::Truth { span, .. } | Self::Always { span, .. } => span,
        }
    }

    /// Every signal this property names, in declaration order.
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

/// One usable `circuit region=<label> void=<N>` reservation, with its scope's footprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct CircuitRegion {
    /// Which scope family the circuit member was declared under.
    pub scope_kind: ScopeKind,
    /// Source-level name of the scope (`struct gatehouse` → `"gatehouse"`).
    pub scope_name: String,
    /// `region=<label>` value the circuit member declared.
    pub label: String,
    /// `void=<N>` service-layer height, at least 1.
    pub void: u32,
    /// Width of the enclosing scope's footprint, copied from `size=WxH`.
    pub width: u32,
    /// Depth of the enclosing scope's footprint, copied from `size=WxH`.
    pub depth: u32,
    /// Byte range of the originating `circuit region=...` line.
    #[serde(skip)]
    pub span: Span,
}

/// The usable reservations among [`circuit_lines`], in the same order.
#[must_use]
pub fn circuit_regions(module: &IntentModule) -> Vec<CircuitRegion> {
    circuit_lines(module)
        .into_iter()
        .filter_map(Result::ok)
        .collect()
}

/// Every `circuit` line in a `struct` or `def` body, top level or under a `level`, read into the
/// reservation it makes or the [`CircuitRegionDefect`] that keeps it from making one. `site`
/// bodies are not walked.
#[must_use]
pub fn circuit_lines(module: &IntentModule) -> Vec<Result<CircuitRegion, RejectedCircuitRegion>> {
    let mut out = Vec::new();
    for s in &module.structs {
        collect_circuit_lines(
            ScopeKind::Struct,
            &s.name,
            s.size.as_ref(),
            &s.members,
            &mut out,
        );
    }
    for d in &module.defs {
        collect_circuit_lines(
            ScopeKind::Def,
            &d.name,
            d.size.as_ref(),
            &d.members,
            &mut out,
        );
    }
    out
}

/// A `circuit` line that reserves nothing, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RejectedCircuitRegion {
    /// Which scope family the circuit member was declared under.
    pub scope_kind: ScopeKind,
    /// Source-level name of the scope.
    pub scope_name: String,
    /// What is wrong with the line.
    pub defect: CircuitRegionDefect,
    /// Byte range of the originating `circuit ...` line.
    pub span: Span,
}

/// Why a `circuit` line is not a usable reservation: the first that applies, in the order listed.
/// Its `Display` is the reason clause, without a repair.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CircuitRegionDefect {
    /// The line sits under a `level`, at any depth.
    #[non_exhaustive]
    NestedUnderLevel,
    /// The enclosing scope has no `size=WxH` header to reserve within.
    #[non_exhaustive]
    NoSize,
    /// No `region=` key.
    #[non_exhaustive]
    RegionMissing,
    /// `region=` holds a value that is not an identifier or a string.
    #[non_exhaustive]
    RegionNotLabel {
        /// The value kind, as [`crate::ast::Value::kind_name`] names it.
        found: &'static str,
    },
    /// `region=` is an empty string.
    #[non_exhaustive]
    RegionEmpty,
    /// No `void=` key.
    #[non_exhaustive]
    VoidMissing,
    /// `void=` holds a value that is not an integer.
    #[non_exhaustive]
    VoidNotInteger {
        /// The value kind, as [`crate::ast::Value::kind_name`] names it.
        found: &'static str,
    },
    /// `void=` is an integer below 1, which reserves no service layer.
    #[non_exhaustive]
    VoidBelowOne {
        /// The integer written.
        value: i64,
    },
    /// `void=` is an integer above [`u32::MAX`].
    #[non_exhaustive]
    VoidTooLarge {
        /// The integer written.
        value: i64,
    },
}

impl fmt::Display for CircuitRegionDefect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NestedUnderLevel => f.write_str(
                "it is written under a `level`, and only a `circuit` line at the scope's top \
                 level is read as a reservation",
            ),
            Self::NoSize => {
                f.write_str("the enclosing scope has no `size=WxH` header for it to reserve within")
            }
            Self::RegionMissing => f.write_str("it has no `region=`"),
            Self::RegionNotLabel { found } => write!(
                f,
                "its `region=` must be an identifier or string label, got {found}",
            ),
            Self::RegionEmpty => f.write_str("its `region=` is an empty label"),
            Self::VoidMissing => f.write_str("it has no `void=`"),
            Self::VoidNotInteger { found } => {
                write!(f, "its `void=` must be an integer, got {found}")
            }
            Self::VoidBelowOne { value } => {
                write!(f, "its `void={value}` reserves no service layer")
            }
            Self::VoidTooLarge { value } => write!(
                f,
                "its `void={value}` is over the limit of {limit}",
                limit = u32::MAX,
            ),
        }
    }
}

/// Push each `circuit` line among `members`, and each under a `level` among them, onto `out`.
fn collect_circuit_lines(
    scope_kind: ScopeKind,
    scope_name: &str,
    size: Option<&Size>,
    members: &[Member],
    out: &mut Vec<Result<CircuitRegion, RejectedCircuitRegion>>,
) {
    for m in members {
        match m.role {
            MemberRole::Circuit => {
                let read = size.ok_or(CircuitRegionDefect::NoSize).and_then(|size| {
                    parse_circuit_region_fixture(m).map(|fixture| (size, fixture))
                });
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
            MemberRole::Level => {
                out.extend(circuit_lines_under_level(m).into_iter().map(|line| {
                    Err(RejectedCircuitRegion {
                        scope_kind,
                        scope_name: scope_name.to_owned(),
                        defect: CircuitRegionDefect::NestedUnderLevel,
                        span: line.span.clone(),
                    })
                }));
            }
            _ => {}
        }
    }
}

/// Each `circuit` line indented under `level`, at any depth through the `level`s in its body, in
/// source order.
pub(crate) fn circuit_lines_under_level(level: &Member) -> Vec<&Member> {
    let mut out = Vec::new();
    push_circuit_lines_under_level(level, &mut out);
    out
}

fn push_circuit_lines_under_level<'a>(level: &'a Member, out: &mut Vec<&'a Member>) {
    for m in &level.children.members {
        match m.role {
            MemberRole::Circuit => out.push(m),
            MemberRole::Level => push_circuit_lines_under_level(m, out),
            _ => {}
        }
    }
}

/// The `(label, void)` a `circuit` member declares, or its first defect.
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
