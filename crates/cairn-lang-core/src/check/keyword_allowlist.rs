//! `keyword_allowlist` pass: flags every member whose role lowered to
//! [`MemberRole::Other`], and every theme selector whose keyword is outside
//! the known-keyword table. Lowering keeps a selector's keyword as a raw
//! string, so this pass classifies it with [`role_of`].
//!
//! A member's candidates are its enclosing body's keywords, not the whole
//! table: suggesting `place` for `plce` in a `struct` body would walk the
//! author into [`super::member_scope`]'s `E_MISPLACED_MEMBER`. A theme
//! selector matches members of every body, so its candidates are the whole
//! table.

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::{BodyKind, IntentModule, Member, MemberRole, known_keywords, role_of};
use crate::suggest::{did_you_mean_note, nearest_match};

use super::DiagnosticSink;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for body in ir.bodies() {
        walk(body.members, body.kind, sink);
    }
    for theme in &ir.themes {
        for rule in &theme.selectors {
            if matches!(role_of(&rule.keyword), MemberRole::Other(_)) && !rule.keyword.is_empty() {
                push_unknown_keyword(&rule.keyword, &rule.span, known_keywords(), sink);
            }
        }
    }
}

fn walk(members: &[Member], body: BodyKind, sink: &mut DiagnosticSink) {
    for m in members {
        if let MemberRole::Other(kw) = &m.role {
            // An empty keyword is `intent::lower`'s synthetic placeholder
            // member, not a word the author wrote.
            if !kw.is_empty() {
                push_unknown_keyword(kw, &m.span, &body.allowed_keywords(), sink);
            }
        }
        walk(&m.children.members, body, sink);
    }
}

fn push_unknown_keyword(
    keyword: &str,
    span: &crate::error::Span,
    candidates: &[&'static str],
    sink: &mut DiagnosticSink,
) {
    // Suggestion first, so the targeted fix reads before the closed set.
    let mut notes = Vec::with_capacity(2);
    notes.extend(nearest_match(keyword, candidates.iter().copied()).map(did_you_mean_note));
    notes.push(DiagnosticNote {
        span: None,
        message: format!("expected one of: {}", candidates.join(", ")),
    });
    sink.push(Diagnostic {
        code: DiagnosticCode::UnknownKeyword,
        span: span.clone(),
        primary: format!("unknown keyword `{keyword}`"),
        notes,
        data: None,
    });
}
