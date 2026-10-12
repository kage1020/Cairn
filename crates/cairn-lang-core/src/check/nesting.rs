//! The `nesting` pass: an indented body that never reaches the build. Only a `level y=N` directly
//! in a `struct` or `def` body has a reader; every other body is dropped before any block lands.

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{BodyKind, IntentModule, Member, MemberRole};

use super::DiagnosticSink;

/// Why an indented body will not reach the build.
#[derive(Debug, Clone, Copy)]
enum Loss {
    NotAGroupingConstruct,
    LevelInsideLevel,
    LevelWithoutOffset,
}

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for body in ir.bodies() {
        walk(body.members, body.kind, false, sink);
    }
}

fn walk(members: &[Member], scope: BodyKind, inside_level: bool, sink: &mut DiagnosticSink) {
    for member in members {
        let (Some(first), Some(last)) = (
            member.children.members.first(),
            member.children.members.last(),
        ) else {
            continue;
        };
        // `keyword_allowlist` and `member_scope` own these rows.
        if matches!(member.role, MemberRole::Other(_)) || !member.role.is_read_in(scope) {
            continue;
        }
        match loss(member, inside_level) {
            Some(loss) => report(member, first, last, loss, scope, sink),
            None => walk(&member.children.members, scope, true, sink),
        }
    }
}

/// Whether this member's indented body reaches the build, for a role its body reads.
pub(super) fn body_reaches_the_build(member: &Member, inside_level: bool) -> bool {
    loss(member, inside_level).is_none()
}

fn loss(member: &Member, inside_level: bool) -> Option<Loss> {
    if !matches!(member.role, MemberRole::Level) {
        return Some(Loss::NotAGroupingConstruct);
    }
    if inside_level {
        return Some(Loss::LevelInsideLevel);
    }
    // The accessor `flatten_members` uses, so the two agree on the offset.
    if member.nonneg_u32("y").is_none() {
        return Some(Loss::LevelWithoutOffset);
    }
    None
}

fn report(
    member: &Member,
    first: &Member,
    last: &Member,
    loss: Loss,
    scope: BodyKind,
    sink: &mut DiagnosticSink,
) {
    let keyword = member.role.keyword();
    let count = member.children.members.len();
    let subject = if count == 1 { "member" } else { "members" };
    let span = Span {
        start: first.span.start,
        end: subtree_end(last),
    };
    let (primary, advice) = match (loss, scope) {
        (Loss::NotAGroupingConstruct, BodyKind::Site) => (
            format!(
                "`{keyword}` does not group members: the {count} {subject} indented under it {} not part of the site",
                if count == 1 { "is" } else { "are" },
            ),
            "a `site` body is a flat list of `place` and `connect` rows — dedent these to the site's own indentation".to_owned(),
        ),
        (Loss::NotAGroupingConstruct, BodyKind::Geometry) => (
            geometry_primary(&format!("`{keyword}` does not group members"), count, subject),
            "only `level y=N` groups members inside a `struct` or `def` body — move these out to the enclosing body, or into a `level`".to_owned(),
        ),
        (Loss::LevelInsideLevel, _) => (
            geometry_primary("a `level` inside another `level` does not group members", count, subject),
            "flatten the two levels into one, or give this level's members their own `level y=N` in the enclosing body".to_owned(),
        ),
        (Loss::LevelWithoutOffset, _) => (
            geometry_primary("`level` has no `y=` offset to place its children at", count, subject),
            "add `y=N`, where N is a non-negative integer height above the body's floor".to_owned(),
        ),
    };
    let mut notes = vec![
        DiagnosticNote {
            span: Some(member.span.clone()),
            message: format!("`{keyword}` declared here"),
        },
        DiagnosticNote {
            span: None,
            message: advice,
        },
    ];
    if scope == BodyKind::Geometry {
        notes.push(DiagnosticNote {
            span: None,
            message: "these members are not gone: they still take a theme binding, and any `-> sig.NAME` or `logic` line still reaches redstone synthesis. What they do not produce is blocks".to_owned(),
        });
    }
    sink.push(Diagnostic {
        code: DiagnosticCode::UnsupportedNesting,
        span,
        primary,
        notes,
        data: None,
    });
}

fn geometry_primary(reason: &str, count: usize, subject: &str) -> String {
    format!(
        "{reason}: the {count} {subject} indented under it {} no blocks",
        if count == 1 { "produces" } else { "produce" },
    )
}

/// Last byte of `member`'s subtree; [`Member::span`] covers its own line only.
fn subtree_end(member: &Member) -> usize {
    member
        .children
        .members
        .last()
        .map_or(member.span.end, |last| {
            subtree_end(last).max(member.span.end)
        })
}
