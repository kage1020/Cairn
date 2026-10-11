//! AST → Intent IR lowering, which is total: validation belongs to `crate::check`.

use indexmap::IndexMap;

use crate::ast::{Arg, Item, Module, Statement, ThemeRule, Value, ValueKind};

use super::{
    AssertIr, DefIr, IntentModule, IntentState, Label, LogicBinding, Member, MemberBody,
    SelectorRule, SemanticLevel, SiteIr, Size, StructIr, ThemeIr, ValueWithSpan, role_of,
};

/// Lower a parsed [`Module`] into its [`IntentModule`] form, at [`SemanticLevel::Grouped`].
#[must_use]
pub fn lower(module: &Module) -> IntentModule {
    let mut themes = Vec::new();
    let mut defs = Vec::new();
    let mut sites = Vec::new();
    let mut structs = Vec::new();

    for item in &module.items {
        match item {
            Item::Theme {
                name, body, span, ..
            } => themes.push(lower_theme(name, body, span.clone())),
            Item::Def {
                name,
                args,
                body,
                span,
                ..
            } => defs.push(lower_def(name, args, body, span.clone())),
            Item::Site {
                name, body, span, ..
            } => sites.push(lower_site(name, body, span.clone())),
            Item::Struct {
                name,
                args,
                body,
                span,
                ..
            } => structs.push(lower_struct(name, args, body, span.clone())),
        }
    }

    IntentModule {
        semantic_level: SemanticLevel::Grouped,
        headers: module.headers.clone(),
        themes,
        defs,
        sites,
        structs,
    }
}

fn lower_theme(name: &str, body: &[ThemeRule], span: crate::error::Span) -> ThemeIr {
    let mut slots: IndexMap<String, ValueWithSpan> = IndexMap::new();
    let mut selectors = Vec::new();

    for rule in body {
        match rule {
            ThemeRule::Slot { slot, value, .. } => {
                slots.insert(slot.clone(), ValueWithSpan::from_value(value.clone()));
            }
            ThemeRule::Selector {
                keyword,
                attrs,
                bindings,
                span: rule_span,
            } => {
                selectors.push(SelectorRule {
                    keyword: keyword.clone(),
                    attrs: args_to_map(attrs),
                    bindings: args_to_map(bindings),
                    span: rule_span.clone(),
                });
            }
        }
    }

    ThemeIr {
        name: name.to_owned(),
        slots,
        selectors,
        span,
    }
}

fn lower_struct(
    name: &str,
    header_args: &[Arg],
    body: &[Statement],
    span: crate::error::Span,
) -> StructIr {
    let HeaderBreakdown { size, args } = split_size(header_args);
    let MemberBody {
        members,
        logic,
        asserts,
    } = lower_body(body);
    StructIr {
        name: name.to_owned(),
        size,
        args,
        members,
        logic,
        asserts,
        span,
    }
}

fn lower_def(
    name: &str,
    header_args: &[Arg],
    body: &[Statement],
    span: crate::error::Span,
) -> DefIr {
    let HeaderBreakdown { size, args } = split_size(header_args);
    let MemberBody {
        members,
        logic,
        asserts,
    } = lower_body(body);
    DefIr {
        name: name.to_owned(),
        size,
        args,
        members,
        logic,
        asserts,
        span,
    }
}

fn lower_site(name: &str, body: &[Statement], span: crate::error::Span) -> SiteIr {
    let MemberBody {
        members,
        logic,
        asserts,
    } = lower_body(body);
    SiteIr {
        name: name.to_owned(),
        placements: members,
        logic,
        asserts,
        span,
    }
}

struct HeaderBreakdown {
    size: Option<Size>,
    args: IndexMap<String, ValueWithSpan>,
}

fn split_size(header_args: &[Arg]) -> HeaderBreakdown {
    let mut size = None;
    let mut args: IndexMap<String, ValueWithSpan> = IndexMap::new();
    for arg in header_args {
        // A repeated `size=WxH` is dropped here; `check::duplicate` reports it from the AST.
        if arg.key == "size"
            && let ValueKind::Size { w, h } = &arg.value.kind
        {
            if size.is_none() {
                size = Some(Size {
                    w: *w,
                    h: *h,
                    span: arg.value.span.clone(),
                });
            }
            continue;
        }
        args.insert(
            arg.key.clone(),
            ValueWithSpan::from_value(arg.value.clone()),
        );
    }
    HeaderBreakdown { size, args }
}

