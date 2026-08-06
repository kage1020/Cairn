//! `duplicate` pass — flags `key=` / `slot` / `id=` repeated in the same
//! scope.
//!
//! Walks the surface AST rather than the IR because the IR's
//! [`IntentState`](crate::intent::IntentState) and `args` maps are
//! last-write-wins; the surface form is the only place where both the first
//! and second occurrences are still visible.
//!
//! The codes emitted here use distinct scopes:
//! - `E_DUPLICATE_HEADER` — a single-valued `@directive` (`@cairn`,
//!   `@intended_targets`) appears more than once in the module.
//!   `@requires` is exempt; see [`check_headers`].
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
//! declaration, so the anchor is the token the author would edit and the
//! note is the one they would keep.

use indexmap::IndexMap;

use crate::ast::{Arg, Header, Item, ItemKind, Module, Statement, ThemeRule, ValueKind};
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

/// Module-header scope: a single-valued `@directive` may be declared
/// once.
///
/// `@cairn` and `@intended_targets` are single-valued by construction —
/// one language version the file was written against, one list of
/// intended targets. Two of either state two answers to a question that
/// has one, and no rule anywhere says which holds. Neither has a
/// consumer in the compiler today (both are provenance for a future
/// reader), which is precisely why a repeat has to be reported rather
/// than resolved: there is no reader to prefer one over the other, and
/// picking silently later would be worse than saying so now.
///
/// `@requires` is exempt because it *composes*. `resolve::version_axes`
/// folds every `version>=X` floor to the strictest, which is the
/// conjunction of the constraints, not a choice between them — nothing
/// is discarded, and the behaviour is documented on `RegistryRange` and
/// pinned by that module's tests. Reporting it here would make an error
/// out of a shape the rest of the crate defines as meaningful.
fn check_headers(headers: &[Header], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<&'static str, Span> = IndexMap::new();
    for header in headers {
        let directive = match header {
            Header::Cairn { .. } => "@cairn",
            Header::IntendedTargets { .. } => "@intended_targets",
            Header::Requires { .. } => continue,
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
                            "`{directive}` is single-valued: keep the line that states what you mean and delete the other",
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
/// [`ItemKind`] is the namespace: the resolver keys scopes `struct::`,
/// `def::`, and `site::NAME::` and holds themes in a map of their own,
/// so the same name on two different kinds never collides and must not
/// be reported. Keying the seen-map by the kind rather than by a
/// keyword string is what keeps this pass and the resolver reading the
/// same definition of "namespace".
fn check_item_names(items: &[Item], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<(ItemKind, &str), Span> = IndexMap::new();
    for item in items {
        let kind = item.kind();
        let keyword = kind.keyword();
        let name = item.name();
        // Anchor on the name token, not the item's span: the block span
        // covers the indented body, and underlining a whole `def` says
        // nothing about which word to change.
        let span = item.name_span().clone();
        if let Some(first_span) = seen.get(&(kind, name)) {
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
                        message: repair_advice(kind, name),
                    },
                ],
                data: None,
            });
        } else {
            seen.insert((kind, name), span);
        }
    }
}

/// What the author has to do about a repeated name, which differs by
/// kind because what the repeat costs differs by kind.
///
/// For `theme` / `def` / `struct` the name *is* the binding key, so the
/// second declaration binds nothing and its body reaches no artifact.
/// For `site` the binding key is `site::NAME::PLACE_ID`, so two blocks
/// of one name do not shadow each other — their places land in one
/// shared namespace and all of them build. Telling that author "the
/// first one resolves" would invite them to delete a block whose
/// placements were in the build.
fn repair_advice(kind: ItemKind, name: &str) -> String {
    let keyword = kind.keyword();
    match kind {
        ItemKind::Site => format!(
            "two `{keyword} {name}` blocks share one `site::{name}::` namespace: places with distinct `id=` all build, a repeated `id=` keeps only the first, and `east_of=` cannot reach across the blocks — merge the bodies into one block, or give this one its own name"
        ),
        ItemKind::Theme | ItemKind::Def | ItemKind::Struct => format!(
            "the first `{keyword} {name}` is the one that resolves; rename this one, or merge the two bodies"
        ),
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
