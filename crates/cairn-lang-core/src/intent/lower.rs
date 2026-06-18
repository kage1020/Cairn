//! AST → Intent IR lowering.
//!
//! Intentionally total: every AST that survives [`crate::parse`] lowers to a
//! well-formed [`IntentModule`]. Validation lives elsewhere (M2-PR2's
//! `check` module) so that diagnostic collection can run to completion
//! instead of being short-circuited by the first structural surprise here.

use indexmap::IndexMap;

use crate::ast::{Arg, Item, Module, Statement, ThemeRule, Value};

use super::{
    AssertIr, DefIr, IntentModule, IntentState, LogicBinding, Member, MemberBody, SelectorRule,
    SemanticLevel, SiteIr, Size, StructIr, ThemeIr, role_of,
};

/// Lower a parsed [`Module`] into its [`IntentModule`] form.
///
/// Total function: returns a value with [`SemanticLevel::Grouped`] for every
/// successfully-parsed input. Unknown keywords are preserved via
/// [`super::MemberRole::Other`] rather than rejected, and any duplication
/// (repeated `size=`, duplicate slot, etc.) is *silently* normalised here
/// on the IR side — the M2-PR2 `duplicate` pass detects those by walking
/// the surface [`Module`] directly, so the IR's last-write-wins shape is
/// not load-bearing for diagnostics.
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
                // Last-write-wins on duplicate slot names. The M2-PR2
                // `duplicate` pass walks `&Module` directly, so the IR not
                // remembering the earlier value is fine.
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
    }
}

fn lower_def(name: &str, header_args: &[Arg], body: &[Statement]) -> DefIr {
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
    }
}

fn lower_site(name: &str, body: &[Statement]) -> SiteIr {
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
        // The first well-typed `size=WxH` hoists into `StructIr::size`; any
        // additional `size=` occurrences are dropped from the IR. A repeated
        // declaration is still visible to the M2-PR2 `duplicate` pass via
        // `&Module`, so it isn't lost — it just doesn't leak into the
        // residual `args` map and contradict that field's documented
        // contract ("everything except size"). Non-`Size` values for `size=`
        // do fall through and end up in `args` so the type-mismatch pass
        // can flag them.
        if arg.key == "size"
            && let Value::Size { w, h } = arg.value
        {
            if size.is_none() {
                size = Some(Size { w, h });
            }
            continue;
        }
        args.insert(arg.key.clone(), arg.value.clone());
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

/// Append one body statement to a [`MemberBody`], grouping by statement
/// flavour. Used both for the top-level body of a struct/def/site and for
/// the nested body indented under a member (`level y=0` etc.), so the same
/// triple of (members, logic, asserts) is preserved at every depth.
fn push_statement(stmt: &Statement, out: &mut MemberBody) {
    match stmt {
        Statement::Generic { .. } => out.members.push(lower_member(stmt)),
        Statement::Logic { lhs, rhs } => out.logic.push(LogicBinding {
            lhs: lhs.clone(),
            rhs: rhs.clone(),
        }),
        Statement::AssertTruth {
            inputs,
            output,
            rows,
        } => out.asserts.push(AssertIr::Truth {
            inputs: inputs.clone(),
            output: output.clone(),
            rows: rows.clone(),
        }),
        Statement::AssertAlways {
            antecedent,
            consequent,
            within,
        } => out.asserts.push(AssertIr::Always {
            antecedent: antecedent.clone(),
            consequent: consequent.clone(),
            within: *within,
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
    } = stmt
    else {
        // push_statement is the only caller and dispatches on the variant
        // before reaching here; an unreachable here would still be wrong if
        // dispatch ever changed, so route the non-generic flavours through
        // the body grouping instead of relying on a panic.
        let mut body = MemberBody::default();
        push_statement(stmt, &mut body);
        return placeholder_member_carrying(body);
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
            intent_state.insert(arg.key.clone(), arg.value.clone());
        }
    }

    let lowered_selector = selector.as_ref().map(|attrs| args_to_map(attrs));
    let lowered_children = lower_body(children);

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

/// Wrap a child [`MemberBody`] in a synthetic `Other` member when the
/// dispatcher reaches `lower_member` with a non-`Generic` statement. Never
/// triggered by the current `push_statement` flow; exists so that a future
/// caller that ignores the dispatch contract still produces an inspectable
/// IR rather than panicking, in line with the "lowering is total"
/// invariant.
fn placeholder_member_carrying(body: MemberBody) -> Member {
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
    }
}

fn args_to_map(args: &[Arg]) -> IndexMap<String, Value> {
    let mut map = IndexMap::with_capacity(args.len());
    for arg in args {
        // Last-write-wins on duplicate keys. The M2-PR2 `duplicate` pass
        // walks the surface AST to detect repeats, so the IR's compacted
        // shape here is intentional rather than load-bearing.
        map.insert(arg.key.clone(), arg.value.clone());
    }
    map
}

/// Try to move a label-valued `id` / `class` / `mat_slot` argument into a
/// dedicated [`Member`] field. Accepts only the *textual* shapes
/// ([`Value::Ident`] and [`Value::Str`]) — `@oak_planks` or `foo.bar` are
/// canonical-token / reference values that may not stand in as a label, so
/// they are left in [`IntentState`] for the M2-PR2 type-mismatch pass to
/// flag rather than silently coerced.
///
/// Returns `true` when the value was consumed; `false` keeps the argument
/// in [`IntentState`] (non-label value, or a duplicate key).
fn hoist_label(value: &Value, slot: &mut Option<String>) -> bool {
    if slot.is_some() {
        return false;
    }
    let label = match value {
        Value::Ident(s) | Value::Str(s) => s.clone(),
        _ => return false,
    };
    *slot = Some(label);
    true
}
