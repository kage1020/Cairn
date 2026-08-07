//! `type_mismatch` pass — flags label-typed keys with non-label values and
//! `size=` with a non-`Size` value.
//!
//! Walks the surface AST because the offending values may have been hoisted
//! out of `IntentState` during lowering (when the value *was* a label) or
//! kept (when it was not), and the AST is the simplest place where the
//! original value type is still attached to its span.

use crate::ast::{Arg, Item, Module, Statement, ThemeRule, Value, ValueKind};

use super::{Diagnostic, DiagnosticCode, DiagnosticSink};

/// Keys whose value is a label and whose mistyping is otherwise silent.
///
/// Not every `as_label_str` call site is here. `circuit region=` is lifted
/// the same way but has its own recognizer in `block_array`, which names
/// the key and the offending kind in a `W_DEFERRED_MEMBER`; adding it to
/// this list would report the same value twice with two different codes.
/// `east_of=` / `north_of=` are owned by `E_INVALID_PLACE_ORIGIN` for the
/// same reason. What is left are the keys whose mistyped value reaches a
/// reader that skips silently.
///
/// `id` / `class` / `mat_slot` are hoisted onto [`crate::intent::Member`]'s
/// own fields during lowering; `use` / `theme` stay in `intent_state` and
/// are read by `resolve::resolve_site_placements`. The distinction does not
/// reach the author: all five reject a non-label the same way, and for
/// `use=` / `theme=` the rejection used to be the resolver's silent
/// `continue`, which drops the whole placement — the same arm a `place` row
/// with no `use=` at all takes. Naming the mistyped case here is what tells
/// those two apart.
const LABEL_KEYS: &[&str] = &["id", "class", "mat_slot", "use", "theme"];

pub(super) fn run(module: &Module, sink: &mut DiagnosticSink) {
    for item in &module.items {
        match item {
            Item::Theme { body, .. } => {
                for rule in body {
                    check_theme_rule(rule, sink);
                }
            }
            Item::Def { args, body, .. } | Item::Struct { args, body, .. } => {
                check_args(args, sink);
                check_body(body, sink);
            }
            Item::Site { body, .. } => check_body(body, sink),
        }
    }
}

fn check_theme_rule(rule: &ThemeRule, sink: &mut DiagnosticSink) {
    if let ThemeRule::Selector {
        attrs, bindings, ..
    } = rule
    {
        check_args(attrs, sink);
        check_args(bindings, sink);
    }
}

fn check_body(body: &[Statement], sink: &mut DiagnosticSink) {
    for stmt in body {
        if let Statement::Generic {
            args,
            selector,
            children,
            ..
        } = stmt
        {
            check_args(args, sink);
            if let Some(attrs) = selector {
                check_args(attrs, sink);
            }
            check_body(children, sink);
        }
    }
}

fn check_args(args: &[Arg], sink: &mut DiagnosticSink) {
    for arg in args {
        if arg.key == "size" {
            check_size(&arg.value, sink);
        } else if LABEL_KEYS.contains(&arg.key.as_str()) {
            check_label(&arg.key, &arg.value, sink);
        }
    }
}

fn check_size(value: &Value, sink: &mut DiagnosticSink) {
    if matches!(value.kind, ValueKind::Size { .. }) {
        return;
    }
    sink.push(Diagnostic {
        code: DiagnosticCode::TypeMismatchSize,
        span: value.span.clone(),
        primary: format!("`size=` expects a `WxH` literal, got {}", value.kind_name()),
        notes: Vec::new(),
        data: None,
    });
}

fn check_label(key: &str, value: &Value, sink: &mut DiagnosticSink) {
    if matches!(value.kind, ValueKind::Ident(_) | ValueKind::Str(_)) {
        return;
    }
    sink.push(Diagnostic {
        code: DiagnosticCode::TypeMismatchLabel,
        span: value.span.clone(),
        primary: format!(
            "`{key}=` expects a label (identifier or string), got {}",
            value.kind_name()
        ),
        notes: Vec::new(),
        data: None,
    });
}
