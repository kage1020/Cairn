//! Top-level `edit` lines applied to the members of the `struct` and `def` bodies they select
//! (`spec/components-editing-sites` "Editing model").

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::Serialize;

use crate::address::MemberAddress;
use crate::ast::{Edit, EditTarget};
use crate::error::Span;

use super::member::{Destination, LabelKey, args_to_map, destination};
use super::{
    IntentModule, Member, MemberRole, SelectorKeys, ValueWithSpan, levels_under, patched_actuator,
    role_of, selector_keys, selects,
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

/// Whether `target` names a keyword that is no member keyword, at any step of an address.
pub(crate) fn names_an_unknown_keyword(target: &EditTarget) -> bool {
    target
        .keywords()
        .iter()
        .any(|(keyword, _)| matches!(role_of(keyword), MemberRole::Other(_)))
}

/// What an edit's target matches a member by.
enum Matcher<'a> {
    /// The address the member carries.
    Address(&'a MemberAddress),
    /// A selector's keyword and attributes.
    Selector {
        keyword: &'a str,
        attrs: IndexMap<String, ValueWithSpan>,
    },
}

impl Matcher<'_> {
    /// Whether it matches `member`, standing under the `level`s whose ids are `levels`.
    fn matches(&self, member: &Member, levels: &[&str]) -> bool {
        match self {
            Self::Address(address) => member.address.as_ref() == Some(*address),
            Self::Selector { keyword, attrs } => selects(keyword, attrs, member, levels),
        }
    }
}

/// The lines of the members `target` selects in the `struct` and `def` bodies of `module`.
fn select(module: &IntentModule, target: &EditTarget) -> Vec<Span> {
    let mut out = Vec::new();
    // An unknown keyword's own finding carries the repair.
    if names_an_unknown_keyword(target) {
        return out;
    }
    let matcher = match target {
        EditTarget::Selector { keyword, attrs, .. } => Matcher::Selector {
            keyword,
            attrs: args_to_map(attrs),
        },
        EditTarget::Address { address, .. } => Matcher::Address(address),
    };
    let bodies = module
        .structs
        .iter()
        .map(|s| &s.members)
        .chain(module.defs.iter().map(|d| &d.members));
    for members in bodies {
        select_in(members, &[], &matcher, &mut out);
    }
    out
}

fn select_in(members: &[Member], levels: &[&str], matcher: &Matcher<'_>, out: &mut Vec<Span>) {
    for member in members {
        // An actuator patch stands nowhere: it adds to the member its selector picks.
        if patched_actuator(member).is_some() {
            continue;
        }
        if matcher.matches(member, levels) {
            out.push(member.span.clone());
        }
        let under = levels_under(levels, member);
        select_in(&member.children.members, &under, matcher, out);
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

/// Write `key=value` onto `member` where its line would have put it, in place of what the line
/// wrote under `key`.
fn set_key(member: &mut Member, key: &str, value: &ValueWithSpan) {
    match destination(&member.role, key, value) {
        Destination::Label(label_key, label) => {
            label_key.write(member, label);
            member.arguments.remove(key);
        }
        Destination::State => member.intent_state.insert(key.to_owned(), value.clone()),
        Destination::Argument => {
            // A value that is no label leaves the key's own field empty, as it does on a line.
            if let Some(label_key) = LabelKey::of(key) {
                label_key.clear(member);
            }
            member.arguments.insert(key.to_owned(), value.clone());
        }
    }
}
