//! Validation passes over a parsed [`Module`] and its [`IntentModule`], collected by [`check`].
//! Block-array lowering and [`weigh_intended_targets`] raise codes [`check`] never sees; a caller
//! that runs them merges their findings itself.

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
mod struct_name;
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

/// Every pass's findings, [`crate::resolve::resolve`]'s included, sorted by span.
///
/// `edition` picks theme variants; `None` unions slot names across them
/// (`spec/versioning-editions` "Java / Bedrock portability").
#[must_use]
pub fn check(module: &Module, ir: &IntentModule, edition: Option<Edition>) -> Vec<Diagnostic> {
    let mut sink = DiagnosticSink::new();
    cairn_version::run(module, &mut sink);
    duplicate::run(module, ir, &mut sink);
    keyword_allowlist::run(ir, &mut sink);
    arguments::run(module, ir, &mut sink);
    binding::run(ir, &mut sink);
    material::run(ir, &mut sink);
    member_scope::run(ir, &mut sink);
    connect_arity::run(ir, &mut sink);
    nesting::run(ir, &mut sink);
    positional::run(ir, &mut sink);
    requires::run(module, &mut sink);
    struct_name::run(module, &mut sink);
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
    #[derive(Debug, PartialEq, Eq)]
    enum RaisedBy {
        Parser,
        Syntactic,
        Resolver,
        ResolverAndLowering,
        LoweringOnly,
        NeedsVersionTable,
    }

    /// No wildcard arm, so a new code must say which pass raises it.
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
            | C::InvalidStructName
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
            | C::InvalidMember
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

    #[test]
    fn the_intended_target_codes_are_the_ones_check_cannot_reach() {
        assert_eq!(
            codes_where(|c| raised_by(c) == RaisedBy::NeedsVersionTable),
            [
                "E_INTENDED_TARGET_CAP",
                "W_INTENDED_TARGET_CAP",
                "W_INTENDED_TARGET_UNSUPPORTED",
            ],
            "these codes are invisible to `check`",
        );
    }

    #[test]
    fn the_syntactic_error_codes_are_the_ones_the_cli_parity_table_lists() {
        assert_eq!(
            codes_where(|c| raised_by(c) == RaisedBy::Syntactic && c.severity() == Severity::Error),
            [
                "E_CONNECT_ARITY",
                "E_DUPLICATE_ARG",
                "E_DUPLICATE_CIRCUIT",
                "E_DUPLICATE_HEADER",
                "E_DUPLICATE_ID",
                "E_DUPLICATE_ITEM",
                "E_DUPLICATE_SELECTOR",
                "E_DUPLICATE_SIZE",
                "E_DUPLICATE_SLOT",
                "E_INVALID_REQUIRES",
                "E_INVALID_STRUCT_NAME",
                "E_MISPLACED_BINDING",
                "E_MISPLACED_MEMBER",
                "E_MISSING_MATERIAL",
                "E_TRUTH_TABLE_CONFLICT",
                "E_TRUTH_TABLE_DUPLICATE_INPUT",
                "E_TRUTH_TABLE_EMPTY",
                "E_TYPE_MISMATCH_LABEL",
                "E_TYPE_MISMATCH_SIZE",
                "E_UNEXPECTED_POSITIONAL",
                "E_UNKNOWN_ARGUMENT",
                "E_UNKNOWN_KEYWORD",
                "E_UNSUPPORTED_NESTING",
            ],
            "change `SYNTACTIC_FIXTURES` in cairn-lang-cli's tests/cli/check/parity.rs to match",
        );
    }

    #[test]
    fn exactly_one_code_belongs_to_the_parser() {
        assert_eq!(
            codes_where(|c| raised_by(c) == RaisedBy::Parser),
            ["E_PARSE"],
            "a parse failure renders under one code",
        );
    }

    #[test]
    fn check_sees_every_error_code_except_the_lowering_only_ones() {
        let escapes = codes_where(|c| {
            raised_by(c) == RaisedBy::LoweringOnly && c.severity() == Severity::Error
        });
        assert_eq!(
            escapes,
            [
                "E_INCOMPATIBLE_MATERIAL",
                "E_INVALID_MEMBER",
                "E_UNKNOWN_ABSTRACT_TOKEN",
                "E_UNKNOWN_ID",
            ],
            "`check` cannot raise these, so a front end reports them only by lowering too",
        );
    }
}
