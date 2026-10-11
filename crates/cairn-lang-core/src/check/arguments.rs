//! `arguments` pass: flags every `key=value` outside the vocabulary of the
//! member's role, every accepted key no pass reads, every key a sibling
//! argument's value routed past, and every binding on a theme selector. A
//! member's own `[key=value]` selector answers to the same vocabulary. A
//! `struct` / `def` header answers to its own, `size=` and `class=`, and
//! gets only the unknown-key and unread-key findings.
//!
//! A member whose keyword is unknown is left to
//! [`super::keyword_allowlist`]: it has no vocabulary to judge against.
//! Positionals belong to `check::positional` and the `-> value` tail to
//! [`super::binding`].
//!
//! The question asked of a key is only whether something in the module reads
//! the word, so `clas=` is refused and `id=` accepted in an argument and a
//! selector alike, whatever a later pass decides a selector key means. A theme
//! selector `window[tags=...]` makes `tags=` a word something reads
//! (`resolve`'s matcher) on a `window` and nothing else. A widened key one
//! edit from the role's own vocabulary is still refused, because
//! `walls[hieght=3]` beside `walls hieght=3` would otherwise forgive the
//! typo everywhere; a deliberate tag that close has to be renamed. Widening
//! admits a word without making its value do anything, so a key no pass
//! reads stays reported when a selector matches on it.
//!
//! Suggestions draw on the role's keys plus the universal ones: `class` is
//! hoisted only when its value is label-shaped, so `clas=outer` never
//! reaches the field and the role's own list could not offer `class`.

use std::collections::{BTreeSet, HashMap};

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::{
    IntentModule, Member, MemberRole, SelectorValue, UNIVERSAL_ARGUMENTS, Unread, ValueWithSpan,
    role_of,
};
use crate::prose::or_list;
use crate::suggest::{did_you_mean_note, nearest_match};

use super::DiagnosticSink;

/// Keys the module's theme selectors match on, per keyword they name.
///
/// A `BTreeSet` keeps the closed-set note in a stable order.
type SelectorKeys<'a> = HashMap<&'a str, BTreeSet<&'a str>>;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for theme in &ir.themes {
        for rule in &theme.selectors {
            // An unknown keyword's own finding carries the repair.
            if matches!(role_of(&rule.keyword), MemberRole::Other(_)) {
                continue;
            }
            for (key, value) in &rule.bindings {
                sink.push(unlowered_binding(&rule.keyword, key, &value.span));
            }
        }
    }
    let selected = selector_keys(ir);
    for s in &ir.structs {
        check_header("struct", &s.args, sink);
        walk(&s.members, &selected, sink);
    }
    for d in &ir.defs {
        check_header("def", &d.args, sink);
        walk(&d.members, &selected, sink);
    }
    for s in &ir.sites {
        walk(&s.placements, &selected, sink);
    }
}

/// Keys a `struct` / `def` header line may carry, in the order the
/// closed-set note lists them.
///
/// `class=` is here because `spec/components-editing-sites` "`def`, the
/// component construct" writes it on a header.
const HEADER_ARGUMENTS: &[&str] = &["size", "class"];

/// Keys in [`HEADER_ARGUMENTS`] that no pass reads yet: carried into
/// `StructIr::args` / `DefIr::args` and never consulted.
const UNREAD_HEADER_ARGUMENTS: &[&str] = &["class"];

/// Judge the `key=value` arguments of a `struct` / `def` header line
/// against [`HEADER_ARGUMENTS`].
///
/// No theme selector widens it, since a selector names a member keyword. A
/// `size=` still in `args` has a value that is not a `WxH` literal, which
/// `check::type_mismatch` reports, so the key itself is accepted here.
fn check_header(keyword: &str, args: &IndexMap<String, ValueWithSpan>, sink: &mut DiagnosticSink) {
    for (key, value) in args {
        if !HEADER_ARGUMENTS.contains(&key.as_str()) {
            sink.push(unknown_key(
                Field::Header,
                keyword,
                key,
                &value.span,
                HEADER_ARGUMENTS,
            ));
        } else if UNREAD_HEADER_ARGUMENTS.contains(&key.as_str()) {
            sink.push(unread_header_argument(keyword, key, &value.span));
        }
    }
}

