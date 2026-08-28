//! `arguments` pass — flags every `key=value` whose key is outside the
//! vocabulary of the member's role, and every key in that vocabulary no
//! pass reads yet.
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

use std::collections::{BTreeSet, HashMap};

use crate::intent::{IntentModule, Member};
use crate::suggest::nearest_match;

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
        }
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
    if let Some(suggested) = nearest_match(key, accepted.iter().copied()) {
        notes.push(DiagnosticNote {
            span: None,
            message: format!("did you mean `{suggested}`?"),
        });
    }
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
            DiagnosticNote {
                span: None,
                message: format!("did you mean `{suggested}`?"),
            },
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
