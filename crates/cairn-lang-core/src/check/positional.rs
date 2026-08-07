//! `positional` pass — flags bare values on a line that reads none.
//!
//! `spec/syntax.md` §5.1 requires `key=value` for everything after the
//! command keyword, and gives the reason: positional arguments make the
//! author (or the model) remember an order, and a dropped or reordered one
//! is invisible. `connect FROM.PORT to TO.PORT` is the single form that
//! reads positionals, and `check::connect_arity` owns its shape.
//!
//! Nothing enforced the rule. The parser fills `positional` with any token
//! on the line that is not `key=`, `-> binding`, or `[selector]` — so a
//! dropped `=` (`walls height 3`) and the spec's own forbidden example
//! (`window front G 2 2 2x2`) both parse, classify to a real role, and
//! lower with the bare values ignored. What the author gets is the member
//! they wrote minus the arguments they meant, with `cairn check` reporting
//! nothing.
//!
//! Walks every member at every depth. Where a line sits does not change
//! whether its own values are read, so unlike `member_scope` and `nesting`
//! this pass keeps descending through a subtree those two have already
//! reported: dedenting a `window front G 2 2 2x2` leaves it just as
//! broken.
//!
//! [`MemberRole::Other`] is skipped for the reason `keyword_allowlist`
//! gives — the keyword is not in the table, so the repair is the word, and
//! there is no reader whose argument form the bare values could be
//! measured against.

use crate::ast::Value;
use crate::error::Span;
use crate::intent::{IntentModule, Member, MemberRole};

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

/// `first` / `last` are the ends of the run, not necessarily a prefix of
/// the line: the parser appends to `positional` whenever the next token is
/// not `key=`, so `size=2 x2` leaves one bare value *after* an argument.
/// Spanning first-to-last covers the run either way, and the arguments
/// caught in between are part of the line the author rewrites.
fn report(member: &Member, first: &Value, last: &Value, sink: &mut DiagnosticSink) {
    let keyword = member.role.keyword();
    let count = member.positional.len();
    sink.push(Diagnostic {
        code: DiagnosticCode::UnexpectedPositional,
        severity: DiagnosticCode::UnexpectedPositional.severity(),
        span: Span {
            start: first.span.start,
            end: last.span.end,
        },
        primary: format!(
            "`{keyword}` reads only `key=value` arguments: the {count} bare {value} on this line {verb} dropped",
            value = if count == 1 { "value" } else { "values" },
            verb = if count == 1 { "is" } else { "are" },
        ),
        // No "declared here" note: the keyword and the run sit on the same
        // line, so a second location would point where the underline
        // already is.
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
