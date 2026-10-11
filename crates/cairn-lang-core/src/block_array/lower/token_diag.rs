//! Findings about a material token or state literal, worded for the site
//! that wrote it.

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::error::Span;
use crate::intent::ValueWithSpan;
use crate::suggest::{candidate_list, did_you_mean_note};

use crate::block_array::BlockState;
use crate::block_array::material::{IdOrigin, UnknownId};

use super::members::MemberFallback;

/// Where an `@token` was read from, for the prose of the abstract-token diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenSite {
    /// A `connect` row's `path=`.
    WalkwayPath,
    /// A member's `mat_slot=` binding, with what becomes of the member without it.
    MemberSlot(MemberFallback),
}

impl TokenSite {
    fn token_noun(self) -> &'static str {
        match self {
            Self::WalkwayPath => "abstract path token",
            Self::MemberSlot(_) => "abstract token",
        }
    }

    /// What the build does instead, following "cannot be lowered without the registry pack; ".
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

/// Warn that a state literal reaches the output unchecked, when `value` carries one.
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
    let pairs = state.properties_text();
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

/// Report a block id the compile's target does not declare, worded for who chose the id.
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
    // One note, the alias table winning it; the payload carries both answers.
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
