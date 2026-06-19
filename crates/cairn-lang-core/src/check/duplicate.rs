//! `duplicate` pass — flags `key=` / `slot` / `id=` repeated in the same
//! scope.
//!
//! Walks the surface AST rather than the IR because the IR's
//! [`IntentState`](crate::intent::IntentState) and `args` maps are
//! last-write-wins; the surface form is the only place where both the first
//! and second occurrences are still visible.
//!
//! The four codes emitted here use distinct scopes:
//! - `E_DUPLICATE_SIZE` — a struct/def header has more than one `size=`.
//! - `E_DUPLICATE_SLOT` — a `theme` body has two `slot NAME ->` lines for
//!   the same `NAME`.
//! - `E_DUPLICATE_ARG`  — any other duplicate `key=` inside the same
//!   argument list (header excluding `size=`, statement args, selector
//!   attrs, selector bindings, header args of struct/def excluding size).
//! - `E_DUPLICATE_ID`   — two members in the same immediate body scope
//!   declare `id=NAME` for the same `NAME` (per-body scope; nested `level`
//!   blocks have their own namespace).

use indexmap::IndexMap;

use crate::ast::{Arg, Item, Module, Statement, ThemeRule, ValueKind};
use crate::error::Span;

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    for item in &module.items {
        match item {
            Item::Theme { body, .. } => check_theme_body(body, sink),
            Item::Def { args, body, .. } | Item::Struct { args, body, .. } => {
                check_header_args(args, sink);
                check_body(body, sink);
            }
            Item::Site { body, .. } => check_body(body, sink),
        }
    }
}

fn check_theme_body(body: &[ThemeRule], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<String, Span> = IndexMap::new();
    for rule in body {
        match rule {
            ThemeRule::Slot { slot, span, .. } => {
                if let Some(first_span) = seen.get(slot) {
                    sink.push(Diagnostic {
                        code: DiagnosticCode::DuplicateSlot,
                        severity: DiagnosticCode::DuplicateSlot.severity(),
                        span: span.clone(),
                        primary: format!("`slot {slot}` is declared more than once"),
                        notes: vec![DiagnosticNote {
                            span: Some(first_span.clone()),
                            message: "first declaration here".into(),
                        }],
                    });
                } else {
                    seen.insert(slot.clone(), span.clone());
                }
            }
            ThemeRule::Selector {
                attrs, bindings, ..
            } => {
                check_arg_list(attrs, sink);
                check_arg_list(bindings, sink);
            }
        }
    }
}

/// Header-args scope: emit `E_DUPLICATE_SIZE` for repeated `size=`, and
/// `E_DUPLICATE_ARG` for any other repeated key.
fn check_header_args(args: &[Arg], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<String, Span> = IndexMap::new();
    for arg in args {
        if let Some(first_span) = seen.get(&arg.key) {
            let code = if arg.key == "size" {
                DiagnosticCode::DuplicateSize
            } else {
                DiagnosticCode::DuplicateArg
            };
            sink.push(Diagnostic {
                code,
                severity: code.severity(),
                span: arg.span.clone(),
                primary: format!("`{}=` is declared more than once in this header", arg.key),
                notes: vec![DiagnosticNote {
                    span: Some(first_span.clone()),
                    message: "first declaration here".into(),
                }],
            });
        } else {
            seen.insert(arg.key.clone(), arg.span.clone());
        }
    }
}

/// Non-header arg list scope: every duplicate key is `E_DUPLICATE_ARG`.
fn check_arg_list(args: &[Arg], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<String, Span> = IndexMap::new();
    for arg in args {
        if let Some(first_span) = seen.get(&arg.key) {
            sink.push(Diagnostic {
                code: DiagnosticCode::DuplicateArg,
                severity: DiagnosticCode::DuplicateArg.severity(),
                span: arg.span.clone(),
                primary: format!("`{}=` is declared more than once", arg.key),
                notes: vec![DiagnosticNote {
                    span: Some(first_span.clone()),
                    message: "first declaration here".into(),
                }],
            });
        } else {
            seen.insert(arg.key.clone(), arg.span.clone());
        }
    }
}

fn check_body(body: &[Statement], sink: &mut DiagnosticSink) {
    // Per immediate body: collect `id=` values declared by `Statement::Generic`
    // at this depth, plus the `key=` arg list of each statement and selector.
    let mut seen_ids: IndexMap<String, Span> = IndexMap::new();
    for stmt in body {
        if let Statement::Generic {
            args,
            selector,
            children,
            span,
            ..
        } = stmt
        {
            check_arg_list(args, sink);
            if let Some(attrs) = selector {
                check_arg_list(attrs, sink);
            }
            // Hoist the id value (and its span) out of args / selector and
            // diagnose duplicates within this scope. Both kinds of id-bearing
            // attribute count.
            if let Some((id, id_span)) = extract_id(stmt) {
                if let Some(first_span) = seen_ids.get(&id) {
                    sink.push(Diagnostic {
                        code: DiagnosticCode::DuplicateId,
                        severity: DiagnosticCode::DuplicateId.severity(),
                        span: id_span,
                        primary: format!("`id={id}` is declared more than once in this scope"),
                        notes: vec![DiagnosticNote {
                            span: Some(first_span.clone()),
                            message: "first declaration here".into(),
                        }],
                    });
                } else {
                    seen_ids.insert(id, id_span);
                }
            }
            // Nested body has its own scope — both for `id=` and for args.
            let _ = span;
            check_body(children, sink);
        }
    }
}

/// Pull an `id=label` *declaration* out of a generic statement.
///
/// Only the dedicated arg list (`door id=front ...`) declares a fresh id;
/// the selector form (`door[id=front] ...`) references an existing member
/// per the surface grammar — see `Member::selector` in `intent::member`.
/// Treating selector ids as declarations would falsely flag the
/// `redstone-door` example's `logic` rebind line.
///
/// `id=` whose value is not a label is left to the type-mismatch pass.
fn extract_id(stmt: &Statement) -> Option<(String, Span)> {
    let Statement::Generic { args, .. } = stmt else {
        return None;
    };
    args.iter().find_map(label_id)
}

fn label_id(arg: &Arg) -> Option<(String, Span)> {
    if arg.key != "id" {
        return None;
    }
    match &arg.value.kind {
        ValueKind::Ident(s) | ValueKind::Str(s) => Some((s.clone(), arg.span.clone())),
        _ => None,
    }
}
