//! `member_scope` pass — flags a known keyword the enclosing body has no
//! reader for.
//!
//! [`crate::intent::role_of`] is one global table, and nothing downstream
//! restores the per-body distinction: `block_array`'s phase buckets take the
//! geometry roles and `resolve_site_placements` takes `place` and
//! `connect`, so a member in the wrong body falls off both without a
//! diagnostic. The legality lives on [`MemberRole::is_read_in`], beside
//! [`MemberRole::keyword`], because `check::nesting` needs the same split.
//!
//! [`MemberRole::Other`] is `keyword_allowlist`'s, and `logic` / `assert`
//! lines never reach this walk: `intent::lower` sorts them into
//! [`crate::intent::MemberBody`]'s own fields.
//!
//! "Produces nothing" is about the block array only. `redstone::synth`
//! walks a site's rows on purpose, so a `sig.` reference on a misplaced row
//! does reach synthesis, and the message says so. It is still an error,
//! because the block the signal would sense or drive is the one dropped; a
//! site hosting fixtures needs lowering support, not a checker exemption.
//!
//! One diagnostic per misplaced subtree, at its root, and the walk does not
//! descend into a body [`super::nesting`] reports as lost: a child finding
//! would count one mistake once per line under it.

use crate::ast::{SIGNAL_HEAD, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::{BodyKind, IntentModule, Member, MemberRole};

use super::DiagnosticSink;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for body in ir.bodies() {
        walk(body.members, body.kind, false, sink);
    }
}

/// Recurse with the body kind unchanged: `flatten_members` splices a
/// `level`'s children into the same phase buckets, so a `place` under one
/// is as unreadable as one at the top. `inside_level` only tells
/// [`super::nesting`] how deep the walk is.
///
/// Stops before a member's children when this pass or `nesting` reports
/// the row, since that finding covers every line below.
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
    // `nesting` stays quiet under this row, so the cost is counted here,
    // over the whole subtree.
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
        // Scoped to the block array, since a signal-bearing row still
        // reaches redstone synthesis. The nouns cover geometry, `place` and
        // `connect`.
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
/// Looser than `redstone::synth`'s test, which keys actuators on a closed
/// list of argument names: the note only softens a claim and excuses
/// nothing.
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
/// A `level` in a `site` body gets its own advice: its children are usually
/// `place` rows that belong one indent to the left, not in a `struct`.
fn advice(role: &MemberRole, body: BodyKind) -> &'static str {
    // Reached only for a role the body cannot read, so the geometry arm is
    // `place` / `connect`. No wildcard on `body`, so a new `BodyKind` has
    // to answer here too.
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
