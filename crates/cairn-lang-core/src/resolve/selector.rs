//! Which members a theme selector row matches, and when two rows match the
//! same ones.

use crate::ast::{Value, ValueKind};
use crate::intent::{Member, MemberRole, SelectorRule};

use super::binding::SelectorMatch;

/// Whether two selector rows pick out the same members, in this file and
/// in any other.
///
/// True when the keywords are equal and the attribute maps have the same
/// keys with values [`attr_values_select_alike`] calls interchangeable:
/// `class=small` and `class="small"` name one label, while `side=front` and
/// `side="front"` are two [`ValueKind`]s and select disjoint sets.
///
/// The relation is symmetric and transitive but **not** reflexive: a label
/// key holding a non-label value (`window[id=5]`) is unrelated to itself.
/// That is enough for `check::duplicate`, which compares a new row with one
/// representative per group; swapping the representative for an arbitrary
/// member or short-circuiting the self-comparison would rely on a property
/// this does not have.
///
/// Lives beside the matcher so the answer stays derived from its rule.
pub(crate) fn select_the_same_members(a: &SelectorRule, b: &SelectorRule) -> bool {
    a.keyword == b.keyword
        && a.attrs.len() == b.attrs.len()
        && a.attrs.iter().all(|(key, value)| {
            b.attrs
                .get(key)
                .is_some_and(|other| attr_values_select_alike(key, &value.value, &other.value))
        })
}

/// Whether swapping one selector attribute value for the other leaves
/// [`member_attr_matches`] answering the same for every member under `key`.
///
/// A [`LABEL_ATTRS`] key compares an `Ident` or `Str` by text, as
/// [`value_eq_label`] does; any other key compares by [`ValueKind`] at
/// every depth, lists included, as the matcher does. A non-label value
/// under a label key is `false` accurately rather than conservatively: it
/// matches no member, and `check::type_mismatch` already reports it.
/// `tests/check/duplicate.rs` pins both answers.
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

/// Whether a selector's keyword names this member's role.
///
/// `MemberRole::keyword` is the inverse of `intent::role_of`, including
/// for `Other`, which carries the author's own word.
fn keyword_matches_role(keyword: &str, role: &MemberRole) -> bool {
    role.keyword() == keyword
}

/// Reads the [`Member`] field one label selector attribute filters on.
/// The elided lifetime is the higher-ranked one, so the borrow of the
/// returned label is the borrow of the member it came off.
type LabelField = fn(&Member) -> Option<&str>;

/// The selector attributes lowering lifts out of
/// [`crate::intent::IntentState`] and onto [`Member`]'s own fields, paired
/// with the field each one filters on.
///
/// One table for both readers, [`member_attr_matches`] and
/// [`attr_values_select_alike`], so the duplicate check cannot disagree
/// with the matcher. `check::type_mismatch`'s `LABEL_KEYS` is a deliberate
/// superset, adding `use=` and `theme=`, which are not selector attributes.
const LABEL_ATTRS: [(&str, LabelField); 3] = [
    ("id", |member| member.id.as_deref()),
    ("class", |member| member.class.as_deref()),
    ("mat_slot", |member| {
        member.mat_slot.as_ref().map(|slot| slot.name.as_str())
    }),
];

/// The accessor for `key`, or `None` when `key` is an ordinary
/// `key=value` living in [`crate::intent::IntentState`].
fn label_attr(key: &str) -> Option<LabelField> {
    LABEL_ATTRS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, field)| *field)
}

/// Compare a selector attribute's expected value against the member: a
/// [`LABEL_ATTRS`] key against its field by text, any other against the
/// [`crate::intent::IntentState`] entry by [`ValueKind`].
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
