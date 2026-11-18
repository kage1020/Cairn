//! Whether a member matches a selector's attributes, in the one grammar a `theme` row and an
//! `edit` share (`spec/materials-themes` "Slots as dependency injection").

use std::collections::{BTreeSet, HashMap};

use indexmap::IndexMap;

use crate::ast::{Value, ValueKind};

use super::member::LabelKey;
use super::{Member, MemberRole, ThemeIr, ValueWithSpan};

/// The attribute that names a `level` the member stands under, not a key of its line.
const LEVEL_ATTR: &str = "level";

/// Keys the theme selectors match on, per keyword; ordered for a stable closed-set note.
pub(crate) type SelectorKeys<'a> = HashMap<&'a str, BTreeSet<&'a str>>;

/// The keys the selectors of `themes` match on, which widen each keyword's vocabulary;
/// `level=` names a `level` the member stands under, not a key of its line.
pub(crate) fn selector_keys(themes: &[ThemeIr]) -> SelectorKeys<'_> {
    let mut keys: SelectorKeys<'_> = HashMap::new();
    for rule in themes.iter().flat_map(|theme| &theme.selectors) {
        keys.entry(rule.keyword.as_str()).or_default().extend(
            rule.attrs
                .keys()
                .map(String::as_str)
                .filter(|key| *key != LEVEL_ATTR),
        );
    }
    keys
}

/// Whether `key`'s value is compared as label text, not as the value a line writes.
pub(crate) fn compares_as_label(key: &str) -> bool {
    key == LEVEL_ATTR || LabelKey::of(key).is_some()
}

/// Whether `member`, standing under the `level`s whose ids are `levels`, has `keyword` and
/// matches every one of `attrs`.
pub(crate) fn selects(
    keyword: &str,
    attrs: &IndexMap<String, ValueWithSpan>,
    member: &Member,
    levels: &[&str],
) -> bool {
    member.role.keyword() == keyword
        && attrs
            .iter()
            .all(|(key, expected)| attr_matches(member, levels, key, &expected.value))
}

fn attr_matches(member: &Member, levels: &[&str], key: &str, expected: &Value) -> bool {
    if key == LEVEL_ATTR {
        return levels.iter().any(|level| value_eq_label(expected, level));
    }
    let Some(label) = LabelKey::of(key) else {
        return member
            .written(key)
            .is_some_and(|actual| actual.value.same_value(expected));
    };
    label
        .read(member)
        .is_some_and(|actual| value_eq_label(expected, actual))
}

fn value_eq_label(expected: &Value, raw: &str) -> bool {
    matches!(&expected.kind,
        ValueKind::Ident(s) | ValueKind::Str(s) if s == raw,
    )
}

/// `levels` with the id of `member` added when it is a `level` that has one, for its children.
pub(crate) fn levels_under<'a>(levels: &[&'a str], member: &'a Member) -> Vec<&'a str> {
    let mut under = levels.to_vec();
    if matches!(member.role, MemberRole::Level)
        && let Some(id) = member.id.as_deref()
    {
        under.push(id);
    }
    under
}
