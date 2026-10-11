//! `positional` pass — flags bare values on a line that reads none.
//!
//! `spec/syntax` "Lexical" requires `key=value` after the command keyword,
//! because an order the author has to remember is invisible when a value is
//! dropped or reordered. The parser still puts any token that is not
//! `key=`, `-> binding` or `[selector]` into `positional`, so `walls height
//! 3` and `window front G 2 2 2x2` lower with the bare values ignored.
//! `connect FROM.PORT to TO.PORT` is the one form that reads positionals,
//! and `check::connect_arity` owns its shape.
//!
//! Unlike `member_scope` and `nesting`, this pass descends through subtrees
//! those two report, since dedenting a line does not make its values read.
//! [`MemberRole::Other`] is skipped: with no role there is no argument form
//! to measure the values against.

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

/// `first` and `last` bound the run, which need not be a prefix of the
/// line: `size=2 x2` leaves a bare value after an argument. Arguments
/// caught between them are part of the line the author rewrites.
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
        // No "declared here" note: the keyword is on the underlined line.
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
