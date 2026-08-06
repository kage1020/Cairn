//! `duplicate` pass — flags `key=` / `slot` / `id=` repeated in the same
//! scope.
//!
//! Walks the surface AST rather than the IR because the IR's
//! [`IntentState`](crate::intent::IntentState) and `args` maps are
//! last-write-wins; the surface form is the only place where both the first
//! and second occurrences are still visible.
//!
//! The codes emitted here use distinct scopes:
//! - `E_DUPLICATE_HEADER` — a `@directive` appears more than once in the
//!   module.
//! - `E_DUPLICATE_ITEM` — two top-level items *of the same kind* share a
//!   name. The four kinds are separate namespaces, so this is four
//!   independent scopes rather than one.
//! - `E_DUPLICATE_SIZE` — a struct/def header has more than one `size=`.
//! - `E_DUPLICATE_SLOT` — a `theme` body has two `slot NAME ->` lines for
//!   the same `NAME`.
//! - `E_DUPLICATE_ARG`  — any other duplicate `key=` inside the same
//!   argument list (header excluding `size=`, statement args, selector
//!   attrs, selector bindings, header args of struct/def excluding size).
//! - `E_DUPLICATE_ID`   — two members in the same immediate body scope
//!   declare `id=NAME` for the same `NAME` (per-body scope; nested `level`
//!   blocks have their own namespace).
//!
//! Every scope here reports the *repeat* and points a note at the first
//! declaration, so the anchor is the line the author would delete or
//! rename and the note is the one they would keep.

use indexmap::IndexMap;

use crate::ast::{Arg, Header, Item, Module, Statement, ThemeRule, ValueKind};
use crate::error::Span;

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    check_headers(&module.headers, sink);
    check_item_names(&module.items, sink);
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

/// Module-header scope: each `@directive` may be declared once.
///
/// A repeat is not merely redundant. `@cairn` and `@intended_targets`
/// have exactly one reader each, which takes the first match and never
/// looks further; `@requires` floors are folded together by taking the
/// strictest, so a second line asking for *less* than the first leaves
/// no trace anywhere in the build. Both shapes are a declaration the
/// author wrote and the compiler discarded.
fn check_headers(headers: &[Header], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<&'static str, Span> = IndexMap::new();
    for header in headers {
        let directive = match header {
            Header::Cairn { .. } => "@cairn",
            Header::Requires { .. } => "@requires",
            Header::IntendedTargets { .. } => "@intended_targets",
        };
        let span = header.span().clone();
        if let Some(first_span) = seen.get(directive) {
            sink.push(Diagnostic {
                code: DiagnosticCode::DuplicateHeader,
                severity: DiagnosticCode::DuplicateHeader.severity(),
                span,
                primary: format!("`{directive}` is declared more than once"),
                notes: vec![
                    DiagnosticNote {
                        span: Some(first_span.clone()),
                        message: "first declaration here".into(),
                    },
                    DiagnosticNote {
                        span: None,
                        message: format!(
                            "a module carries at most one `{directive}`; keep the line that states what you mean and delete the other",
                        ),
                    },
                ],
                data: None,
            });
        } else {
            seen.insert(directive, span);
        }
    }
}

/// Top-level name scope, one namespace per item kind.
///
/// The resolver keys structs, defs, and placements under `struct::`,
/// `def::`, and `site::NAME::` prefixes and holds themes in a map of
/// their own, so the same name on two different kinds never collides and
/// must not be reported. A repeat *within* a kind is the case where one
/// declaration is discarded: the resolver binds the first and skips the
/// rest, which leaves the second body's members out of every artifact.
fn check_item_names(items: &[Item], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<(&'static str, &str), Span> = IndexMap::new();
    for item in items {
        let keyword = item.keyword();
        // Anchor on the name token, not the item's span: the block span
        // covers the indented body, and underlining a whole `def` says
        // nothing about which word to change.
        let (name, span) = item.name();
        let span = span.clone();
        if let Some(first_span) = seen.get(&(keyword, name)) {
            sink.push(Diagnostic {
                code: DiagnosticCode::DuplicateItem,
                severity: DiagnosticCode::DuplicateItem.severity(),
                span,
                primary: format!("`{keyword} {name}` is declared more than once"),
                notes: vec![
                    DiagnosticNote {
                        span: Some(first_span.clone()),
                        message: "first declaration here".into(),
                    },
                    DiagnosticNote {
                        span: None,
                        message: format!(
                            "the first `{keyword} {name}` is the one that resolves; rename this one, or merge the two bodies",
                        ),
                    },
                ],
                data: None,
            });
        } else {
            seen.insert((keyword, name), span);
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
                        data: None,
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
                data: None,
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
                data: None,
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
                        data: None,
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
