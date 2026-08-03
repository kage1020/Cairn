//! Diagnostic-collecting validation over a parsed [`Module`] and its
//! [`IntentModule`].
//!
//! Each pass is non-fatal: passes accumulate findings into a
//! [`DiagnosticSink`] and the top-level [`check`] runs every pass before
//! returning. The order `duplicate` → `keyword_allowlist` →
//! `connect_arity` → `type_mismatch` is fixed so the emitted list is
//! stable across runs, but the diagnostics themselves are sorted by
//! source position once everything has finished collecting.
//!
//! The boundary with lowering is intentional: `crate::intent::lower` is a
//! total function (see its module doc) and never rejects input. Any
//! "structural surprise" — an unknown keyword, a duplicate `size=`, an `id=`
//! whose value is not a label — surfaces here as a [`Diagnostic`] instead of
//! a hard parse error, so a single `cairn check` invocation reports every
//! problem in a file rather than only the first one.

mod connect_arity;
mod diagnostic;
mod duplicate;
mod keyword_allowlist;
mod sink;
mod type_mismatch;

pub use diagnostic::{
    Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote, LineStarts, RenderedDiagnostic,
    RenderedNote, Severity, position_at,
};
pub use sink::DiagnosticSink;

use crate::ast::Module;
use crate::edition::Edition;
use crate::intent::IntentModule;

/// Run every validation pass over the given module + IR and collect all
/// findings.
///
/// Passes run unconditionally; none short-circuit, none depend on another's
/// findings being empty. The returned list is sorted by `(span.start,
/// span.end)` so consumers can stream it line-by-line.
///
/// A final theme-binding pass runs via [`crate::resolve::resolve`]; its
/// diagnostics (`E_UNRESOLVED_SLOT`, `E_UNKNOWN_SLOT_TARGET`,
/// `E_THEME_SELECTOR_UNMATCHED`) are merged with the syntactic findings so a
/// single `cairn check` invocation reports both kinds together.
/// Run every syntactic + semantic diagnostic pass and return the merged,
/// span-sorted findings.
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
    duplicate::run(module, &mut sink);
    keyword_allowlist::run(ir, &mut sink);
    connect_arity::run(ir, &mut sink);
    type_mismatch::run(module, &mut sink);
    for d in crate::resolve::resolve(ir, edition).diagnostics {
        sink.push(d);
    }
    sink.into_sorted()
}

#[cfg(test)]
mod tests {
    use super::DiagnosticCode as C;
    use super::{DiagnosticCode, Severity};

    /// Which pass inside [`check`] can raise a given code.
    ///
    /// The distinction matters to the CLI: resolver findings already reached
    /// `cairn lower` / `info` / `compile` through `Resolution::diagnostics`,
    /// but the syntactic passes ran only under `cairn check` until those
    /// commands started calling [`check`] themselves. Every `Error`-severity
    /// code in the [`Origin::Syntactic`] set therefore needs a fixture in
    /// `cairn-lang-cli/tests/cli_check_parity.rs`, which is what keeps the
    /// four commands from drifting apart again.
    ///
    /// The match below is deliberately exhaustive. `DiagnosticCode` is
    /// `#[non_exhaustive]` for downstream crates but not in-crate, so a new
    /// variant stops this file compiling and forces the author to say which
    /// pass raises it.
    #[derive(Debug, PartialEq, Eq)]
    enum Origin {
        /// `duplicate` / `keyword_allowlist` / `connect_arity` /
        /// `type_mismatch`, run directly by [`check`].
        Syntactic,
        /// `crate::resolve::resolve`, whose diagnostics [`check`] merges in.
        Resolver,
        /// `crate::block_array`, reported by the commands that lower.
        Lowering,
    }

