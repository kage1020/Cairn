//! `nesting` pass — flags an indented body that never reaches the build.
//!
//! The surface grammar hangs a body off every command
//! (`parse_optional_command_body` is called for each generic statement),
//! but only one shape has a reader: a `level y=N` sitting directly in a
//! `struct` or `def` body, which [`crate::block_array`]'s
//! `flatten_members` unwraps so the children join the phase buckets.
//! Everything else is dropped before any block is placed — and dropped
//! quietly, which is what this pass exists to stop.
//!
//! Three ways to lose a body, all one code because the author's next
//! action is the same (move the members) and the message says which
//! case they are in:
//!
//! * the parent's role is not a grouping construct at all;
//! * `level` inside another `level`, which the lowering pass refuses;
//! * `level` whose `y=` is missing or not a `u32`, so it has nowhere to
//!   put the children.
//!
//! A fourth shape — `level` in a `site` body, where nothing unwraps it —
//! belongs to [`super::member_scope`] instead: a `site` body has no
//! reader for `level` at all, so the finding is about the row rather
//! than about what is indented under it, and reporting both would bill
//! one mistake twice.
//!
//! The last two also earn a `W_DEFERRED_MEMBER` from block-array
//! lowering. That is not a substitute: `check` does not lower (see the
//! module doc on [`super::check`]), so `cairn check` exited 0 on both
//! until this pass covered them — and a nested `level` in a `def` that
//! no `site` places is never lowered at all, so it had no reporter
//! anywhere.
//!
//! Dropped is not quite inert, which is why the geometry message is
//! about blocks rather than about the member disappearing. A member
//! nested in a `struct` or `def` body still receives a theme binding
//! (`resolve::resolver`'s `resolve_members` recurses) and still
//! contributes sensors, actuators, and `logic` bindings to redstone
//! synthesis (`collect_member` recurses). What it never produces is
//! blocks. In a `site` body neither of those applies:
//! `resolve_site_placements` iterates the rows without descending, so a
//! nested row takes part in nothing.
//!
//! Scope of the walk: `children.members` only. A `logic` binding in the
//! same body is read by redstone synthesis at any depth, so it is not
//! lost. An `assert` is read by nothing at any depth — the evaluator is
//! not implemented — so a nested one is no worse off than a top-level
//! one, and reporting it here would blame the indentation for that.
//!
//! Without this pass the failure was inverted: `check::connect_arity`
//! recurses into children, so a *malformed* nested `connect` earned a
//! position-anchored error while a well-formed one laid no walkway and
//! said nothing at all.
//!
//! Anchoring: the indented members, from the first to the last byte of
//! the last one's own subtree, so the underline reaches the members
//! that have to move rather than stopping at the last one's header
//! line. `logic` / `assert` lines interleaved in the same body fall
//! inside that range without being counted — they are part of the block
//! the author will re-indent, but not part of what is lost.
//!
//! One diagnostic per dropped subtree, reported at its root. Recursing
//! into a body that is already dropped would count one mistake once per
//! member inside it.

use crate::error::Span;
use crate::intent::{BodyKind, IntentModule, Member, MemberRole};

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

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
    for s in &ir.structs {
        walk(&s.members, BodyKind::Geometry, false, sink);
    }
    for d in &ir.defs {
        walk(&d.members, BodyKind::Geometry, false, sink);
    }
    for s in &ir.sites {
        walk(&s.placements, BodyKind::Site, false, sink);
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
        // An unknown keyword is `keyword_allowlist`'s finding, and a
        // keyword this body has no reader for is `member_scope`'s. Both
        // repairs are the keyword line, not the indentation — "move
        // these into a `level`" would send the author to fix the wrong
        // line, and in a `site` body there is no `level` to move them
        // into. The subtree under either is skipped rather than walked:
        // it hangs off a row that does not name anything the compiler
        // builds here.
        if matches!(member.role, MemberRole::Other(_)) || !member.role.is_read_in(scope) {
            continue;
        }
        match loss(member, inside_level) {
            Some(loss) => report(member, first, last, loss, scope, sink),
            None => walk(&member.children.members, scope, true, sink),
        }
    }
}

/// Whether this member's indented body reaches the build.
///
/// [`super::member_scope`] asks before recursing, for the same reason
/// this pass asks before recursing: a body about to be reported as lost
/// takes its children with it, so a second finding on one of those
/// children bills one mistake twice — and with repairs that disagree,
/// since following "dedent these" leaves the other finding standing.
///
/// Only meaningful for a role the enclosing body reads; both callers
/// check that first.
pub(super) fn body_reaches_the_build(member: &Member, inside_level: bool) -> bool {
    loss(member, inside_level).is_none()
}

/// How this member's indented body is lost, or `None` when it reaches
/// the build.
///
/// Only reached for a role the enclosing body reads — `walk` hands the
/// rest to `member_scope` — so a `Level` here is always in a `struct` or
/// `def`, where `flatten_members` is the thing that would unwrap it.
fn loss(member: &Member, inside_level: bool) -> Option<Loss> {
    if !matches!(member.role, MemberRole::Level) {
        return Some(Loss::NotAGroupingConstruct);
    }
    if inside_level {
        return Some(Loss::LevelInsideLevel);
    }
    // Same reader as `flatten_members`, through the same accessor, so
    // "the offset this pass accepted" and "the offset lowering placed
    // the children at" cannot drift apart.
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

/// Last byte of `member` including everything indented under it.
///
/// [`Member::span`] covers the member's own line only, so a dropped
/// subtree would otherwise be underlined down to its root's header and
/// stop there, leaving the grandchildren the author has to move outside
/// the range.
fn subtree_end(member: &Member) -> usize {
    member
        .children
        .members
        .last()
        .map_or(member.span.end, |last| {
            subtree_end(last).max(member.span.end)
        })
}
