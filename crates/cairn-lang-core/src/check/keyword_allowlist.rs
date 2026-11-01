//! The `keyword_allowlist` pass: a member or theme selector whose keyword is unknown. A member's
//! suggestions come from its own body's keywords, so `plce` in a `struct` is not offered `place`.

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
            // An empty keyword is `intent::lower`'s placeholder, not a word the author wrote.
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