/// Every attribute key a theme selector filters on, under the keyword that
/// selector names: `window[tags=...]` says nothing about a `door`.
fn selector_keys(ir: &IntentModule) -> SelectorKeys<'_> {
    let mut keys: SelectorKeys<'_> = HashMap::new();
    for theme in &ir.themes {
        for rule in &theme.selectors {
            let entry = keys.entry(rule.keyword.as_str()).or_default();
            for key in rule.attrs.keys() {
                entry.insert(key.as_str());
            }
        }
    }
    keys
}

fn walk(members: &[Member], selected: &SelectorKeys<'_>, sink: &mut DiagnosticSink) {
    for m in members {
        check_member(m, selected, sink);
        walk(&m.children.members, selected, sink);
    }
}

fn check_member(member: &Member, selected: &SelectorKeys<'_>, sink: &mut DiagnosticSink) {
    // An unknown keyword's own finding carries the repair.
    let Some(own) = member.role.accepted_arguments() else {
        return;
    };
    let keyword = member.role.keyword();
    let widened = selected.get(keyword);
    // Deduplicated: a closed set naming one word twice reads as two words.
    let mut accepted = own.clone();
    if let Some(extra) = widened {
        accepted.extend(extra.iter().copied().filter(|k| !own.contains(k)));
    }
    for (key, value) in &member.intent_state.fields {
        let coined = widened.is_some_and(|extra| extra.contains(key.as_str()));
        if !accepted.contains(&key.as_str()) {
            sink.push(unknown_key(
                Field::Argument,
                keyword,
                key,
                &value.span,
                &accepted,
            ));
        } else if coined && !own.contains(&key.as_str()) {
            // Candidates leave out the widened set, which would let the key
            // suggest itself.
            if let Some(suggested) = nearest_match(key, own.iter().copied()) {
                sink.push(coined_near_miss(
                    Field::Argument,
                    keyword,
                    key,
                    &value.span,
                    suggested,
                    &own,
                ));
            }
        } else if let Some(why) = member.role.unread_argument(key) {
            // A universal key still in the fields is one `intent::lower`
            // could not hoist, which `check::type_mismatch` or
            // `check::duplicate` reports; reporting it here too would put
            // "write a label" and "drop it" on one span. The hoisted
            // occurrence is judged below.
            if !UNIVERSAL_ARGUMENTS.contains(&key.as_str()) {
                sink.push(unread_argument(keyword, key, &value.span, why));
            }
        } else if let Some(finding) = routed_past(member, key, &value.span) {
            sink.push(finding);
        }
    }
    // A label-shaped `mat_slot=` was hoisted out of the fields, so it is
    // judged unread here. This fires exactly where `Member::read_slot`
    // answers `None`; the table is asked directly because the finding needs
    // the reason that answer drops.
    if let Some(slot) = &member.mat_slot
        && let Some(why) = member.role.unread_argument("mat_slot")
    {
        let mut finding = unread_argument(keyword, "mat_slot", &slot.span, why);
        finding.notes.push(DiagnosticNote {
            span: None,
            message: "the slot name is not looked up in the theme, so a misspelling of it is \
                      not caught"
                .to_owned(),
        });
        sink.push(finding);
    }
    // Only the two branches about the word apply to the member's own
    // `[key=value]`: a selector filters on a value rather than supplying one.
    for (key, value) in member.selector.iter().flatten() {
        if !accepted.contains(&key.as_str()) {
            sink.push(unknown_key(
                Field::Selector,
                keyword,
                key,
                &value.span,
                &accepted,
            ));
        } else if widened.is_some_and(|extra| extra.contains(key.as_str()))
            && !own.contains(&key.as_str())
            && let Some(suggested) = nearest_match(key, own.iter().copied())
        {
            sink.push(coined_near_miss(
                Field::Selector,
                keyword,
                key,
                &value.span,
                suggested,
                &own,
            ));
        }
    }
}

/// A `key=value` on the right of a theme selector's arrow.
///
/// No lowering rule reads a selector's bindings yet
/// (`spec/materials-themes` "Slots as dependency injection"), so every
/// binding on a row whose keyword names a role is reported, whatever its key
/// or value, with the code `spec/lint` "Error vs warning" gives an unreached
/// key. Neither key nor value is judged: there is no vocabulary of binding
/// keys until a pass reads one. A key written twice in a row is reported
/// once, for its last value, since the bindings are a map by then.
fn unlowered_binding(keyword: &str, key: &str, span: &crate::error::Span) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::IgnoredArgument,
        span: span.clone(),
        primary: format!(
            "`{key}=` is bound by a theme selector on `{keyword}`, and no pass lowers a \
             selector's bindings yet; the value was ignored",
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "the build is the same with this binding or without it; delete it, or \
                      the whole row if it binds nothing else, or keep it and expect no effect \
                      until selector bindings are lowered"
                .into(),
        }],
        data: None,
    }
}

