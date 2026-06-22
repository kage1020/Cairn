//! `keyword_allowlist` pass — flags every member whose role lowered to
//! [`MemberRole::Other`], and every theme selector whose leading keyword
//! is outside the M2 table.
//!
//! Walks the Intent IR rather than the AST because the role classification
//! already happened during lowering; `MemberRole::Other(kw)` is the single
//! source of truth for "this keyword wasn't in the M2 table" on
//! struct/def/site bodies. For theme selectors the lowering step keeps
//! the raw keyword string, so this pass re-checks it directly via
//! [`role_of`](crate::intent::role_of).

use crate::intent::{IntentModule, Member, MemberRole, known_keywords, role_of};
use crate::suggest::nearest_match;

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for s in &ir.structs {
        walk(&s.members, sink);
    }
    for d in &ir.defs {
        walk(&d.members, sink);
    }
    for s in &ir.sites {
        walk(&s.placements, sink);
    }
    for theme in &ir.themes {
        for rule in &theme.selectors {
            if matches!(role_of(&rule.keyword), MemberRole::Other(_)) && !rule.keyword.is_empty() {
                push_unknown_keyword(&rule.keyword, &rule.span, sink);
            }
        }
    }
}

fn walk(members: &[Member], sink: &mut DiagnosticSink) {
    for m in members {
        if let MemberRole::Other(kw) = &m.role {
            // The synthetic `placeholder_member_carrying` in `intent::lower`
            // emits `MemberRole::Other(String::new())` from a dead branch
            // (its only caller never trips it). Skipping the empty-string
            // case keeps a hypothetical future bug there from manifesting
            // as a confusing ``unknown keyword `` `` diagnostic.
            if !kw.is_empty() {
                push_unknown_keyword(kw, &m.span, sink);
            }
        }
        walk(&m.children.members, sink);
    }
}

fn push_unknown_keyword(keyword: &str, span: &crate::error::Span, sink: &mut DiagnosticSink) {
    // Suggestion goes *before* the candidate list so a user reading top-down
    // sees the targeted fix first; the closed-set listing stays as the
    // fallback when the typo is too far from any keyword to suggest.
    // Informational notes — no distinct secondary location, so renderers
    // skip the `file:L:C:` prefix and just print `note: ...`.
    let mut notes = Vec::with_capacity(2);
    if let Some(suggested) = nearest_match(keyword, known_keywords().iter().copied()) {
        notes.push(DiagnosticNote {
            span: None,
            message: format!("did you mean `{suggested}`?"),
        });
    }
    notes.push(DiagnosticNote {
        span: None,
        message: format!("expected one of: {}", known_keywords().join(", ")),
    });
    sink.push(Diagnostic {
        code: DiagnosticCode::UnknownKeyword,
        severity: DiagnosticCode::UnknownKeyword.severity(),
        span: span.clone(),
        primary: format!("unknown keyword `{keyword}`"),
        notes,
    });
}
