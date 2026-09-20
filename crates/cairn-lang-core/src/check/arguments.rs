//! `arguments` pass — flags every `key=value` whose key is outside the
//! vocabulary of the member's role, every key in that vocabulary no pass
//! reads yet, and every key a sibling argument's value routed past.
//!
//! Walks the Intent IR beside [`super::keyword_allowlist`], which asks the
//! same question one level up. The two do not both fire on a line: a
//! member whose keyword is unknown has no vocabulary to judge its
//! arguments against, so this pass leaves it alone and the keyword's own
//! finding carries the repair.
//!
//! Only `intent_state` is in scope, and the other three fields are covered
//! unevenly rather than fully. `check::positional` reads positionals at
//! every role and depth. A member's own selector (`door[id=front]`) has its
//! keys read in two narrow places — the `door` actuator-patch recogniser in
//! `block_array::lower` and redstone's binding-key walk — and nowhere else,
//! so `window[clas=outer]` is silent. The `-> value` tail is refused by
//! `synth`'s `diag_misplaced_sensor`, which only `cairn synth` reaches.
//!
//! A theme selector widens the vocabulary of the keyword it names. `theme t:
//! window[tags=[a,b]] -> frame=@spruce_wood` makes `tags=` a key something
//! reads — `resolve`'s selector matcher — on a `window` and on nothing else,
//! so the member carrying it is not writing a word that will be read by
//! nothing. That is the whole test this pass applies, and a key the module
//! never selects on fails it however plausible it looks. The reverse
//! direction is already covered: a selector matching no member is
//! `E_THEME_SELECTOR_UNMATCHED`.
//!
//! The widening admits words the module *coins*, and one edit from an
//! existing key is not a coinage. `walls[hieght=3]` beside `walls hieght=3`
//! would otherwise forgive the typo completely — the selector matches, so
//! nothing anywhere says a word — and that is byte for byte the failure
//! this pass exists to end. A widened key near-missing the role's own
//! vocabulary is refused with the suggestion, as though it had never been
//! widened. The cost is that a deliberate tag one edit from a real key is
//! refused too; the tie-break favours catching the typo, and renaming the
//! tag is the escape.
//!
//! The candidate set is the role's, plus the universal keys. `clas=outer`
//! is the case that needs the second half: `class` is hoisted into a
//! dedicated field only when the value is label-shaped, so the typo never
//! reaches the field, and a suggestion drawn from the role's own arguments
//! could not offer the word the author meant.
//!
//! The third finding is the vocabulary's second axis. A key the role's list
//! contains is still read by nothing when a sibling argument chose a
//! lowering rule that does not consult it — `roof kind=gable
//! slope_to=front` is the instance, and it built a roof that ignored the
//! direction exactly the way a misspelled key did. The message names both
//! repair sites, because which of the two arguments the author meant is not
//! this pass's to decide; why that makes it a warning where an unknown key
//! is a refusal is argued once, on [`DiagnosticCode::severity`].
//!
//! Where the *selector* names no rule — a word the dispatch does not know,
//! a value of another shape, or, on an axis with no absent arm, not written
//! at all — nothing is reported here. That member lowers to nothing and the
//! pass that tried to draw it says so with `W_DEFERRED_MEMBER`, which is
//! the same repair; a finding here would bill it twice. It is worth being
//! exact about which stream that is: the deferral is raised by block-array
//! lowering, so a `cairn check` with no `--edition` / `--target` runs none
//! of it and reports nothing at all on such a line. Pinning one of the two
//! and dropping the other would be the wrong trade — the case the author
//! most needs told is the one where the member *does* build.

use std::collections::{BTreeSet, HashMap};

use crate::ast::ValueKind;
use crate::intent::{IntentModule, Member, SelectorValue};
use crate::prose::or_list;
use crate::suggest::{did_you_mean_note, nearest_match};

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

/// Keys the module's theme selectors match on, per keyword they name.
///
/// `BTreeSet` rather than a hash set so the closed-set note reads in a
/// stable order whatever the source did.
type SelectorKeys<'a> = HashMap<&'a str, BTreeSet<&'a str>>;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    let selected = selector_keys(ir);
    for s in &ir.structs {
        walk(&s.members, &selected, sink);
    }
    for d in &ir.defs {
        walk(&d.members, &selected, sink);
    }
    for s in &ir.sites {
        walk(&s.placements, &selected, sink);
    }
}

