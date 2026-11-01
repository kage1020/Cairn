//! Which members a theme selector row matches, and when two rows match the
//! same ones.

use crate::ast::{Value, ValueKind};
use crate::intent::{Member, MemberRole, SelectorRule};

use super::binding::SelectorMatch;

/// Whether two selector rows pick out the same members, in this file and in any other.
///
/// Not reflexive: a label key holding a non-label value (`window[id=5]`) is unrelated to itself.
pub(crate) fn select_the_same_members(a: &SelectorRule, b: &SelectorRule) -> bool {
    a.keyword == b.keyword
        && a.attrs.len() == b.attrs.len()
        && a.attrs.iter().all(|(key, value)| {
            b.attrs
                .get(key)
                .is_some_and(|other| attr_values_select_alike(key, &value.value, &other.value))
        })
}

/// Whether swapping `a` for `b` leaves [`member_attr_matches`] answering the same under `key`.
fn attr_values_select_alike(key: &str, a: &Value, b: &Value) -> bool {
    if label_attr(key).is_none() {
        return a.kind == b.kind;
    }
    match (&a.kind, &b.kind) {
        (
            ValueKind::Ident(left) | ValueKind::Str(left),
            ValueKind::Ident(right) | ValueKind::Str(right),
        ) => left == right,
        _ => false,
    }
}

pub(super) fn selector_matches(sel: &SelectorMatch, member: &Member) -> bool {
    if !keyword_matches_role(&sel.keyword, &member.role) {
        return false;
    }
    sel.attrs
        .iter()
        .all(|(key, expected)| member_attr_matches(member, key, &expected.value))
}

fn keyword_matches_role(keyword: &str, role: &MemberRole) -> bool {
    role.keyword() == keyword
}

/// Reads the [`Member`] field one label selector attribute filters on.
type LabelField = fn(&Member) -> Option<&str>;

/// The selector attributes lowering lifts onto [`Member`]'s own fields, with each one's accessor.
const LABEL_ATTRS: [(&str, LabelField); 3] = [
    ("id", |member| member.id.as_deref()),
    ("class", |member| member.class.as_deref()),
    ("mat_slot", |member| {
        member.mat_slot.as_ref().map(|slot| slot.name.as_str())
    }),
];

/// The accessor for `key`, or `None` for a key that stays in [`crate::intent::IntentState`].
fn label_attr(key: &str) -> Option<LabelField> {
    LABEL_ATTRS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, field)| *field)
}

fn member_attr_matches(member: &Member, key: &str, expected: &Value) -> bool {
    let Some(field) = label_attr(key) else {
        return member
            .intent_state
            .get(key)
            .is_some_and(|actual| actual.value.kind == expected.kind);
    };
    field(member).is_some_and(|actual| value_eq_label(expected, actual))
}

fn value_eq_label(expected: &Value, raw: &str) -> bool {
    matches!(&expected.kind,
        ValueKind::Ident(s) | ValueKind::Str(s) if s == raw,
    )
}
