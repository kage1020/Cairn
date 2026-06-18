//! AST → Intent IR lowering.
//!
//! Intentionally total: every AST that survives [`crate::parse`] lowers to a
//! well-formed [`IntentModule`]. Validation lives elsewhere (M2-PR2's
//! `check` module) so that diagnostic collection can run to completion
//! instead of being short-circuited by the first structural surprise here.

use indexmap::IndexMap;

use crate::ast::{Arg, Item, Module, Statement, ThemeRule, Value};

use super::{
    AssertIr, DefIr, IntentModule, IntentState, LogicBinding, Member, SelectorRule, SemanticLevel,
    SiteIr, Size, StructIr, ThemeIr, role_of,
};

/// Lower a parsed [`Module`] into its [`IntentModule`] form.
///
/// Total function: returns a value with [`SemanticLevel::Grouped`] for every
/// successfully-parsed input. Unknown keywords are preserved via
/// [`MemberRole::Other`] rather than rejected.
#[must_use]
pub fn lower(module: &Module) -> IntentModule {
    let mut themes = Vec::new();
    let mut defs = Vec::new();
    let mut sites = Vec::new();
    let mut structs = Vec::new();

    for item in &module.items {
        match item {
            Item::Theme { name, body } => themes.push(lower_theme(name, body)),
            Item::Def { name, args, body } => defs.push(lower_def(name, args, body)),
            Item::Site { name, body } => sites.push(lower_site(name, body)),
            Item::Struct { name, args, body } => structs.push(lower_struct(name, args, body)),
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

fn lower_theme(name: &str, body: &[ThemeRule]) -> ThemeIr {
    let mut slots: IndexMap<String, Value> = IndexMap::new();
    let mut selectors = Vec::new();

    for rule in body {
        match rule {
            ThemeRule::Slot { slot, value } => {
                slots.insert(slot.clone(), value.clone());
            }
            ThemeRule::Selector {
                keyword,
                attrs,
                bindings,
            } => {
                selectors.push(SelectorRule {
                    keyword: keyword.clone(),
                    attrs: args_to_map(attrs),
                    bindings: args_to_map(bindings),
                });
            }
        }
    }

    ThemeIr {
        name: name.to_owned(),
        slots,
        selectors,
    }
}

fn lower_struct(name: &str, header_args: &[Arg], body: &[Statement]) -> StructIr {
    let HeaderBreakdown { size, args } = split_size(header_args);
    let LoweredBody {
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
    }
}

fn lower_def(name: &str, header_args: &[Arg], body: &[Statement]) -> DefIr {
    let HeaderBreakdown { size, args } = split_size(header_args);
    let LoweredBody {
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
    }
}

fn lower_site(name: &str, body: &[Statement]) -> SiteIr {
    let LoweredBody {
        members,
        logic,
        asserts,
    } = lower_body(body);
    SiteIr {
        name: name.to_owned(),
        placements: members,
        logic,
        asserts,
    }
}

struct HeaderBreakdown {
    size: Option<Size>,
    args: IndexMap<String, Value>,
}

fn split_size(header_args: &[Arg]) -> HeaderBreakdown {
    let mut size = None;
    let mut args: IndexMap<String, Value> = IndexMap::new();
    for arg in header_args {
        // Only the first `size=WxH` wins; the M2-PR2 duplicate pass will
        // surface any second occurrence as `E_DUPLICATE_ARG`.
        if arg.key == "size"
            && let Value::Size { w, h } = arg.value
            && size.is_none()
        {
            size = Some(Size { w, h });
            continue;
        }
        args.insert(arg.key.clone(), arg.value.clone());
    }
    HeaderBreakdown { size, args }
}

struct LoweredBody {
    members: Vec<Member>,
    logic: Vec<LogicBinding>,
    asserts: Vec<AssertIr>,
}

fn lower_body(body: &[Statement]) -> LoweredBody {
    let mut members = Vec::new();
    let mut logic = Vec::new();
    let mut asserts = Vec::new();

    for stmt in body {
        match stmt {
            Statement::Generic { .. } => members.push(lower_member(stmt)),
            Statement::Logic { lhs, rhs } => logic.push(LogicBinding {
                lhs: lhs.clone(),
                rhs: rhs.clone(),
            }),
            Statement::AssertTruth {
                inputs,
                output,
                rows,
            } => asserts.push(AssertIr::Truth {
                inputs: inputs.clone(),
                output: output.clone(),
                rows: rows.clone(),
            }),
            Statement::AssertAlways {
                antecedent,
                consequent,
                within,
            } => asserts.push(AssertIr::Always {
                antecedent: antecedent.clone(),
                consequent: consequent.clone(),
                within: *within,
            }),
        }
    }

    LoweredBody {
        members,
        logic,
        asserts,
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
    } = stmt
    else {
        unreachable!("lower_member must be called with Statement::Generic; lower_body dispatches");
    };

    let role = role_of(keyword);
    let mut id = None;
    let mut class = None;
    let mut mat_slot = None;
    let mut intent_state = IntentState::new();

    for arg in args {
        // `id` / `class` / `mat_slot` are hoisted into dedicated `Member`
        // fields when the value is a plain label; anything that is not
        // label-shaped, or a second occurrence of the same key, stays in
        // `intent_state` for the M2-PR2 passes to diagnose.
        let hoisted = match arg.key.as_str() {
            "id" => hoist_label(&arg.value, &mut id),
            "class" => hoist_label(&arg.value, &mut class),
            "mat_slot" => hoist_label(&arg.value, &mut mat_slot),
            _ => false,
        };
        if !hoisted {
            intent_state.fields.insert(arg.key.clone(), arg.value.clone());
        }
    }

    let lowered_selector = selector.as_ref().map(|attrs| args_to_map(attrs));
    let lowered_children = children.iter().map(lower_member).collect();

    Member {
        id,
        class,
        role,
        mat_slot,
        selector: lowered_selector,
        positional: positional.clone(),
        binding: binding.clone(),
        intent_state,
        resolved_state: None,
        children: lowered_children,
    }
}

fn args_to_map(args: &[Arg]) -> IndexMap<String, Value> {
    let mut map = IndexMap::with_capacity(args.len());
    for arg in args {
        // Last-write-wins on duplicate keys; the M2-PR2 duplicate pass will
        // catch the duplicate before this matters.
        map.insert(arg.key.clone(), arg.value.clone());
    }
    map
}

/// Try to move a label-valued `id` / `class` / `mat_slot` argument into a
/// dedicated [`Member`] field. Returns `true` when the value was consumed;
/// `false` keeps the argument in [`IntentState`] so a later validation pass
/// can report the mismatch (non-label value, or a duplicate key).
fn hoist_label(value: &Value, slot: &mut Option<String>) -> bool {
    if slot.is_some() {
        return false;
    }
    let label = match value {
        Value::Ident(s) | Value::Str(s) | Value::Token(s) => s.clone(),
        Value::DotRef(dr) => dr.to_string(),
        _ => return false,
    };
    *slot = Some(label);
    true
}
