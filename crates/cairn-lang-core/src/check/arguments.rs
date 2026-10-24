//! The `arguments` pass: each `key=value` and member selector judged against the vocabulary of
//! the member's role or the header's, and each binding on a known keyword's theme-selector row
//! reported as ignored whatever its key. A theme selector's attribute (`window[tags=...]`) widens
//! a role's vocabulary for its keyword only.

use std::collections::{BTreeSet, HashMap};

use indexmap::IndexMap;

use crate::ast::{Item, Module, ThemeRule, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::{
    IntentModule, Member, MemberRole, SelectorValue, UNIVERSAL_ARGUMENTS, Unread, ValueWithSpan,
    role_of,
};
use crate::prose::or_list;
use crate::suggest::{did_you_mean_note, nearest_match};

use super::DiagnosticSink;

/// Keys the theme selectors match on, per keyword; ordered for a stable closed-set note.
type SelectorKeys<'a> = HashMap<&'a str, BTreeSet<&'a str>>;

pub(super) fn run(module: &Module, ir: &IntentModule, sink: &mut DiagnosticSink) {
    check_selector_bindings(module, sink);
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

/// One `W_IGNORED_ARGUMENT` per selector binding as written; the IR keeps only the last value of a
/// key a row repeats.
fn check_selector_bindings(module: &Module, sink: &mut DiagnosticSink) {
    for item in &module.items {
        let Item::Theme { body, .. } = item else {
            continue;
        };
        for rule in body {
            let ThemeRule::Selector {
                keyword, bindings, ..
            } = rule
            else {
                continue;
            };
            // An unknown keyword's own finding carries the repair.
            if matches!(role_of(keyword), MemberRole::Other(_)) {
                continue;
            }
            for binding in bindings {
                sink.push(unlowered_binding(
                    keyword,
                    &binding.key,
                    &binding.value.span,
                ));
            }
        }
    }
}

/// Keys a `struct` / `def` header may carry, in the closed-set note's order.
///
/// `class=` is from `spec/components-editing-sites` "`def`, the component construct".
const HEADER_ARGUMENTS: &[&str] = &["size", "class"];

/// Keys in [`HEADER_ARGUMENTS`] that no pass reads yet.
const UNREAD_HEADER_ARGUMENTS: &[&str] = &["class"];

/// A `size=` left in `args` is not `WxH`, which `check::type_mismatch` reports instead.
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
    // Deduplicated, or the closed-set note names a word twice.
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
            // Only the role's own keys: the widened set would suggest the key itself.
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
            // A universal key left in the fields failed to hoist, which `check::type_mismatch` or
            // `check::duplicate` reports.
            if !UNIVERSAL_ARGUMENTS.contains(&key.as_str()) {
                sink.push(unread_argument(keyword, key, &value.span, why));
            }
        } else if let Some(finding) = routed_past(member, key, &value.span) {
            sink.push(finding);
        }
    }
    // A label-shaped `mat_slot=` was hoisted out of the fields, so it is judged here.
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
    // A selector filters on a value rather than supplying one, so only the word is judged.
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

/// A `key=value` on the right of a theme selector's arrow, which no pass lowers yet
/// (`spec/materials-themes` "Slots as dependency injection").
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

/// The finding for a key this member's selector argument (`kind=` on a `roof`) routed past.
///
/// `None` also when the selector names no rule, which lowering's `W_DEFERRED_MEMBER` reports.
fn routed_past(member: &Member, key: &str, span: &crate::error::Span) -> Option<Diagnostic> {
    for axis in member.role.conditional_arguments() {
        if !axis.is_conditional(key) {
            continue;
        }
        // An unwritten selector still picks a rule, such as the `place` one that reads `gap=`.
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

fn repair_phrase(selector: &str, value: SelectorValue) -> String {
    match value.ident() {
        Some(ident) => format!("write `{selector}={ident}`"),
        None => format!("drop the `{selector}=`"),
    }
}

fn written_as(selector: &str, value: SelectorValue) -> String {
    match value.ident() {
        Some(ident) => format!("`{selector}={ident}`"),
        None => format!("no `{selector}=` at all"),
    }
}

fn routed_past_argument(
    keyword: &str,
    key: &str,
    span: &crate::error::Span,
    selector: &str,
    chosen: SelectorValue,
    alternatives: &str,
    repair: &str,
) -> Diagnostic {
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

/// Which key-bearing field a finding is about.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Argument,
    /// The member's own `[key=value]`.
    Selector,
    Header,
}

impl Field {
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

/// A selector-widened key one edit from the role's own vocabulary, reported as a typo.
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