/// A key in the role's vocabulary that this member's own selector argument
/// (`kind=` on a `roof`) routed past, if it has one.
///
/// `None` also when the selector names no rule at all: an unknown word, a
/// value of another shape, or no value on an axis without a
/// [`SelectorValue::Absent`] arm. That member lowers to nothing and
/// lowering's `W_DEFERRED_MEMBER` carries the repair, which a finding here
/// would bill twice. An unpinned `cairn check` runs no lowering and so says
/// nothing on such a line; the member that does build is the one the author
/// most needs told about.
fn routed_past(member: &Member, key: &str, span: &crate::error::Span) -> Option<Diagnostic> {
    for axis in member.role.conditional_arguments() {
        if !axis.is_conditional(key) {
            continue;
        }
        // An unwritten selector can still pick a rule: on a `place` it is
        // the one that reads `gap=`.
        let written = match member.intent_state.get(axis.selector) {
            None => None,
            Some(value) => match &value.value.kind {
                ValueKind::Ident(name) => Some(name.as_str()),
                _ => continue,
            },
        };
        let Some(arm) = axis.arm(written) else {
            continue;
        };
        if arm.reads.contains(&key) {
            continue;
        }
        // Rendered here, where an empty list of reading arms can still be
        // skipped, so the message builder has nothing to assert.
        let reading = axis.read_when(key);
        let alternatives: Vec<String> = reading
            .iter()
            .map(|value| written_as(axis.selector, *value))
            .collect();
        let repairs: Vec<String> = reading
            .iter()
            .map(|value| repair_phrase(axis.selector, *value))
            .collect();
        let (Some(alternatives), Some(repair)) = (or_list(&alternatives), or_list(&repairs)) else {
            continue;
        };
        return Some(routed_past_argument(
            member.role.keyword(),
            key,
            span,
            axis.selector,
            arm.value,
            &alternatives,
            &repair,
        ));
    }
    None
}

/// How the author reaches a rule that reads the key: `` write `kind=shed` ``
/// for a value, `` drop the `at=` `` for the rule that answers to the
/// selector not being written at all.
fn repair_phrase(selector: &str, value: SelectorValue) -> String {
    match value.ident() {
        Some(ident) => format!("write `{selector}={ident}`"),
        None => format!("drop the `{selector}=`"),
    }
}

/// How the line writes the selector, for the clause that quotes it back.
fn written_as(selector: &str, value: SelectorValue) -> String {
    match value.ident() {
        Some(ident) => format!("`{selector}={ident}`"),
        None => format!("no `{selector}=` at all"),
    }
}

/// Build the finding for a key the selected rule does not read, from the
/// caller's rendered `alternatives` and `repair`.
fn routed_past_argument(
    keyword: &str,
    key: &str,
    span: &crate::error::Span,
    selector: &str,
    chosen: SelectorValue,
    alternatives: &str,
    repair: &str,
) -> Diagnostic {
    // Either argument may be the mistake, so both repairs are named; it is
    // also why `DiagnosticCode::severity` makes this a warning.
    let kept = match chosen.ident() {
        Some(ident) => format!("`{ident}` {keyword}"),
        None => format!("`{keyword}`"),
    };
    Diagnostic {
        code: DiagnosticCode::IgnoredArgument,
        span: span.clone(),
        primary: format!(
            "`{key}=` is an argument `{keyword}` reads only with {alternatives}, and this one \
             is {}; the value was ignored",
            written_as(selector, chosen),
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "either argument may be the repair — {repair} to have the `{key}=` read, or \
                 drop `{key}=` and keep the {kept} the line already asks for",
            ),
        }],
        data: None,
    }
}

/// Which key-bearing field a finding is about: a member's two, or the
/// `struct` / `def` header line.
///
/// All three share a code and notes; only the sentence naming the field to
/// edit differs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    /// A `key=value` written after the keyword.
    Argument,
    /// The member's own `[key=value]`.
    Selector,
    /// A `key=value` on a `struct` / `def` header line.
    Header,
}

