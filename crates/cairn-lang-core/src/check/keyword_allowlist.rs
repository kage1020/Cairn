//! `keyword_allowlist` pass — flags every member whose role lowered to
//! [`MemberRole::Other`].
//!
//! Walks the Intent IR rather than the AST because the role classification
//! already happened during lowering; `MemberRole::Other(kw)` is the single
//! source of truth for "this keyword wasn't in the M2 table".

use crate::intent::{IntentModule, Member, MemberRole, known_keywords};

use super::{Diagnostic, DiagnosticCode, DiagnosticSink};

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
}

fn walk(members: &[Member], sink: &mut DiagnosticSink) {
    for m in members {
        if let MemberRole::Other(kw) = &m.role {
            sink.push(Diagnostic {
                code: DiagnosticCode::UnknownKeyword,
                severity: DiagnosticCode::UnknownKeyword.severity(),
                span: m.span.clone(),
                primary: format!("unknown keyword `{kw}`"),
                notes: vec![super::DiagnosticNote {
                    span: m.span.clone(),
                    message: format!("expected one of: {}", known_keywords().join(", ")),
                }],
            });
        }
        walk(&m.children.members, sink);
    }
}
