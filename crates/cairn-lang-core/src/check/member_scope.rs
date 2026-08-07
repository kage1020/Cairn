//! `member_scope` pass — flags a known keyword the enclosing body has no
//! reader for.
//!
//! [`crate::intent::role_of`] is a single global table, so every keyword
//! classifies to its role in every body. Nothing downstream restores the
//! distinction: `block_array`'s phase buckets take the geometry roles and
//! `resolve_site_placements` matches `place` and `connect`, so a member
//! written into the wrong body falls off the end of both. In a `struct` or
//! `def` body the misplaced `place` / `connect` at least earns a
//! `W_DEFERRED_MEMBER` from lowering — but `check` does not lower (see the
//! module doc on [`super::check`]), so `cairn check` exited 0 on it. In a
//! `site` body nothing reported it at either stage: the resolver's loop
//! `continue`s past a non-`place` row and the lowering loop does the same,
//! so a `floor` written among the placements produced no voxels and no
//! diagnostic anywhere.
//!
//! The legality itself lives on [`MemberRole::is_read_in`], next to
//! [`MemberRole::keyword`], because `check::nesting` needs the same split
//! and the readers it is derived from are in a third crate module.
//!
//! Two things are deliberately not this pass's business:
//!
//! * [`MemberRole::Other`] — the keyword is not in the table at all, so
//!   `keyword_allowlist` owns it and the repair is the word, not the body.
//! * `logic` / `assert` lines, which are not members at all: `intent::lower`
//!   sorts them into [`crate::intent::MemberBody`]'s own fields, so they
//!   never reach this walk. (Whether anything reads them is a separate
//!   question, and the answer differs — redstone synthesis takes `logic`
//!   from either body; nothing reads `assert` yet.)
//!
//! "Reaches nothing" is about the block array, and only about that.
//! `redstone::synth`'s `collect_member` is role-agnostic and its
//! `lower_site` walks a site's rows on purpose, so a `-> sig.NAME` tail or
//! a `sig.` argument on a misplaced row *does* reach synthesis today — the
//! message says so when one is present rather than overclaiming. It is
//! still an error: the sensor or actuator would have nothing physical
//! behind it, because the member that builds the block is the one being
//! dropped. A site that hosts fixtures directly needs lowering support,
//! not a checker exemption.
//!
//! One diagnostic per misplaced subtree, reported at its root. A member the
//! body cannot read takes its children with it, so walking into them would
//! count one mistake once per line underneath it — and the same holds for
//! a member whose indented body [`super::nesting`] is about to report as
//! lost, which is why the walk asks that pass before descending.

use crate::ast::{SIGNAL_HEAD, ValueKind};
use crate::intent::{BodyKind, IntentModule, Member, MemberRole};

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

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

/// Recurse with the body kind unchanged: nesting does not open a new body.
/// A `level y=N` groups members inside the geometry body it already sits
/// in — `flatten_members` splices its children into the same phase buckets
/// — so a `place` indented under one is exactly as unreadable as a `place`
/// written at the top of the `struct`. `inside_level` therefore says only
/// how far down [`super::nesting`] is, not which vocabulary applies.
///
/// Two reasons to stop before a member's children, and they are the same
/// reason stated from two sides: this pass owns the row itself, or
/// `nesting` owns the whole indented body under it. Either way one
/// finding already covers every line below, and a second one on a child
/// would send the author to a different fix.
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
    // Everything indented under a row this body cannot read goes with it,
    // and `nesting` stays quiet about those lines precisely because this
    // finding covers them — so the count has to be said here or nowhere.
    // Counted over the whole subtree: a three-deep run loses three members,
    // and "the 1 member indented under it" would understate the cost by the
    // amount that matters most.
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
        // Scoped to the block array on purpose. "Reaches no part of the
        // build" would be false for a signal-bearing row, which redstone
        // synthesis picks up from either body — the note above says so.
        // The three nouns cover every role: geometry members lay blocks,
        // `place` produces a placement, `connect` a walkway.
        primary: format!(
            "nothing in a {body} body lowers `{keyword}`: this member produces no blocks, no placement, and no walkway",
            body = body.describe(),
        ),
        notes,
        data: None,
    });
}

/// Members indented under `member`, at every depth.
fn subtree_size(member: &Member) -> usize {
    member.children.members.len()
        + member
            .children
            .members
            .iter()
            .map(subtree_size)
            .sum::<usize>()
}

/// Whether this line mentions a redstone signal.
///
/// Deliberately looser than `redstone::synth`'s own test, which keys the
/// actuator side on a closed list of argument names: this is prose, and
/// naming `sig.` where the author wrote `sig.` is both true and enough. A
/// value that synthesis would ignore still costs nothing here — the note
/// only softens a claim, it does not excuse the member.
fn carries_signal(member: &Member) -> bool {
    let is_signal = |value: &crate::ast::Value| matches!(&value.kind, ValueKind::DotRef(reference) if reference.head() == SIGNAL_HEAD);
    member.binding.as_ref().is_some_and(&is_signal)
        || member
            .intent_state
            .values()
            .any(|held| is_signal(&held.value))
}

/// Where the member the author wrote does belong.
///
/// `level` in a `site` body is called out separately because the generic
/// site advice would misdirect: its children are usually `place` rows that
/// belong exactly where they are, one indent level to the left, and telling
/// the author to move them into a `struct` would cost them the placement.
fn advice(role: &MemberRole, body: BodyKind) -> &'static str {
    // Only reached for a role the body cannot read, so the geometry arm is
    // `place` / `connect` and nothing else. Named rather than wildcarded so
    // a third `BodyKind` has to answer here too.
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
