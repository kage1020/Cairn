//! Diagnostic-collecting validation over a parsed [`Module`] and its
//! [`IntentModule`].
//!
//! Each pass is non-fatal: passes accumulate findings into a
//! [`DiagnosticSink`] and the top-level [`check`] runs every pass before
//! returning. The order `duplicate` → `keyword_allowlist` →
//! `member_scope` → `connect_arity` → `nesting` → `positional` →
//! `requires` → `truth` → `type_mismatch` → [`crate::resolve::resolve`] is fixed so the emitted
//! list is stable across runs, but the diagnostics themselves are sorted by
//! source position once everything has finished collecting.
//!
//! Block-array lowering is *not* among those passes, so an `Error` it
//! raises never reaches `cairn check`. `check::tests` pins which codes that
//! covers.
//!
//! The boundary with lowering is intentional: `crate::intent::lower` is a
//! total function (see its module doc) and never rejects input. Any
//! "structural surprise" — an unknown keyword, a duplicate `size=`, an `id=`
//! whose value is not a label — surfaces here as a [`Diagnostic`] instead of
//! a hard parse error, so a single `cairn check` invocation reports every
//! problem in a file rather than only the first one.

mod arguments;
mod connect_arity;
mod diagnostic;
mod duplicate;
mod keyword_allowlist;
mod member_scope;
mod nesting;
mod positional;
mod requires;
mod sink;
mod truth;
mod type_mismatch;

pub use diagnostic::{
    Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote, LineStarts, RenderedDiagnostic,
    RenderedNote, Severity, position_at,
};
pub use sink::DiagnosticSink;

use crate::ast::Module;
use crate::edition::Edition;
use crate::intent::IntentModule;

