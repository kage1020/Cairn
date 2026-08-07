//! `nesting` pass — flags an indented body that nothing will read.
//!
//! The surface grammar lets any command carry an indented body
//! (`parse_optional_command_body` hangs one off every generic
//! statement), but only one role reads one back: `level y=N` inside a
//! `struct` or `def`, which [`crate::block_array`]'s `flatten_members`
//! unwraps so its children join the phase buckets. For every other
//! member `flatten_members` returns the member itself and never looks at
//! `member.children`, and site resolution walks `site.placements`
//! without descending at all.
//!
//! Dropped is not quite inert, which is why the wording here is about
//! geometry rather than about the member disappearing. A nested member
//! still receives a theme binding — `resolve::resolver`'s
//! `resolve_members` recurses — and still contributes sensors,
//! actuators, and `logic` bindings to redstone synthesis, whose
//! `collect_member` recurses too. What it never produces is blocks: no
//! voxels from a `struct` or `def` body, no placement or walkway from a
//! `site` body.
//!
//! Without this pass the failure was inverted. `check::connect_arity`
//! recurses into children, so a *malformed* nested `connect` earned a
//! position-anchored `E_CONNECT_ARITY`, while a well-formed one laid no
//! walkway and said nothing at all — the more correct the row, the
//! quieter it failed.
//!
//! Anchoring: the run of indented members, first to last, so the
//! underline covers exactly the text that would have to move. The
//! parent is named in the message instead, since it is the line the
//! author reads down from.
//!
//! One diagnostic per dropped subtree, reported at its root. Recursing
//! into a body that is already dropped would count one mistake once per
//! member inside it.

use crate::error::Span;
use crate::intent::{IntentModule, Member, MemberRole};

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

/// Which body a member sits in, and so which nesting it may carry.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// A `struct` or `def` body, where `level y=N` groups members.
    Geometry,
    /// A `site` body, a flat list of `place` and `connect` rows.
    Site,
}

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for s in &ir.structs {
        walk(&s.members, Scope::Geometry, sink);
    }
    for d in &ir.defs {
        walk(&d.members, Scope::Geometry, sink);
    }
    for s in &ir.sites {
        walk(&s.placements, Scope::Site, sink);
    }
}

fn walk(members: &[Member], scope: Scope, sink: &mut DiagnosticSink) {
    for member in members {
        let children = &member.children.members;
        if children.is_empty() {
            continue;
        }
        if groups_members(member, scope) {
            walk(children, scope, sink);
        } else {
            report(member, children, scope, sink);
        }
    }
}

/// Whether this member's indented body reaches the build.
///
/// `level` qualifies only in a geometry body. In a `site` body nothing
/// reads it — `lower_site` iterates the placement rows for `Place` and
/// `Connect` roles and a `Level` is neither — so its children are
/// dropped like any other nested row. A role is only a grouping
/// construct where something groups by it.
fn groups_members(member: &Member, scope: Scope) -> bool {
    scope == Scope::Geometry && matches!(member.role, MemberRole::Level)
}

fn report(member: &Member, children: &[Member], scope: Scope, sink: &mut DiagnosticSink) {
    let keyword = member.role.keyword();
    let count = children.len();
    let plural = if count == 1 { "" } else { "s" };
    let span = Span {
        start: children
            .first()
            .expect("caller checked the body is non-empty")
            .span
            .start,
        end: children
            .last()
            .expect("caller checked the body is non-empty")
            .span
            .end,
    };
    let (primary, advice) = match scope {
        Scope::Geometry => (
            format!(
                "`{keyword}` does not group members: the {count} member{plural} indented under it produce no blocks"
            ),
            "only `level y=N` groups members inside a `struct` or `def` body — move these out to the enclosing body, or into a `level`".to_owned(),
        ),
        Scope::Site => (
            format!(
                "`{keyword}` does not group members: the {count} member{plural} indented under it are not part of the site"
            ),
            "a `site` body is a flat list of `place` and `connect` rows — dedent these to the site's own indentation".to_owned(),
        ),
    };
    sink.push(Diagnostic {
        code: DiagnosticCode::UnsupportedNesting,
        severity: DiagnosticCode::UnsupportedNesting.severity(),
        span,
        primary,
        notes: vec![
            DiagnosticNote {
                span: Some(member.span.clone()),
                message: format!("`{keyword}` declared here"),
            },
            DiagnosticNote {
                span: None,
                message: advice,
            },
        ],
        data: None,
    });
}
