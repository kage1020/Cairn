//! `type_mismatch` pass — flags label-typed keys with non-label values and
//! `size=` with a non-`Size` value.
//!
//! Walks the surface AST, where every value still has its type and span
//! whether or not lowering hoisted it out of `IntentState`.

use crate::ast::{Arg, Item, Module, Statement, ThemeRule, Value, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticCode};

use super::DiagnosticSink;

/// Keys whose value is a label and whose mistyping is otherwise silent.
///
/// `circuit region=` is left to `block_array`'s `W_DEFERRED_MEMBER`, and
/// `east_of=` / `north_of=` to `E_INVALID_PLACE_ORIGIN`, so no value is
/// reported under two codes. A mistyped `use=` or `theme=` makes
/// `resolve_site_placements` drop the placement just as a row with no
/// `use=` does, so this finding is what tells the two apart.
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
