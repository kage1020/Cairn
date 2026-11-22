//! The `edition_guard` pass: a known member outside the escape-hatch layer, or a `logic` or
//! `assert` line, under an `@edition` guard, since the semantic layer is edition-neutral, and an
//! `nbt={...}` compound no guard holds, since it is edition-specific
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

use crate::ast::{Item, Module, Statement, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::edition::Edition;
use crate::error::Span;
use crate::intent::{BodyKind, IntentModule, Member, MemberRole, is_escape_hatch_line, role_of};

use super::DiagnosticSink;

pub(super) fn run(module: &Module, ir: &IntentModule, sink: &mut DiagnosticSink) {
    for item in &module.items {
        match item {
            Item::Struct { body, .. } | Item::Def { body, .. } | Item::Site { body, .. } => {
                walk(body, sink);
            }
            Item::Theme { .. } => {}
        }
    }
    for body in ir.bodies() {
        unguarded_nbt(body.members, body.kind, false, sink);
    }
}

/// One `E_NBT_OUTSIDE_EDITION_GUARD` per `nbt={...}` compound a member of a role that reads one
/// writes outside every guard; a role that reads none gets `E_UNKNOWN_ARGUMENT` instead.
fn unguarded_nbt(
    members: &[Member],
    body: BodyKind,
    inside_level: bool,
    sink: &mut DiagnosticSink,
) {
    for member in members {
        // `member_scope::walk`'s gates: such a row is already reported, subtree and all.
        if member.edition_guard.is_some()
            || matches!(member.role, MemberRole::Other(_))
            || !member.role.is_read_in(body)
        {
            continue;
        }
        if member.role.reads_nbt()
            && let Some(nbt) = member.written("nbt")
            && matches!(nbt.value.kind, ValueKind::Nbt(_))
        {
            let keyword = member.role.keyword();
            sink.push(Diagnostic {
                code: DiagnosticCode::NbtOutsideEditionGuard,
                span: nbt.span.clone(),
                primary: format!(
                    "`{keyword}` writes an `nbt={{...}}` compound outside an `@edition` guard: the \
                     compound spells one edition's field names, and the semantic layer is \
                     edition-neutral"
                ),
                notes: vec![DiagnosticNote {
                    span: None,
                    message: "indent the line under `@edition java` or `@edition bedrock`, the \
                              edition whose field names the compound spells, or drop `nbt=`"
                        .into(),
                }],
                data: None,
            });
        }
        if super::nesting::body_reaches_the_build(member, inside_level) {
            for child in &member.children.members {
                if super::nesting::child_reaches_the_build(member, child) {
                    unguarded_nbt(std::slice::from_ref(child), body, true, sink);
                }
            }
        }
    }
}

fn walk(body: &[Statement], sink: &mut DiagnosticSink) {
    for statement in body {
        match statement {
            Statement::EditionGuard {
                edition,
                children,
                span,
            } => {
                for line in children {
                    check_guarded(line, *edition, span, sink);
                }
            }
            Statement::Generic { children, .. } => walk(children, sink),
            Statement::Logic { .. }
            | Statement::UnboundLogic { .. }
            | Statement::AssertTruth { .. }
            | Statement::AssertAlways { .. }
            | Statement::AssertLatency { .. } => {}
        }
    }
}

/// One finding per refused line, whatever is indented under it.
fn check_guarded(line: &Statement, edition: Edition, guard: &Span, sink: &mut DiagnosticSink) {
    let what = match line {
        Statement::Generic {
            keyword,
            args,
            children,
            ..
        } => match role_of(keyword) {
            // `keyword_allowlist` reports an unknown keyword, offering the ones a guard holds.
            MemberRole::Other(_) => return,
            role if is_escape_hatch_line(&role, args) => {
                // `nesting` judges a member under one; a nested guard's lines answer to that guard.
                for child in children {
                    match child {
                        Statement::Generic { .. } => {}
                        Statement::EditionGuard {
                            edition,
                            children,
                            span,
                        } => {
                            for line in children {
                                check_guarded(line, *edition, span, sink);
                            }
                        }
                        logic_or_assert => check_guarded(logic_or_assert, edition, guard, sink),
                    }
                }
                return;
            }
            _ => format!("a `{keyword}`"),
        },
        Statement::Logic { .. } | Statement::UnboundLogic { .. } => "a `logic` line".to_owned(),
        Statement::AssertTruth { .. }
        | Statement::AssertAlways { .. }
        | Statement::AssertLatency { .. } => "an `assert` line".to_owned(),
        // The parser refuses a guard directly under a guard.
        Statement::EditionGuard { .. } => return,
    };
    sink.push(Diagnostic {
        code: DiagnosticCode::SemanticInEditionGuard,
        span: line.span().clone(),
        primary: format!(
            "{what} stands under an `@edition {edition}` guard, which holds only the escape-hatch \
             layer: the semantic layer is edition-neutral"
        ),
        notes: vec![
            DiagnosticNote {
                span: Some(guard.clone()),
                message: "the guard opens here".into(),
            },
            DiagnosticNote {
                span: None,
                message: "dedent the line out of the guard so that every edition builds it, and \
                          bind what differs per edition from a `theme NAME_java` and a `theme \
                          NAME_bedrock`; the guard is for `raw_block`, `raw_fill`, `raw_repeat`, \
                          `spawn`, the Tier 0 parts `repeater`, `comparator`, `redstone_torch` \
                          and `redstone_dust`, and an entity's line that writes `nbt={...}`"
                    .into(),
            },
        ],
        data: None,
    });
}