/// Every attribute key a theme selector filters on, under the keyword that
/// selector names.
///
/// Per keyword rather than module-wide: `window[tags=...]` says something
/// reads `tags=` on a window, and says nothing at all about a `door`.
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
    // The keyword is the repair; its arguments answer to a vocabulary that
    // does not exist.
    let Some(own) = member.role.accepted_arguments() else {
        return;
    };
    let keyword = member.role.keyword();
    let widened = selected.get(keyword);
    // Deduplicated, because a key can be both in the role's vocabulary and
    // selected on, and a closed set naming one word twice reads as two
    // different things.
    let mut accepted = own.clone();
    if let Some(extra) = widened {
        accepted.extend(extra.iter().copied().filter(|k| !own.contains(k)));
    }
    for (key, value) in &member.intent_state.fields {
        let coined = widened.is_some_and(|extra| extra.contains(key.as_str()));
        if !accepted.contains(&key.as_str()) {
            sink.push(unknown_argument(keyword, key, &value.span, &accepted));
        } else if coined && !own.contains(&key.as_str()) {
            // Widened by a selector. Legal unless it is a near-miss of a
            // word the role already has, which is a typo written twice
            // rather than a word the module coined. Candidates are the
            // role's own vocabulary — feeding the widened set in would let
            // the key suggest itself.
            if let Some(suggested) = nearest_match(key, own.iter().copied()) {
                sink.push(coined_near_miss(keyword, key, &value.span, suggested, &own));
            }
        } else if member.role.unread_arguments().contains(&key.as_str()) {
            // A key the specification defines and nothing reads — unless
            // the module selects on it, in which case something does, and
            // "the value was ignored" would be false advice that breaks a
            // working theme.
            if !coined {
                sink.push(unread_argument(keyword, key, &value.span));
            }
        } else if !coined {
            // A key some lowering rule reads, on a member whose sibling
            // argument picked a rule that does not. The selector check is
            // the same one the unread branch makes: a key the module
            // selects on is read whatever the lowering does with it.
            if let Some(finding) = routed_past(member, key, &value.span) {
                sink.push(finding);
            }
        }
    }
}

/// A key in the role's vocabulary that this member's own selector routed
/// past, if it has one.
///
/// `None` covers three different silences, and all three are wanted. The
/// key may be conditional on no axis, which is the ordinary case. The
/// selector may name a rule that does read it, which is the working source.
/// Or the selector may name no rule at all — a word the dispatch does not
/// know, a value of another shape, or, on an axis with no
/// [`SelectorValue::Absent`] arm, not written at all — and then the member
/// lowers to nothing and the `W_DEFERRED_MEMBER` from the pass that tried
/// to draw it carries the whole repair.
fn routed_past(member: &Member, key: &str, span: &crate::error::Span) -> Option<Diagnostic> {
    for axis in member.role.conditional_arguments() {
        if !axis.is_conditional(key) {
            continue;
        }
        // Absent is a value here rather than a reason to stop: an axis may
        // have a rule for the selector not being written, and on a `place`
        // that rule is the one that reads `gap=`.
        let written = match member.intent_state.get(axis.selector) {
            None => None,
            Some(value) => match &value.value.kind {
                ValueKind::Ident(name) => Some(name.as_str()),
                // Written as something that is not an identifier, so it
                // names no rule however it is spelled.
                _ => continue,
            },
        };
        let Some(arm) = axis.arm(written) else {
            continue;
        };
        if arm.reads.contains(&key) {
            continue;
        }
        // Both renderings are built here, where the arms that read the key
        // are still in hand, so the message has no invariant left to assert
        // about a list being non-empty.
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

/// How the line reads today, for the clause that quotes it back.
fn written_as(selector: &str, value: SelectorValue) -> String {
    match value.ident() {
        Some(ident) => format!("`{selector}={ident}`"),
        None => format!("no `{selector}=` at all"),
    }
}

/// Build the finding for a key the selected rule does not read.
///
/// `alternatives` and `repair` arrive already rendered because the arms
/// that read the key are the caller's to walk; this function turns one arm
/// and one key into the two sentences the author sees.
fn routed_past_argument(
    keyword: &str,
    key: &str,
    span: &crate::error::Span,
    selector: &str,
    chosen: SelectorValue,
    alternatives: &str,
    repair: &str,
) -> Diagnostic {
    // Both repair sites are named because either argument may be the
    // mistake — which is the reason `DiagnosticCode::severity` gives for
    // this being a warning where an unknown key is a refusal.
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

fn unknown_argument(
    keyword: &str,
    key: &str,
    span: &crate::error::Span,
    accepted: &[&str],
) -> Diagnostic {
    // Suggestion first, closed set second — the same order
    // `E_UNKNOWN_KEYWORD` uses, so a reader who has seen one knows where
    // to look in the other.
    let mut notes = Vec::with_capacity(2);
    notes.extend(nearest_match(key, accepted.iter().copied()).map(did_you_mean_note));
    notes.push(DiagnosticNote {
        span: None,
        message: format!("expected one of: {}", accepted.join(", ")),
    });
    Diagnostic {
        code: DiagnosticCode::UnknownArgument,
        span: span.clone(),
        primary: format!("`{key}=` is not an argument `{keyword}` reads"),
        notes,
        data: None,
    }
}

/// A selector-widened key that is one edit from the role's own vocabulary.
///
/// Reported exactly as if the selector were not there, because a word a
/// module coins is a word it chose, and this one is a word it nearly typed.
fn coined_near_miss(
    keyword: &str,
    key: &str,
    span: &crate::error::Span,
    suggested: &str,
    own: &[&str],
) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::UnknownArgument,
        span: span.clone(),
        primary: format!("`{key}=` is not an argument `{keyword}` reads"),
        notes: vec![
            did_you_mean_note(suggested),
            DiagnosticNote {
                span: None,
                message: format!(
                    "a `{keyword}[{key}=...]` selector would make this a key of its own, but \
                     one edit from `{suggested}` reads as a typo written twice",
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

fn unread_argument(keyword: &str, key: &str, span: &crate::error::Span) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::IgnoredArgument,
        span: span.clone(),
        primary: format!(
            "`{key}=` is an argument `{keyword}` takes and no pass reads yet; the value was ignored",
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "the member is built without it — remove the argument, or keep it and \
                      expect no effect until the lowering rule lands"
                .to_owned(),
        }],
        data: None,
    }
}
