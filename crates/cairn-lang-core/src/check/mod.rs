//! Diagnostic-collecting validation over a parsed [`Module`] and its
//! [`IntentModule`].
//!
//! Every pass is non-fatal: each pushes its findings into a
//! [`DiagnosticSink`], and [`check`] runs them all before sorting by source
//! position. The sort is stable, so the pass order in [`check`] decides the
//! order of findings that share a span.
//!
//! `crate::intent::lower` is total and never rejects input, so a structural
//! surprise (an unknown keyword, a duplicate `size=`, an `id=` whose value
//! is not a label) surfaces here as a [`Diagnostic`] rather than a parse
//! error, and one `cairn check` reports every problem in a file.
//!
//! Two passes run outside [`check`]. Block-array lowering raises codes
//! [`check`] never sees; `cairn check --edition E --target V` lowers beside
//! it and merges both streams. [`weigh_intended_targets`] needs the target
//! edition's `DataVersion` table, which reaches this crate only as a
//! [`crate::resolve::VersionOrder`] built from a registry pack, so a caller
//! that has one runs it beside [`check`] and merges the findings.

mod arguments;
mod binding;
mod cairn_version;
mod connect_arity;
mod duplicate;
mod intended_targets;
mod keyword_allowlist;
mod material;
mod member_scope;
mod nesting;
mod positional;
mod requires;
mod sink;
mod truth;
mod type_mismatch;

pub use crate::diagnostic::{
    Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote, RenderedDiagnostic, RenderedNote,
    Severity,
};
pub use crate::lines::{LineStarts, position_at};
pub use intended_targets::weigh_intended_targets;
pub use sink::DiagnosticSink;

use crate::ast::Module;
use crate::edition::Edition;
use crate::intent::IntentModule;