impl Field {
    /// How a message names a key written in this field that nothing reads.
    fn nothing_reads(self, keyword: &str, key: &str) -> String {
        match self {
            Self::Argument => format!("`{key}=` is not an argument `{keyword}` reads"),
            Self::Selector => format!("`{key}=` is not an attribute a `{keyword}` carries"),
            Self::Header => format!("`{key}=` is not an argument a `{keyword}` header reads"),
        }
    }
}

fn unknown_key(
    field: Field,
    keyword: &str,
    key: &str,
    span: &crate::error::Span,
    accepted: &[&str],
) -> Diagnostic {
    // Suggestion before closed set, the order `E_UNKNOWN_KEYWORD` uses.
    let mut notes = Vec::with_capacity(2);
    notes.extend(nearest_match(key, accepted.iter().copied()).map(did_you_mean_note));
    notes.push(DiagnosticNote {
        span: None,
        message: format!("expected one of: {}", accepted.join(", ")),
    });
    Diagnostic {
        code: DiagnosticCode::UnknownArgument,
        span: span.clone(),
        primary: field.nothing_reads(keyword, key),
        notes,
        data: None,
    }
}

/// A selector-widened key that is one edit from the role's own vocabulary,
/// reported as if the selector were not there.
///
/// The middle note names the widening `theme` row as one that exists.
/// Phrased as something to add, it would advise writing the selector, or on
/// the member's own bracket, the very text being reported.
fn coined_near_miss(
    field: Field,
    keyword: &str,
    key: &str,
    span: &crate::error::Span,
    suggested: &str,
    own: &[&str],
) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::UnknownArgument,
        span: span.clone(),
        primary: field.nothing_reads(keyword, key),
        notes: vec![
            did_you_mean_note(suggested),
            DiagnosticNote {
                span: None,
                message: format!(
                    "a `{keyword}[{key}=...]` row in a `theme` is what makes this a key of its \
                     own, but one edit from `{suggested}` reads as a typo written twice",
                ),
            },
            DiagnosticNote {
                span: None,
                message: format!("expected one of: {}", own.join(", ")),
            },
        ],
        data: None,
    }
}

/// A key the role accepts and nothing on it reads, worded by [`Unread`].
///
/// An unreached key waits on a lowering rule, so keeping it is the author's
/// choice; an inapplicable one waits on nothing, so the note asks for its
/// removal.
fn unread_argument(keyword: &str, key: &str, span: &crate::error::Span, why: Unread) -> Diagnostic {
    let (primary, note) = match why {
        Unread::Unreached => (
            format!(
                "`{key}=` is an argument `{keyword}` takes and no pass reads yet; the value was \
                 ignored",
            ),
            "the member is built without it — remove the argument, or keep it and expect no \
             effect until the lowering rule lands"
                .to_owned(),
        ),
        Unread::Inapplicable => (
            format!(
                "`{key}=` is accepted on every member, and nothing a `{keyword}` builds reads it; \
                 the value was ignored",
            ),
            format!(
                "the `{keyword}` is built the same with it or without it — remove the argument"
            ),
        ),
    };
    Diagnostic {
        code: DiagnosticCode::IgnoredArgument,
        span: span.clone(),
        primary,
        notes: vec![DiagnosticNote {
            span: None,
            message: note,
        }],
        data: None,
    }
}

/// A key the specification writes on a `struct` / `def` header and no pass
/// reads yet — [`UNREAD_HEADER_ARGUMENTS`].
fn unread_header_argument(keyword: &str, key: &str, span: &crate::error::Span) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::IgnoredArgument,
        span: span.clone(),
        primary: format!(
            "`{key}=` is an argument a `{keyword}` header takes and no pass reads yet; the value \
             was ignored",
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "the `{keyword}` is built without it — remove the argument, or keep it and \
                 expect no effect until a pass reads it"
            ),
        }],
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{HEADER_ARGUMENTS, UNREAD_HEADER_ARGUMENTS};

    /// Otherwise `check_header` refuses the key as unknown before the
    /// unread branch sees it.
    #[test]
    fn every_unread_header_argument_is_in_the_header_vocabulary() {
        for key in UNREAD_HEADER_ARGUMENTS {
            assert!(
                HEADER_ARGUMENTS.contains(key),
                "`{key}` is called unread but the header vocabulary does not list it",
            );
        }
    }
}
