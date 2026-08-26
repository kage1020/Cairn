//! `arguments` pass — flags every `key=value` whose key is outside the
//! vocabulary of the member's role, and every key in that vocabulary no
//! pass reads yet.
//!
//! Walks the Intent IR beside [`super::keyword_allowlist`], which asks the
//! same question one level up. The two do not both fire on a line: a
//! member whose keyword is unknown has no vocabulary to judge its
//! arguments against, so this pass leaves it alone and the keyword's own
//! finding carries the repair.
//!
//! Only `intent_state` is in scope. A member's selector (`door[id=front]`),
//! its `-> value` tail and its positionals are separate fields with checks
//! of their own, and each answers to a different vocabulary.
//!
//! The candidate set is the role's, plus the universal keys. `clas=outer`
//! is the case that needs the second half: `class` is hoisted into a
//! dedicated field only when the value is label-shaped, so the typo never
//! reaches the field, and a suggestion drawn from the role's own arguments
//! could not offer the word the author meant.

use crate::intent::{IntentModule, Member, MemberRole};
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
}

fn walk(members: &[Member], sink: &mut DiagnosticSink) {
    for m in members {
        check_member(m, sink);
        walk(&m.children.members, sink);
    }
}

fn check_member(member: &Member, sink: &mut DiagnosticSink) {
    // The keyword is the repair; its arguments answer to a vocabulary that
    // does not exist.
    if matches!(member.role, MemberRole::Other(_)) {
        return;
    }
    let accepted = member.role.accepted_arguments();
    for (key, value) in &member.intent_state.fields {
        if !accepted.contains(&key.as_str()) {
            sink.push(unknown_argument(member, key, &value.span, &accepted));
        } else if member.role.unread_arguments().contains(&key.as_str()) {
            sink.push(unread_argument(member, key, &value.span));
        }
    }
}

fn unknown_argument(
    member: &Member,
    key: &str,
    span: &crate::error::Span,
    accepted: &[&'static str],
) -> Diagnostic {
    // Suggestion first, closed set second — the same order
    // `E_UNKNOWN_KEYWORD` uses, so a reader who has seen one knows where
    // to look in the other.
    let mut notes = Vec::with_capacity(2);
    if let Some(suggested) = nearest_match(key, accepted.iter().copied()) {
        notes.push(DiagnosticNote {
            span: None,
            message: format!("did you mean `{suggested}`?"),
        });
    }
    notes.push(DiagnosticNote {
        span: None,
        message: format!("expected one of: {}", accepted.join(", ")),
    });
    Diagnostic {
        code: DiagnosticCode::UnknownArgument,
        span: span.clone(),
        primary: format!(
            "`{key}=` is not an argument `{}` reads",
            role_keyword(&member.role),
        ),
        notes,
        data: None,
    }
}

fn unread_argument(member: &Member, key: &str, span: &crate::error::Span) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::IgnoredArgument,
        span: span.clone(),
        primary: format!(
            "`{key}=` is an argument `{}` takes and no pass reads yet; the value was ignored",
            role_keyword(&member.role),
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "the member is built without it — remove the argument, or keep it and \
                      expect no effect until the lowering rule lands"
                .to_owned(),
        }],
        data: None,
    }
}

/// The surface keyword a role is written as, for a message that has to name
/// it back to the author.
fn role_keyword(role: &MemberRole) -> &str {
    match role {
        MemberRole::Floor => "floor",
        MemberRole::Walls => "walls",
        MemberRole::Door => "door",
        MemberRole::Window => "window",
        MemberRole::Roof => "roof",
        MemberRole::Stair => "stair",
        MemberRole::Level => "level",
        MemberRole::PressurePlate => "pressure_plate",
        MemberRole::Circuit => "circuit",
        MemberRole::Place => "place",
        MemberRole::Connect => "connect",
        MemberRole::Other(kw) => kw,
    }
}
