//! Which members a theme selector row matches, and when two rows match the
//! same ones.

use crate::ast::{Value, ValueKind};
use crate::intent::{Member, SelectorRule, compares_as_label, selects};

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

/// Whether swapping `a` for `b` leaves a selector's match answering the same under `key`.
fn attr_values_select_alike(key: &str, a: &Value, b: &Value) -> bool {
    if !compares_as_label(key) {
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

/// Whether `sel` matches `member`, standing under the `level`s whose ids are `levels`.
pub(super) fn selector_matches(sel: &SelectorMatch, member: &Member, levels: &[&str]) -> bool {
    selects(&sel.keyword, &sel.attrs, member, levels)
}
