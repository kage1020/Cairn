//! `nesting` pass: flags an indented body that never reaches the build.
//!
//! The parser hangs a body off every command, but only a `level y=N`
//! directly in a `struct` or `def` body has a reader:
//! [`crate::block_array`]'s `flatten_members` unwraps it into the phase
//! buckets. Every other body is dropped before any block is placed, in one
//! of three ways that share a code because the repair is the same (move the
//! members):
//!
//! * the parent's role is not a grouping construct;
//! * `level` inside another `level`, which lowering refuses;
//! * `level` whose `y=` is missing or not a `u32`.
//!
//! A `level` in a `site` body is [`super::member_scope`]'s, since that body
//! has no reader for the row at all. The last two shapes also earn
//! lowering's `W_DEFERRED_MEMBER`, but `check` does not lower, and a `def`
//! no `site` places is never lowered.
//!
//! A dropped member in a `struct` or `def` still takes a theme binding
//! (`resolve::resolver`'s `resolve_members` recurses) and still feeds
//! redstone synthesis (`collect_member` recurses); it only produces no
//! blocks, so the message is about blocks. In a `site` body
//! `resolve_site_placements` does not descend, so a nested row takes part
//! in nothing.
//!
//! Only `children.members` are walked: `logic` and `assert` lines are read
//! at any depth ([`super::truth`] and redstone synthesis walk nested
//! bodies), so indentation loses nothing of theirs.
//!
//! The underline runs from the first indented member to the end of the last
//! one's subtree, covering every line the author has to move; interleaved
//! `logic` / `assert` lines fall inside it without being counted. One
//! diagnostic per dropped subtree, at its root.

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{BodyKind, IntentModule, Member, MemberRole};

use super::DiagnosticSink;

/// Why an indented body will not reach the build.
#[derive(Debug, Clone, Copy)]
enum Loss {
    /// The parent's role is not a grouping construct.
    NotAGroupingConstruct,
    /// `level` inside another `level`.
    LevelInsideLevel,
    /// `level` with no usable `y=` to place its children at.
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
        // `keyword_allowlist` and `member_scope` own these rows, and their
        // repair is the keyword line, not the indentation.
        if matches!(member.role, MemberRole::Other(_)) || !member.role.is_read_in(scope) {
            continue;
        }
        match loss(member, inside_level) {
            Some(loss) => report(member, first, last, loss, scope, sink),
            None => walk(&member.children.members, scope, true, sink),
        }
    }
}

/// Whether this member's indented body reaches the build, so another walk
/// does not report a child of a body this pass reports as lost.
///
/// Only meaningful for a role the enclosing body reads.
pub(super) fn body_reaches_the_build(member: &Member, inside_level: bool) -> bool {
    loss(member, inside_level).is_none()
}

/// How this member's indented body is lost, or `None` when it reaches
/// the build.
///
/// The caller has checked that the enclosing body reads the role, so a
/// `Level` here is in a `struct` or `def`.
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

/// Last byte of `member` including everything indented under it;
/// [`Member::span`] covers the member's own line only.
fn subtree_end(member: &Member) -> usize {
    member
        .children
        .members
        .last()
        .map_or(member.span.end, |last| {
            subtree_end(last).max(member.span.end)
        })
}