/// Run every syntactic + semantic validation pass over the given module +
/// IR and return the merged, span-sorted findings.
///
/// Passes run unconditionally; none short-circuit, none depend on another's
/// findings being empty. The returned list is sorted by `(span.start,
/// span.end)` so consumers can stream it line-by-line.
///
/// A final theme-binding pass runs via [`crate::resolve::resolve`]; its
/// diagnostics (`E_UNRESOLVED_SLOT`, `E_UNKNOWN_SLOT_TARGET`,
/// `E_THEME_SELECTOR_UNMATCHED`) are merged with the syntactic findings so a
/// single `cairn check` invocation reports both kinds together. A caller
/// that has its own [`crate::resolve::Resolution`] must therefore *not*
/// append `Resolution::diagnostics` on top of this list — they are already
/// in it, and appending duplicates every one of them.
///
/// The `edition` argument threads through to the resolver so per-edition
/// theme-variant selection (spec versioning-editions §10.7) can pin the
/// diagnostic set for a specific target. Pass `None` when no target has
/// been picked yet (the CLI's `cairn check` without `--edition`); the
/// resolver then unions slot names across variants of one logical theme
/// so `mat_slot=` references that only one variant declares don't
/// spuriously fire `E_UNRESOLVED_SLOT`.
#[must_use]
pub fn check(module: &Module, ir: &IntentModule, edition: Option<Edition>) -> Vec<Diagnostic> {
    let mut sink = DiagnosticSink::new();
    duplicate::run(module, ir, &mut sink);
    keyword_allowlist::run(ir, &mut sink);
    arguments::run(ir, &mut sink);
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
    /// [`check`] runs the syntactic passes and merges `resolve`'s output. It
    /// does **not** run block-array lowering, so a code raised only there is
    /// invisible to `cairn check` no matter its severity — the asymmetry
    /// `check_sees_every_error_code_except_the_lowering_only_ones` pins.
    ///
    /// The match is exhaustive on purpose. `DiagnosticCode` is
    /// `#[non_exhaustive]` for downstream crates but not in-crate, so a new
    /// variant stops this file compiling until someone says where it comes
    /// from.
    #[derive(Debug, PartialEq, Eq)]
    enum RaisedBy {
        /// `duplicate` / `keyword_allowlist` / `member_scope` /
        /// `connect_arity` / `nesting` / `positional` / `requires` /
        /// `truth` / `type_mismatch`, run directly by [`check`].
        Syntactic,
        /// `crate::resolve::resolve`, whose diagnostics [`check`] merges in.
        Resolver,
        /// Raised from both `crate::resolve` and `crate::block_array`, so
        /// `cairn check` reports a subset of the occurrences a build does.
        ResolverAndLowering,
        /// `crate::block_array` only. [`check`] never runs that pass.
        LoweringOnly,
    }

    fn raised_by(code: DiagnosticCode) -> RaisedBy {
        match code {
            C::DuplicateSize
            | C::DuplicateSlot
            | C::DuplicateSelector
            | C::DuplicateArg
            | C::DuplicateId
            | C::DuplicateItem
            | C::DuplicateHeader
            | C::UnsupportedNesting
            | C::MisplacedMember
            | C::UnknownKeyword
            | C::UnknownArgument
            | C::UnexpectedPositional
            | C::InvalidRequires
            | C::TypeMismatchLabel
            | C::TypeMismatchSize
            | C::ConnectArity
            | C::TruthTableEmpty
            | C::TruthTableConflict
            | C::TruthTableDuplicateRow
            | C::TruthTablePartial => RaisedBy::Syntactic,
            C::UnresolvedSlot
            | C::ThemeSelectorUnmatched
            | C::UnresolvedPlaceRef
            | C::UnresolvedThemeRef
            | C::DuplicatePlaceId
            | C::InvalidPlaceOrigin
            | C::UnusedDef
            | C::UnresolvedPort
            | C::AmbiguousPort
            | C::MissingPathMaterial
            | C::InvalidPlaceId
            | C::IncompletePlace
            | C::DeferredConnect
            | C::ThemeVariantMissing
            | C::ThemeVariantRebound => RaisedBy::Resolver,
            C::UnknownSlotTarget => RaisedBy::ResolverAndLowering,
            C::DeferredMember
            | C::IgnoredArgument
            | C::NoThemeBound
            | C::AbstractTokenDeferred
            | C::UnknownAbstractToken
            | C::UnknownId
            | C::IncompatibleMaterial
            | C::StructNoSize
            | C::DefNoSize
            | C::WalkwayBlocked
            | C::DuplicateWalkway
            | C::StructureTooLarge
            | C::InvalidWalkwayIdent
            | C::PhaseConflict => RaisedBy::LoweringOnly,
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

    /// The set `cairn-lang-cli/tests/cli_check_parity.rs` keeps a fixture for.
    ///
    /// These are the codes only [`check`] produces, so before the build
    /// commands started calling it they reached `cairn check` and nothing
    /// else. Adding one without a fixture would leave that gap reopenable
    /// without any test noticing.
    #[test]
    fn syntactic_error_codes_are_covered_by_cli_fixtures() {
        let actual =
            codes_where(|c| raised_by(c) == RaisedBy::Syntactic && c.severity() == Severity::Error);
        assert_eq!(
            actual,
            [
                "E_CONNECT_ARITY",
                "E_DUPLICATE_ARG",
                "E_DUPLICATE_HEADER",
                "E_DUPLICATE_ID",
                "E_DUPLICATE_ITEM",
                "E_DUPLICATE_SELECTOR",
                "E_DUPLICATE_SIZE",
                "E_DUPLICATE_SLOT",
                "E_INVALID_REQUIRES",
                "E_MISPLACED_MEMBER",
                "E_TRUTH_TABLE_CONFLICT",
                "E_TRUTH_TABLE_EMPTY",
                "E_TYPE_MISMATCH_LABEL",
                "E_TYPE_MISMATCH_SIZE",
                "E_UNEXPECTED_POSITIONAL",
                "E_UNKNOWN_KEYWORD",
                "E_UNSUPPORTED_NESTING",
            ],
            "the syntactic Error set changed: add or remove the matching \
             fixture in cairn-lang-cli/tests/cli_check_parity.rs so \
             `cairn lower` / `info` / `compile` stay in step with `cairn check`",
        );
    }

    /// `cairn check` is documented and used as the gate the build commands
    /// sit behind, but it does not lower, so an `Error` raised during
    /// block-array lowering escapes it: `cairn check` exits 0 and
    /// `cairn compile` then exits 1 on the same file.
    ///
    /// This pins the size of that hole rather than leaving it implied. A new
    /// entry here means another way for `cairn check` to pass a source the
    /// build refuses, so it should be a deliberate decision, not a side
    /// effect.
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
