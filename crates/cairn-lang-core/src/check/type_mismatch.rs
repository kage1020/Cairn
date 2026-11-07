//! The `type_mismatch` pass: a label key with a non-label value, or a footprint's `size=` with a
//! non-`WxH` one. It walks the surface AST, where a value lowering hoisted still has its type and
//! span.

use crate::ast::{Arg, Item, Module, Statement, ThemeRule, Value, ValueKind, guarded_lines};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::role_of;

use super::DiagnosticSink;

/// Label keys whose mistyping is otherwise silent. `region=`, `east_of=` and `north_of=` are
/// left out because another code already reports them.
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
                check_args(None, args, sink);
                check_body(body, sink);
            }
            Item::Site { body, .. } => check_body(body, sink),
        }
    }
}

fn check_theme_rule(rule: &ThemeRule, sink: &mut DiagnosticSink) {
    if let ThemeRule::Selector {
        keyword,
        attrs,
        bindings,
        ..
    } = rule
    {
        check_args(Some(keyword), attrs, sink);
        check_args(Some(keyword), bindings, sink);
    }
}

fn check_body(body: &[Statement], sink: &mut DiagnosticSink) {
    for (_, stmt) in guarded_lines(body) {
        if let Statement::Generic {
            keyword,
            args,
            selector,
            children,
            ..
        } = stmt
        {
            check_args(Some(keyword), args, sink);
            if let Some(attrs) = selector {
                check_args(Some(keyword), attrs, sink);
            }
            check_body(children, sink);
        }
    }
}

/// Check the arguments of a line led by `keyword`, or of a header when there is none.
fn check_args(keyword: Option<&str>, args: &[Arg], sink: &mut DiagnosticSink) {
    let size_is_a_state =
        keyword.is_some_and(|keyword| role_of(keyword).state_arguments().contains(&"size"));
    for arg in args {
        if arg.key == "size" {
            if !size_is_a_state {
                check_size(&arg.value, sink);
            }
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
        notes: vec![DiagnosticNote {
            span: None,
            message: "write two whole numbers joined by `x`, such as `size=9x7`".to_owned(),
        }],
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
        notes: vec![DiagnosticNote {
            span: None,
            message: label_fix(key, value),
        }],
        data: None,
    });
}

/// How to write `key=` as the label it expects, given the value written instead.
fn label_fix(key: &str, value: &Value) -> String {
    match (key, &value.kind) {
        ("mat_slot", ValueKind::Token(token)) => format!(
            "`mat_slot=` names a slot of the bound theme, not a block: bind the block there \
             (`slot NAME -> @{token}`) and write `mat_slot=NAME`"
        ),
        ("mat_slot", ValueKind::BlockId(id)) => {
            let bind = id.strip_prefix("minecraft:").map_or_else(
                || "bind a canonical token there".to_owned(),
                |token| format!("bind its canonical token there (`slot NAME -> @{token}`)"),
            );
            format!(
                "`mat_slot=` names a slot of the bound theme, not a block: {bind} and write \
                 `mat_slot=NAME`, or place `{id}` as written with a `raw_block` or `raw_fill`"
            )
        }
        ("mat_slot", _) => "write the name of a slot the bound theme declares".to_owned(),
        ("use", _) => "write the name of a `def` this file declares".to_owned(),
        ("theme", _) => "write the name of a `theme` this file declares".to_owned(),
        _ => format!("write a name, such as `{key}=main`, or a quoted string"),
    }
}
