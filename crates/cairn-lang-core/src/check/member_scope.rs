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
//! * `logic` / `assert` lines, which are not members: `intent::lower` sorts
//!   them into [`crate::intent::MemberBody`]'s own fields, and redstone
//!   synthesis reads them from either body.
//!
//! One diagnostic per misplaced subtree, reported at its root. A member the
//! body cannot read takes its children with it, so walking into them would
//! count one mistake once per line underneath it.

use crate::intent::{BodyKind, IntentModule, Member, MemberRole};

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for s in &ir.structs {
        walk(&s.members, BodyKind::Geometry, sink);
    }
    for d in &ir.defs {
        walk(&d.members, BodyKind::Geometry, sink);
    }
    for s in &ir.sites {
        walk(&s.placements, BodyKind::Site, sink);
    }
}

/// Recurse with the body kind unchanged: nesting does not open a new body.
/// A `level y=N` groups members inside the geometry body it already sits
/// in — `flatten_members` splices its children into the same phase buckets
/// — so a `place` indented under one is exactly as unreadable as a `place`
/// written at the top of the `struct`.
fn walk(members: &[Member], body: BodyKind, sink: &mut DiagnosticSink) {
    for member in members {
        if matches!(member.role, MemberRole::Other(_)) {
            continue;
        }
        if member.role.is_read_in(body) {
            walk(&member.children.members, body, sink);
        } else {
            report(member, body, sink);
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
    let nested = member.children.members.len();
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
    notes.push(DiagnosticNote {
        span: None,
        message: format!("expected one of: {}", body.allowed_keywords().join(", ")),
    });
    sink.push(Diagnostic {
        code: DiagnosticCode::MisplacedMember,
        severity: DiagnosticCode::MisplacedMember.severity(),
        span: member.span.clone(),
        // Deliberately not "produces no blocks": the geometry half of this
        // code reports a misplaced `connect`, and what that member would
        // have contributed is a walkway. One sentence covers both halves.
        primary: format!(
            "nothing in a {body} body reads `{keyword}`, so this member reaches no part of the build",
            body = body.describe(),
        ),
        notes,
        data: None,
    });
}

/// Where the member the author wrote does belong.
///
/// `level` in a `site` body is called out separately because the generic
/// site advice would misdirect: its children are usually `place` rows that
/// belong exactly where they are, one indent level to the left, and telling
/// the author to move them into a `struct` would cost them the placement.
fn advice(role: &MemberRole, body: BodyKind) -> &'static str {
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
