//! The `edition_guard` pass: a known member other than a raw member or a Tier 0 part, or a `logic`
//! or `assert` line, under an `@edition` guard, since the semantic layer is edition-neutral
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

use crate::ast::{Item, Module, Statement};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::edition::Edition;
use crate::error::Span;
use crate::intent::{MemberRole, role_of};

use super::DiagnosticSink;

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    for item in &module.items {
        match item {
            Item::Struct { body, .. } | Item::Def { body, .. } | Item::Site { body, .. } => {
                walk(body, sink);
            }
            Item::Theme { .. } => {}
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
            keyword, children, ..
        } => match role_of(keyword) {
            // `keyword_allowlist` reports an unknown keyword, offering the ones a guard holds.
            MemberRole::Other(_) => return,
            role if role.is_escape_hatch() => {
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
        Statement::Logic { .. } => "a `logic` line".to_owned(),
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
            "{what} stands under an `@edition {edition}` guard, which holds only raw members and \
             Tier 0 parts: the semantic layer is edition-neutral"
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
                          NAME_bedrock`; the guard is for `raw_block`, `raw_fill`, `raw_repeat` \
                          and the Tier 0 parts `repeater`, `comparator`, `redstone_torch` and \
                          `redstone_dust`"
                    .into(),
            },
        ],
        data: None,
    });
}
