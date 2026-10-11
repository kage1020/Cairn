//! Which members a theme selector row matches, and when two rows match the
//! same ones.

use crate::ast::{Value, ValueKind};
use crate::intent::{Member, MemberRole, SelectorRule};

use super::binding::SelectorMatch;

/// Whether two selector rows pick out the same members — in this file and
/// in any other.
///
/// [`selector_matches`] cannot tell two rows apart when they carry the same
/// keyword (the comparison is string equality against `MemberRole::keyword`)
/// and attribute maps with the same keys whose values are interchangeable
/// under [`member_attr_matches`]. Interchangeability is per key rather than
/// per value: `class=small` and `class="small"` name one label and select
/// alike, while `side=front` and `side="front"` are two [`ValueKind`]s and
/// select disjoint sets.
///
/// The relation is symmetric and transitive but **not** reflexive: a
/// label key holding a non-label value takes the `false` arm of
/// [`attr_values_select_alike`], so `select_the_same_members(r, r)` is
/// false for `window[id=5]`. A partial equivalence is all the grouping in
/// `check::duplicate` needs — symmetry and transitivity are what let a new
/// row be compared against one representative instead of every row in the
/// group, and a row unrelated to itself opens a group nothing ever joins,
/// which reports nothing. A reader who takes "equivalence" at face value
/// and optimises on it (swapping the representative for an arbitrary
/// member, short-circuiting the self-comparison) would be relying on a
/// property this does not have.
///
/// Lives beside the matcher rather than in the pass that reports duplicate
/// rows, so the answer stays derived from the rule it is about.
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
/// The split is [`LABEL_ATTRS`], the same table that function reads.
/// A label attribute goes through [`value_eq_label`], which takes an
/// `Ident` or a `Str` carrying the same text; every other key is compared
/// against an [`crate::intent::IntentState`] entry by [`ValueKind`], where
/// the two spellings are different values.
/// `selector::value_form_matters_exactly_where_the_matcher_says_it_does` in
/// `tests/check/duplicate.rs` pins that pair of answers from the
/// outside, by checking `E_THEME_SELECTOR_UNMATCHED` alongside the
/// finding.
///
/// The `false` for a non-label value under a label key is the accurate
/// answer rather than a conservative one: [`value_eq_label`] rejects such a
/// value for every member, so neither row binds anything the other could
/// take over. Those rows are already an error by a different scope —
/// `check::type_mismatch` covers the same three keys and reports
/// `E_TYPE_MISMATCH_LABEL` on each of them — so the gap says nothing that
/// goes unsaid. `selector::a_label_key_holding_a_non_label_value_pairs_with_nothing`
/// in `tests/check/duplicate.rs` pins the pairing.
///
/// The [`ValueKind`] comparison is by value at every depth, including
/// inside a `ValueKind::List`: [`Value`]'s equality is its kind's, so two
/// lists spelled identically on two lines select alike. That is the same
/// answer [`member_attr_matches`] gives, which is what makes "these two
/// rows select the same members" and "this row selects this member" agree
/// about what a value is.
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
/// `MemberRole::keyword` is the inverse of `intent::role_of`, so the
/// comparison is the identity it looks like — including the `Other`
/// case, where the role carries the author's own word.
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
/// One table rather than one list per reader. [`member_attr_matches`] wants
/// the accessor and [`attr_values_select_alike`] wants the membership; a key
/// added to only one of them would let the duplicate check disagree with the
/// matcher it is derived from. (`check::type_mismatch`'s `LABEL_KEYS` is a
/// third list and a deliberate superset — it also covers `use=` and
/// `theme=`, which are not selector attributes at all.)
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

/// Compare a selector attribute's expected value against the corresponding
/// member field. A [`LABEL_ATTRS`] key is compared string-vs-`Ident`/`Str`;
/// everything else is a generic `key=value` arg that lives in
/// [`crate::intent::IntentState`] and compares by [`ValueKind`].
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
