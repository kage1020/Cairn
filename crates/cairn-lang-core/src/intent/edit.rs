//! Top-level `edit` lines applied to the members of the `struct` and `def` bodies they select
//! (`spec/components-editing-sites` "Editing model").

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{Edit, EditTarget, ValueKind};
use crate::error::Span;

use super::member::args_to_map;
use super::{
    IntentModule, Label, Member, MemberRole, SelectorKeys, ValueWithSpan, levels_under, role_of,
    selector_keys, selects,
};

/// A top-level `edit` line, with the members it selected.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct EditIr {
    /// The members it selects, as written.
    pub target: EditTarget,
    /// The `key=value` pairs after `set`; a repeated key keeps its last value.
    pub set: IndexMap<String, ValueWithSpan>,
    /// The lines of the members it selected, each body's in turn.
    #[serde(skip)]
    pub selected: Vec<Span>,
    /// Byte range of the whole line.
    #[serde(skip)]
    pub span: Span,
}

/// Apply `edits` to `module` in source order, each selecting among the members as written.
pub(super) fn apply_edits(module: &mut IntentModule, edits: &[Edit]) {
    if edits.is_empty() {
        return;
    }
    let as_written = module.clone();
    let widened = selector_keys(&as_written.themes);
    for edit in edits {
        let selected = select(&as_written, &edit.target);
        let set = args_to_map(&edit.set);
        let lines: HashSet<usize> = selected.iter().map(|span| span.start).collect();
        for members in module
            .structs
            .iter_mut()
            .map(|s| &mut s.members)
            .chain(module.defs.iter_mut().map(|d| &mut d.members))
        {
            apply_to(members, &lines, &set, &widened);
        }
        module.edits.push(EditIr {
            target: edit.target.clone(),
            set,
            selected,
            span: edit.span.clone(),
        });
    }
    module.as_written = Some(Box::new(as_written));
}

/// The lines of the members `target` selects in the `struct` and `def` bodies of `module`.
fn select(module: &IntentModule, target: &EditTarget) -> Vec<Span> {
    let mut out = Vec::new();
    // An unknown keyword's own finding carries the repair.
    if matches!(role_of(target.keyword()), MemberRole::Other(_)) {
        return out;
    }
    let attrs = match target {
        EditTarget::Selector { attrs, .. } => Some(args_to_map(attrs)),
        EditTarget::Address { .. } => None,
    };
    let bodies = module
        .structs
        .iter()
        .map(|s| &s.members)
        .chain(module.defs.iter().map(|d| &d.members));
    for members in bodies {
        select_in(members, &[], target, attrs.as_ref(), &mut out);
    }
    out
}

fn select_in(
    members: &[Member],
    levels: &[&str],
    target: &EditTarget,
    attrs: Option<&IndexMap<String, ValueWithSpan>>,
    out: &mut Vec<Span>,
) {
    for member in members {
        // An actuator patch adds to the member it names rather than standing as one.
        if member.selector.is_some() {
            continue;
        }
        let hit = match (target, attrs) {
            (EditTarget::Address { address, .. }, _) => member.address.as_ref() == Some(address),
            (EditTarget::Selector { keyword, .. }, Some(attrs)) => {
                selects(keyword, attrs, member, levels)
            }
            (EditTarget::Selector { .. }, None) => false,
        };
        if hit {
            out.push(member.span.clone());
        }
        let under = levels_under(levels, member);
        select_in(&member.children.members, &under, target, attrs, out);
    }
}

fn apply_to(
    members: &mut [Member],
    lines: &HashSet<usize>,
    set: &IndexMap<String, ValueWithSpan>,
    widened: &SelectorKeys<'_>,
) {
    for member in members {
        if lines.contains(&member.span.start) {
            for (key, value) in set {
                if edit_may_set(&member.role, key, widened) {
                    set_key(member, key, value);
                }
            }
        }
        apply_to(&mut member.children.members, lines, set, widened);
    }
}

/// Whether an edit applies `key=` to a member of `role`: a key its line could write, but not
/// `id=`, which names the member to edits and references alike.
pub(crate) fn edit_may_set(role: &MemberRole, key: &str, widened: &SelectorKeys<'_>) -> bool {
    key != "id"
        && (role
            .accepted_arguments()
            .is_some_and(|accepted| accepted.contains(&key))
            || widened
                .get(role.keyword())
                .is_some_and(|extra| extra.contains(key)))
}

/// Write `key=value` onto `member` where its line would have put it.
fn set_key(member: &mut Member, key: &str, value: &ValueWithSpan) {
    let label = match &value.value.kind {
        ValueKind::Ident(name) | ValueKind::Str(name) => Some(name),
        _ => None,
    };
    match (key, label) {
        ("class", Some(name)) => {
            member.class = Some(name.clone());
            member.arguments.remove(key);
        }
        ("mat_slot", Some(name)) => {
            member.mat_slot = Some(Label {
                name: name.clone(),
                span: value.span.clone(),
            });
            member.arguments.remove(key);
        }
        _ => {
            if key == "class" {
                member.class = None;
            } else if key == "mat_slot" {
                member.mat_slot = None;
            }
            if member.role.state_arguments().contains(&key) {
                member.intent_state.insert(key.to_owned(), value.clone());
            } else {
                member.arguments.insert(key.to_owned(), value.clone());
            }
        }
    }
}