fn lower_body(body: &[Statement]) -> MemberBody {
    let mut out = MemberBody::default();
    for stmt in body {
        push_statement(stmt, &mut out);
    }
    out
}

/// Append one body statement to a [`MemberBody`], grouped by flavour.
fn push_statement(stmt: &Statement, out: &mut MemberBody) {
    match stmt {
        Statement::Generic { .. } => out.members.push(lower_member(stmt)),
        Statement::Logic { lhs, rhs, span } => out.logic.push(LogicBinding {
            lhs: lhs.clone(),
            rhs: rhs.clone(),
            span: span.clone(),
        }),
        Statement::AssertTruth {
            inputs,
            output,
            rows,
            span,
        } => out.asserts.push(AssertIr::Truth {
            inputs: inputs.clone(),
            output: output.clone(),
            rows: rows.clone(),
            span: span.clone(),
        }),
        Statement::AssertAlways {
            antecedent,
            consequent,
            within,
            span,
        } => out.asserts.push(AssertIr::Always {
            antecedent: antecedent.clone(),
            consequent: consequent.clone(),
            within: *within,
            span: span.clone(),
        }),
    }
}

fn lower_member(stmt: &Statement) -> Member {
    let Statement::Generic {
        keyword,
        selector,
        positional,
        args,
        binding,
        children,
        span,
    } = stmt
    else {
        // `push_statement` never sends one here; lowering stays total all the same.
        let mut body = MemberBody::default();
        push_statement(stmt, &mut body);
        return placeholder_member_carrying(body, stmt.span().clone());
    };

    let role = role_of(keyword);
    let mut id = None;
    let mut class = None;
    let mut mat_slot = None;
    let mut intent_state = IntentState::new();

    for arg in args {
        let hoisted = match arg.key.as_str() {
            "id" => hoist_label(&arg.value, &mut id),
            "class" => hoist_label(&arg.value, &mut class),
            "mat_slot" => hoist_label(&arg.value, &mut mat_slot),
            _ => false,
        };
        if !hoisted {
            intent_state.insert(
                arg.key.clone(),
                ValueWithSpan::from_value(arg.value.clone()),
            );
        }
    }

    let lowered_selector = selector.as_ref().map(|attrs| args_to_map(attrs));
    let lowered_children = lower_body(children);

    Member {
        id: id.map(|label| label.name),
        class: class.map(|label| label.name),
        role,
        mat_slot,
        selector: lowered_selector,
        positional: positional.clone(),
        binding: binding.clone(),
        intent_state,
        resolved_state: None,
        children: lowered_children,
        span: span.clone(),
    }
}

/// A synthetic `Other` member holding `body`, for a statement that is not a member line.
fn placeholder_member_carrying(body: MemberBody, span: crate::error::Span) -> Member {
    Member {
        id: None,
        class: None,
        role: super::MemberRole::Other(String::new()),
        mat_slot: None,
        selector: None,
        positional: Vec::new(),
        binding: None,
        intent_state: IntentState::new(),
        resolved_state: None,
        children: body,
        span,
    }
}

fn args_to_map(args: &[Arg]) -> IndexMap<String, ValueWithSpan> {
    let mut map = IndexMap::with_capacity(args.len());
    for arg in args {
        map.insert(
            arg.key.clone(),
            ValueWithSpan::from_value(arg.value.clone()),
        );
    }
    map
}

/// Move an identifier or string `value` into an empty `slot`; `false` leaves it in [`IntentState`].
fn hoist_label(value: &Value, slot: &mut Option<Label>) -> bool {
    if slot.is_some() {
        return false;
    }
    let name = match &value.kind {
        ValueKind::Ident(s) | ValueKind::Str(s) => s.clone(),
        _ => return false,
    };
    *slot = Some(Label {
        name,
        span: value.span().clone(),
    });
    true
}
