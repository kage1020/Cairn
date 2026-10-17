//! The `member_scope` pass: a known keyword the enclosing body has no reader for, which would
//! otherwise fall out of the build without a word. One finding per misplaced subtree, and one per
//! `circuit` line under a `level`, which place-and-route never reads.

use crate::ast::{SIGNAL_HEAD, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::{
    BodyKind, CircuitRegionDefect, IntentModule, Member, MemberRole, circuit_lines_under_level,
};

use super::DiagnosticSink;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for body in ir.bodies() {
        walk(body.members, body.kind, false, sink);
        if body.kind == BodyKind::Geometry {
            report_circuit_lines_under_levels(body.members, sink);
        }
    }
}

/// Keeps the body kind: `flatten_members` splices a `level`'s children into the same buckets.
fn walk(members: &[Member], body: BodyKind, inside_level: bool, sink: &mut DiagnosticSink) {
    for member in members {
        if matches!(member.role, MemberRole::Other(_)) {
            continue;
        }
        if !member.role.is_read_in(body) {
            report(member, body, sink);
        } else if super::nesting::body_reaches_the_build(member, inside_level) {
            walk(&member.children.members, body, true, sink);
        }
    }
}

fn report(member: &Member, body: BodyKind, sink: &mut DiagnosticSink) {
    let keyword = member.role.keyword();
    let mut notes = vec![DiagnosticNote {
        span: None,
        message: advice(&member.role, body).to_owned(),
    }];
    let nested = subtree_size(member);
    if nested > 0 {
        notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "the {nested} {subject} indented under it {verb} with it",
                subject = if nested == 1 { "member" } else { "members" },
                verb = if nested == 1 { "goes" } else { "go" },
            ),
        });
    }
    if carries_signal(member) {
        notes.push(DiagnosticNote {
            span: None,
            message: "the `sig.` reference on this line does still reach redstone synthesis — but the block it would sense or drive is the one being dropped, so move the member and the signal keeps working".to_owned(),
        });
    }
    notes.push(DiagnosticNote {
        span: None,
        message: format!("expected one of: {}", body.allowed_keywords().join(", ")),
    });
    sink.push(Diagnostic {
        code: DiagnosticCode::MisplacedMember,
        span: member.span.clone(),
        primary: format!(
            "nothing in a {body} body lowers `{keyword}`: this member produces no blocks, no placement, and no walkway",
            body = body.describe(),
        ),
        notes,
        data: None,
    });
}

/// The line a scope's reservation comes from, which a `circuit` line under a `level` defers to.
#[derive(Clone, Copy)]
enum Kept<'a> {
    /// The first `circuit` line among the scope's own members.
    Own(&'a Member),
    /// The first `circuit` line under a `level`, in a scope with none of its own.
    FirstNested(&'a Member),
}

impl Kept<'_> {
    /// The note pointing at the kept line, then the repair for a second line.
    fn notes(self) -> [DiagnosticNote; 2] {
        let (line, here, repair) = match self {
            Kept::Own(line) => (
                line,
                "the scope's own `circuit` line is here",
                "a scope reserves one region for its redstone: delete this line or move it to the scope's top level in place of that one, or split the logic across several scopes, each with its own `circuit` line",
            ),
            Kept::FirstNested(line) => (
                line,
                "the first `circuit` line under a `level` is here",
                "a scope reserves one region for its redstone: move one of the two lines to the scope's top level and delete the other, or split the logic across several scopes, each with its own `circuit` line",
            ),
        };
        [
            DiagnosticNote {
                span: Some(line.span.clone()),
                message: here.to_owned(),
            },
            DiagnosticNote {
                span: None,
                message: repair.to_owned(),
            },
        ]
    }
}

/// Reports each `circuit` line under a `level` among `members`.
fn report_circuit_lines_under_levels(members: &[Member], sink: &mut DiagnosticSink) {
    let mut kept = members
        .iter()
        .find(|m| matches!(m.role, MemberRole::Circuit))
        .map(Kept::Own);
    for line in members
        .iter()
        .filter(|m| matches!(m.role, MemberRole::Level))
        .flat_map(circuit_lines_under_level)
    {
        report_circuit_line_under_level(line, kept, sink);
        kept.get_or_insert(Kept::FirstNested(line));
    }
}

fn report_circuit_line_under_level(
    line: &Member,
    kept: Option<Kept<'_>>,
    sink: &mut DiagnosticSink,
) {
    let mut notes = vec![DiagnosticNote {
        span: None,
        message: "a `circuit` line belongs among the `struct`'s or `def`'s own members: its reservation spans the scope's `size=WxH`, and no pass reads a `level`'s `y=` for it".to_owned(),
    }];
    match kept {
        Some(kept) => notes.extend(kept.notes()),
        None => notes.push(DiagnosticNote {
            span: None,
            message:
                "move it out of the `level` to the scope's top level; nothing else about it changes"
                    .to_owned(),
        }),
    }
    sink.push(Diagnostic {
        code: DiagnosticCode::MisplacedMember,
        span: line.span.clone(),
        primary: format!(
            "this `circuit` line reserves nothing: {}",
            CircuitRegionDefect::NestedUnderLevel,
        ),
        notes,
        data: None,
    });
}

fn subtree_size(member: &Member) -> usize {
    member.children.members.len()
        + member
            .children
            .members
            .iter()
            .map(subtree_size)
            .sum::<usize>()
}

fn carries_signal(member: &Member) -> bool {
    let is_signal = |value: &crate::ast::Value| matches!(&value.kind, ValueKind::DotRef(reference) if reference.head() == SIGNAL_HEAD);
    member.binding.as_ref().is_some_and(&is_signal)
        || member
            .intent_state
            .values()
            .any(|held| is_signal(&held.value))
}

fn advice(role: &MemberRole, body: BodyKind) -> &'static str {
    // Reached only for a role the body cannot read, so `Geometry` here means `place` or `connect`.
    match (body, role) {
        (BodyKind::Geometry, _) => {
            "`place` and `connect` describe a site's layout, so they belong in a `site` body"
        }
        (BodyKind::Site, MemberRole::Level) => {
            "a `site` body has no grouping construct: dedent the rows to the site's own indentation and drop the `level`"
        }
        (BodyKind::Site, _) => {
            "geometry keywords belong in the `struct` or `def` that a `place use=` instantiates"
        }
    }
}