    fn origin(code: DiagnosticCode) -> Origin {
        match code {
            C::DuplicateSize
            | C::DuplicateSlot
            | C::DuplicateArg
            | C::DuplicateId
            | C::UnknownKeyword
            | C::TypeMismatchLabel
            | C::TypeMismatchSize
            | C::ConnectArity => Origin::Syntactic,
            C::UnresolvedSlot
            | C::UnknownSlotTarget
            | C::ThemeSelectorUnmatched
            | C::NoThemeBound
            | C::AbstractTokenDeferred
            | C::UnknownAbstractToken
            | C::StructNoSize
            | C::DefNoSize
            | C::UnresolvedPlaceRef
            | C::UnresolvedThemeRef
            | C::DuplicatePlaceId
            | C::InvalidPlaceOrigin
            | C::UnusedDef
            | C::UnresolvedPort
            | C::AmbiguousPort
            | C::MissingPathMaterial => Origin::Resolver,
            C::DeferredMember
            | C::WalkwayBlocked
            | C::DuplicateWalkway
            | C::DeferredConnect
            | C::InvalidWalkwayIdent => Origin::Lowering,
        }
    }

    /// Every variant, so the assertions below scan the whole surface rather
    /// than whichever ones happen to be listed. Keep in step with the enum;
    /// `origin`'s exhaustive match is what makes a missing addition visible.
    const ALL: &[DiagnosticCode] = &[
        C::DuplicateSize,
        C::DuplicateSlot,
        C::DuplicateArg,
        C::DuplicateId,
        C::UnknownKeyword,
        C::TypeMismatchLabel,
        C::TypeMismatchSize,
        C::UnresolvedSlot,
        C::UnknownSlotTarget,
        C::ThemeSelectorUnmatched,
        C::DeferredMember,
        C::NoThemeBound,
        C::AbstractTokenDeferred,
        C::UnknownAbstractToken,
        C::StructNoSize,
        C::DefNoSize,
        C::UnresolvedPlaceRef,
        C::UnresolvedThemeRef,
        C::DuplicatePlaceId,
        C::InvalidPlaceOrigin,
        C::UnusedDef,
        C::UnresolvedPort,
        C::AmbiguousPort,
        C::MissingPathMaterial,
        C::WalkwayBlocked,
        C::DuplicateWalkway,
        C::DeferredConnect,
        C::InvalidWalkwayIdent,
        C::ConnectArity,
    ];

    /// Pins the exact set the CLI parity fixtures have to cover. A new
    /// syntactic `Error` code lands here first, and the failure message
    /// says where else it has to go.
    #[test]
    fn syntactic_error_codes_match_the_cli_parity_fixtures() {
        let mut actual: Vec<&str> = ALL
            .iter()
            .copied()
            .filter(|c| origin(*c) == Origin::Syntactic && c.severity() == Severity::Error)
            .map(DiagnosticCode::as_str)
            .collect();
        actual.sort_unstable();

        let expected = [
            "E_CONNECT_ARITY",
            "E_DUPLICATE_ARG",
            "E_DUPLICATE_ID",
            "E_DUPLICATE_SIZE",
            "E_DUPLICATE_SLOT",
            "E_TYPE_MISMATCH_LABEL",
            "E_TYPE_MISMATCH_SIZE",
            "E_UNKNOWN_KEYWORD",
        ];
        assert_eq!(
            actual, expected,
            "the syntactic Error set changed: add or remove the matching \
             fixture in cairn-lang-cli/tests/cli_check_parity.rs so \
             `cairn lower` / `info` / `compile` stay in step with `cairn check`",
        );
    }

    /// Guards `ALL` itself: `origin`'s match catches a new variant, but only
    /// if the variant also reaches these scans.
    #[test]
    fn all_lists_every_code_exactly_once() {
        let mut names: Vec<&str> = ALL.iter().copied().map(DiagnosticCode::as_str).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "ALL repeats a code");
        assert_eq!(
            before, 29,
            "ALL is out of step with DiagnosticCode; add the new variant here too",
        );
    }
}
