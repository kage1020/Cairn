//! The `positional` pass: bare values on a line that reads none (`spec/syntax` "Lexical"). It
//! descends through subtrees `member_scope` and `nesting` report, since dedenting a line does not
//! make its values read.

use crate::ast::Value;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{IntentModule, Member, MemberRole};

use super::DiagnosticSink;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for body in ir.bodies() {
        walk(body.members, sink);
    }
}

fn walk(members: &[Member], sink: &mut DiagnosticSink) {
    for member in members {
        if !matches!(member.role, MemberRole::Other(_) | MemberRole::Connect)
            && let (Some(first), Some(last)) = (member.positional.first(), member.positional.last())
        {
            report(member, first, last, sink);
        }
        walk(&member.children.members, sink);
    }
}

fn report(member: &Member, first: &Value, last: &Value, sink: &mut DiagnosticSink) {
    let keyword = member.role.keyword();
    let count = member.positional.len();
    sink.push(Diagnostic {
        code: DiagnosticCode::UnexpectedPositional,
        span: Span {
            start: first.span.start,
            end: last.span.end,
        },
        primary: format!(
            "`{keyword}` reads only `key=value` arguments: the {count} bare {value} on this line {verb} dropped",
            value = if count == 1 { "value" } else { "values" },
            verb = if count == 1 { "is" } else { "are" },
        ),
        notes: vec![
            DiagnosticNote {
                span: None,
                message: "give each value the key it belongs to (`side=front`, `size=2x2`); a missing `=` lands here too".to_owned(),
            },
            DiagnosticNote {
                span: None,
                message: "`connect FROM.PORT to TO.PORT` is the one statement that reads positional values".to_owned(),
            },
        ],
        data: None,
    });
}
