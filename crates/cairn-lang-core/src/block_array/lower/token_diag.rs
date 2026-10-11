//! Findings about a material token or state literal, worded for the site
//! that wrote it.

use crate::ast::ValueKind;
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::error::Span;
use crate::intent::ValueWithSpan;
use crate::suggest::{candidate_list, did_you_mean_note};

use crate::block_array::BlockState;
use crate::block_array::material::{IdOrigin, UnknownId};

use super::members::MemberFallback;

/// Where an `@token` was read from, for the prose of the abstract-token
/// diagnostics. [`Self::MemberSlot`] also carries what gets built without
/// the material, which [`Self::consequence`] words.
///
/// Only `W_ABSTRACT_TOKEN_DEFERRED` reads that payload.
/// [`diag_unknown_abstract_token`] reads [`Self::token_noun`] and
/// [`Self::catalog_note`] alone, because `E_UNKNOWN_ABSTRACT_TOKEN` stops the
/// build and there is no fallback to describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenSite {
    /// A `connect` row's `path=`.
    WalkwayPath,
    /// A member's `mat_slot=` binding, with what becomes of that member
    /// when the binding resolves to nothing.
    MemberSlot(MemberFallback),
}

impl TokenSite {
    fn token_noun(self) -> &'static str {
        match self {
            Self::WalkwayPath => "abstract path token",
            Self::MemberSlot(_) => "abstract token",
        }
    }

    /// What the build does instead, worded to follow "cannot be lowered
    /// without the registry pack; ".
    fn consequence(self) -> &'static str {
        match self {
            Self::WalkwayPath => "the walkway falls back to air",
            Self::MemberSlot(MemberFallback::Air) => "the cell falls back to air",
            Self::MemberSlot(MemberFallback::Absent) => {
                "the walls are not built and take up no rows, so a door or window cut into them \
                 is refused"
            }
            Self::MemberSlot(MemberFallback::WallStays) => {
                "the window is not cut, and the wall stays"
            }
            Self::MemberSlot(MemberFallback::DefaultBlock) => {
                "the member is built from its default block"
            }
        }
    }

    fn canonical_example(self) -> &'static str {
        match self {
            Self::WalkwayPath => "path=@gravel",
            Self::MemberSlot(_) => "@oak_planks",
        }
    }

    fn catalog_note(self) -> &'static str {
        match self {
            Self::WalkwayPath => {
                "abstract path tokens must be declared in the pack's `materials` catalog"
            }
            Self::MemberSlot(_) => {
                "abstract material tokens must be declared in the pack's `materials` catalog \
                 (see `spec/materials-themes` \"Canonical vocabulary\")"
            }
        }
    }
}

pub(super) fn diag_abstract_token(span: Span, token: &str, site: TokenSite) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::AbstractTokenDeferred,
        span,
        primary: format!(
            "{} `@{token}` cannot be lowered without the registry pack; {}",
            site.token_noun(),
            site.consequence(),
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "use a canonical block token (e.g. `{}`) until the registry pack ships",
                site.canonical_example(),
            ),
        }],
        data: None,
    }
}

pub(super) fn diag_unknown_abstract_token(
    span: Span,
    token: &str,
    suggestion: Option<&str>,
    site: TokenSite,
) -> Diagnostic {
    let mut notes = Vec::with_capacity(2);
    notes.extend(suggestion.map(|s| did_you_mean_note(&format!("@{s}"))));
    notes.push(DiagnosticNote {
        span: None,
        message: site.catalog_note().to_owned(),
    });
    Diagnostic {
        code: DiagnosticCode::UnknownAbstractToken,
        span,
        primary: format!(
            "{} `@{token}` is not declared by the registry pack's materials catalog",
            site.token_noun(),
        ),
        notes,
        data: None,
    }
}

/// Say that a state literal was taken as written, when `value` carries one.
///
/// `spec/versioning-editions` "Fail-loud and minimum-version inference"
/// makes an out-of-domain state a hard error, `E_STATE_DOMAIN`, and
/// nothing raises it yet: no table this compiler holds says which
/// properties a block has or which values each takes. Until one does, the
/// literal reaches the palette and the structure file unchanged, right or
/// wrong, and this warning is what keeps that from being silent. It
/// anchors on the value the way `E_UNKNOWN_ID` does, so the mistake right
/// of the `[` is pointed at from the same place as one left of it.
///
/// Only a canonical token folds a `[` into its text, so a state that came
/// from anywhere else — a catalog lookup, a member default — is not one.
pub(super) fn diag_state_literal_unchecked(
    value: &ValueWithSpan,
    state: &BlockState,
) -> Option<Diagnostic> {
    let ValueKind::Token(text) = &value.value.kind else {
        return None;
    };
    if state.properties.is_empty() || !text.contains('[') {
        return None;
    }
    let pairs = state
        .properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",");
    Some(Diagnostic {
        code: DiagnosticCode::StateLiteralUnchecked,
        span: value.span.clone(),
        primary: format!(
            "`{id}` is written with `{pairs}` unchecked: nothing checks a state literal's \
             properties or values against the target yet",
            id = state.id,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "a property the block does not have, or a value outside its domain, \
                      reaches the structure file unchanged; check each one against the \
                      block's states in the target edition and version"
                .to_owned(),
        }],
        data: None,
    })
}

/// Report a block id the compile's target does not declare.
///
/// One builder serves both the member and the walkway site: the span
/// differs and nothing else does. The prose fork that matters is not which
/// statement the id sat on but *who chose it* — an author can correct what
/// they typed, while a catalog mapping is not theirs to edit and a message
/// that blames their token sends them to the wrong file.
pub(super) fn diag_unknown_id(span: Span, unknown: &UnknownId) -> Diagnostic {
    let UnknownId {
        id,
        registry,
        origin,
        suggestion,
        aliases,
    } = unknown;
    let mut notes = Vec::with_capacity(2);
    match origin {
        IdOrigin::Authored => {}
        IdOrigin::Catalog { token } => notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "the registry pack maps `@{token}` onto it; the token is fine and the pack's mapping is not, so this is not a fix you make here",
            ),
        }),
        IdOrigin::Builtin { token } => notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "the registry pack declares no `{token}`, so lowering fell back to the id compiled into the compiler; the pack needs that row",
            ),
        }),
    }
    // One note, and the alias table wins it when it has anything to say.
    // The two answers are not additive: a reader handed "this target calls
    // it X" and "the nearest spelling is Y" has to work out which of the
    // two the compiler believes, and the answer is always the first. The
    // payload still carries both, for a consumer that can show them apart.
    notes.push(DiagnosticNote {
        span: None,
        message: match (aliases.as_slice(), suggestion) {
            ([], None) => format!(
                "no block in `{registry}` is near enough to suggest; compile against a target that declares `{id}`, or pick a block this one has",
            ),
            ([], Some(candidate)) => {
                format!("`{registry}` spells the nearest block `{candidate}`")
            }
            (spellings, _) => format!(
                "`{registry}` spells this block {}, per the registry pack's alias table",
                candidate_list(spellings),
            ),
        },
    });
    Diagnostic {
        code: DiagnosticCode::UnknownId,
        span,
        primary: format!("`{id}` is not a block in `{registry}`"),
        notes,
        data: Some(DiagnosticData::UnknownId {
            id: id.clone(),
            registry: registry.clone(),
            origin: origin.kind().to_owned(),
            token: origin.token().map(str::to_owned),
            suggestion: suggestion.clone(),
            aliases: aliases.clone(),
        }),
    }
}
