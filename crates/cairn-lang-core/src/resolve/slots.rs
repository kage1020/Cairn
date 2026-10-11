//! Theme slot findings: a `mat_slot=` the bound theme does not declare, a
//! slot value that is not a material token, and a selector that matched
//! nothing.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::{Member, MemberRole, ValueWithSpan, role_of};
use crate::prose::selector_text;
use crate::suggest::{did_you_mean_note, nearest_match};

use super::binding::{ThemeBinding, TokenKind, classify_token};

pub(super) fn unresolved_slot_diag(
    slot: &str,
    theme_name: &str,
    member: &Member,
    available_slots: &IndexMap<String, ValueWithSpan>,
) -> Diagnostic {
    // Suggestions come from the applied theme only: a `mat_slot=` cannot
    // bind across themes.
    let mut notes = Vec::with_capacity(2);
    notes.extend(
        nearest_match(slot, available_slots.keys().map(String::as_str)).map(did_you_mean_note),
    );
    notes.push(DiagnosticNote {
        span: None,
        message: format!(
            "add `slot {slot} -> @...` to theme `{theme_name}` or correct the slot name",
        ),
    });
    Diagnostic {
        code: DiagnosticCode::UnresolvedSlot,
        span: member.span.clone(),
        primary: format!("`mat_slot={slot}` is not declared in theme `{theme_name}`"),
        notes,
        data: None,
    }
}

/// Flag slot values that are not material tokens.
///
/// Takes every *declared* theme rather than the bound map: the check is
/// local to one body and needs no resolution, so a body whose name lost
/// the binding still gets its findings. Selector matching is the
/// opposite case — see [`check_unmatched_selectors`].
pub(super) fn check_slot_targets(themes: &[ThemeBinding], diagnostics: &mut Vec<Diagnostic>) {
    for theme in themes {
        for (slot_name, v) in &theme.slots {
            if classify_token(&v.value) == TokenKind::NotAToken {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::UnknownSlotTarget,
                    span: v.span.clone(),
                    primary: format!(
                        "slot `{slot}` in theme `{theme}` does not bind to a canonical or abstract material token",
                        slot = slot_name,
                        theme = theme.name,
                    ),
                    notes: vec![DiagnosticNote {
                        span: None,
                        message: "expected a `@canonical_block` or `@abstract.material` value"
                            .to_owned(),
                    }],
                    data: None,
                });
            }
        }
    }
}

/// Flag theme selectors that matched no member.
///
/// Takes the bound map, not every declared body. "Matched nothing" is a
/// statement about a resolution this selector took part in, and a body
/// whose name lost the binding never got to take part — reporting its
/// selectors would blame them for a name collision reported elsewhere.
/// The body-local counterpart is [`check_slot_targets`].
pub(super) fn check_unmatched_selectors(
    themes: &IndexMap<String, ThemeBinding>,
    applied_themes: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for theme in themes.values() {
        // A theme no scope applies matched nothing vacuously.
        if !applied_themes.contains(&theme.name) {
            continue;
        }
        for sel in &theme.selectors {
            // `E_UNKNOWN_KEYWORD` already covers this span.
            if matches!(role_of(&sel.keyword), MemberRole::Other(_)) {
                continue;
            }
            if sel.matched_member_spans.is_empty() {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::ThemeSelectorUnmatched,
                    span: sel.source_span.clone(),
                    primary: format!(
                        "theme selector `{selector}` in `{theme}` does not match any member",
                        selector = selector_text(&sel.keyword, &sel.attrs),
                        theme = theme.name,
                    ),
                    notes: vec![DiagnosticNote {
                        span: None,
                        message: "remove the selector or relax its attribute filters".to_owned(),
                    }],
                    data: None,
                });
            }
        }
    }
}