/// Run every validation pass over `module` and its IR, including
/// [`crate::resolve::resolve`], and return the findings sorted by
/// `(span.start, span.end)`.
///
/// No pass short-circuits on another's findings. The resolver's diagnostics
/// are already in the list, so a caller holding its own
/// [`crate::resolve::Resolution`] must not append `Resolution::diagnostics`
/// to it.
///
/// `edition` selects theme variants (`spec/versioning-editions` "Java /
/// Bedrock portability"). `None` means no target is picked yet: the
/// resolver then unions slot names across a theme's variants, so a
/// `mat_slot=` only one variant declares does not raise `E_UNRESOLVED_SLOT`.
#[must_use]
pub fn check(module: &Module, ir: &IntentModule, edition: Option<Edition>) -> Vec<Diagnostic> {
    let mut sink = DiagnosticSink::new();
    cairn_version::run(module, &mut sink);
    duplicate::run(module, ir, &mut sink);
    keyword_allowlist::run(ir, &mut sink);
    arguments::run(ir, &mut sink);
    binding::run(ir, &mut sink);
    material::run(ir, &mut sink);
    member_scope::run(ir, &mut sink);
    connect_arity::run(ir, &mut sink);
    nesting::run(ir, &mut sink);
    positional::run(ir, &mut sink);
    requires::run(module, &mut sink);
    truth::run(ir, &mut sink);
    type_mismatch::run(module, &mut sink);
    for d in crate::resolve::resolve(ir, edition).diagnostics {
        sink.push(d);
    }
    sink.into_sorted()
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::{DiagnosticCode, DiagnosticCode as C, Severity};

    /// Which pass raises a code, and therefore whether [`check`] can see it.
    ///
    /// The match is exhaustive on purpose. `DiagnosticCode` is
    /// `#[non_exhaustive]` for downstream crates but not in-crate, so a new
    /// variant stops this file compiling until someone says where it comes
    /// from.
    #[derive(Debug, PartialEq, Eq)]
    enum RaisedBy {
        /// `crate::parse`. A source that reaches [`check`] has parsed, so
        /// commands build this from the [`crate::error::ParseError`].
        Parser,
        /// A pass [`check`] runs directly.
        Syntactic,
        /// `crate::resolve::resolve`, whose diagnostics [`check`] merges in.
        Resolver,
        /// Raised from both `crate::resolve` and `crate::block_array`, so
        /// `cairn check` reports a subset of the occurrences a build does.
        ResolverAndLowering,
        /// `crate::block_array` only. [`check`] never runs that pass.
        LoweringOnly,
        /// [`weigh_intended_targets`], which needs a registry pack's
        /// version table and so runs beside [`check`], never inside it.
        NeedsVersionTable,
    }

    fn raised_by(code: DiagnosticCode) -> RaisedBy {
        match code {
            C::Parse => RaisedBy::Parser,
            C::DuplicateSize
            | C::DuplicateSlot
            | C::DuplicateSelector
            | C::DuplicateArg
            | C::DuplicateId
            | C::DuplicateItem
            | C::DuplicateHeader
            | C::DuplicateCircuit
            | C::UnsupportedNesting
            | C::MisplacedMember
            | C::UnknownKeyword
            | C::UnknownArgument
            | C::MisplacedBinding
            | C::UnexpectedPositional
            | C::InvalidRequires
            | C::InvalidCairnVersion
            | C::FutureCairnVersion
            | C::TypeMismatchLabel
            | C::TypeMismatchSize
            | C::ConnectArity
            | C::TruthTableEmpty
            | C::TruthTableDuplicateInput
            | C::TruthTableConflict
            | C::TruthTableDuplicateRow
            | C::TruthTablePartial
            | C::MissingMaterial => RaisedBy::Syntactic,
            C::UnresolvedSlot
            | C::ThemeSelectorUnmatched
            | C::UnresolvedPlaceRef
            | C::UnresolvedThemeRef
            | C::DuplicatePlaceId
            | C::OutputNameCollision
            | C::InvalidPlaceOrigin
            | C::UnusedDef
            | C::UnresolvedPort
            | C::AmbiguousPort
            | C::MissingPathMaterial
            | C::InvalidPlaceId
            | C::IncompletePlace
            | C::DeferredConnect
            | C::DeferredPlace
            | C::ThemeVariantMissing
            | C::ThemeVariantRebound => RaisedBy::Resolver,
            C::UnknownSlotTarget => RaisedBy::ResolverAndLowering,
            C::DeferredMember
            | C::IgnoredArgument
            | C::NoThemeBound
            | C::AbstractTokenDeferred
            | C::StateLiteralUnchecked
            | C::UnknownAbstractToken
            | C::UnknownId
            | C::IncompatibleMaterial
            | C::StructNoSize
            | C::DefNoSize
            | C::WalkwayBlocked
            | C::DuplicateWalkway
            | C::StructureTooLarge
            | C::PaletteTooLarge
            | C::InvalidWalkwayIdent
            | C::PhaseConflict => RaisedBy::LoweringOnly,
            C::IntendedTargetCap | C::IntendedTargetCapPartial | C::IntendedTargetUnsupported => {
                RaisedBy::NeedsVersionTable
            }
        }
    }

    fn codes_where(predicate: impl Fn(DiagnosticCode) -> bool) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = DiagnosticCode::iter()
            .filter(|c| predicate(*c))
            .map(DiagnosticCode::as_str)
            .collect();
        names.sort_unstable();
        names
    }

    /// A consumer that calls only [`check`] (the LSP, the wasm surface) sees
    /// none of these, so another one landing here should be a decision.
    #[test]
    fn the_intended_target_codes_are_the_ones_check_cannot_reach() {
        assert_eq!(
            codes_where(|c| raised_by(c) == RaisedBy::NeedsVersionTable),
            [
                "E_INTENDED_TARGET_CAP",
                "W_INTENDED_TARGET_CAP",
                "W_INTENDED_TARGET_UNSUPPORTED",
            ],
            "a code whose pass needs a registry pack's version table is \
             invisible to `check`, so a caller that only calls `check` \
             reports a source clean that a build refuses",
        );
    }

    /// Every consumer renders a parse failure under this one code.
    #[test]
    fn exactly_one_code_belongs_to_the_parser() {
        assert_eq!(
            codes_where(|c| raised_by(c) == RaisedBy::Parser),
            ["E_PARSE"],
            "a parse failure renders under one code, whatever shape it took",
        );
    }

    /// An unpinned `cairn check` passes a source raising one of these that
    /// `cairn compile` then refuses; only `--edition E --target V` lowers and
    /// catches them. A new entry widens that gap, so it should be a decision.
    #[test]
    fn check_sees_every_error_code_except_the_lowering_only_ones() {
        let escapes = codes_where(|c| {
            raised_by(c) == RaisedBy::LoweringOnly && c.severity() == Severity::Error
        });
        assert_eq!(
            escapes,
            [
                "E_INCOMPATIBLE_MATERIAL",
                "E_UNKNOWN_ABSTRACT_TOKEN",
                "E_UNKNOWN_ID",
            ],
            "an Error-severity code raised only during block-array lowering \
             cannot be reported by `cairn check`, so a CI job gating on it \
             goes green on a source `cairn compile` refuses",
        );
    }
}
