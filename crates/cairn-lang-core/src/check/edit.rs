//! The `edit` pass: an `edit` line that selects no member, and a key it sets that the keyword it
//! selects does not take (`spec/components-editing-sites` "Editing model").

use crate::ast::EditTarget;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::{
    BodyKind, EditIr, IntentModule, MemberRole, SelectorKeys, ValueWithSpan, args_to_map,
    edit_may_set, names_an_unknown_keyword, role_of, selector_keys,
};
use crate::prose::{and_list, selector_text};
use crate::suggest::{did_you_mean_note, nearest_match};

use super::DiagnosticSink;

/// The most addresses an unmatched address's note lists.
const LISTED_ADDRESSES: usize = 8;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    let widened = selector_keys(&ir.themes);
    for edit in &ir.edits {
        // An unknown keyword's own finding carries the repair.
        if names_an_unknown_keyword(&edit.target) {
            continue;
        }
        let role = role_of(edit.target.keyword());
        if edit.selected.is_empty() {
            sink.push(unmatched(ir, edit, &role));
        }
        let refused: Vec<&String> = edit
            .set
            .keys()
            .filter(|key| !edit_may_set(&role, key, &widened))
            .collect();
        let sets_others = refused.len() < edit.set.len();
        for key in refused {
            sink.push(unknown_key(
                &role,
                key,
                &edit.set[key],
                &widened,
                sets_others,
            ));
        }
    }
}

/// The edit's target as its line spells it, its brackets joined into one.
fn target_text(target: &EditTarget) -> String {
    match target {
        EditTarget::Selector { keyword, attrs, .. } => selector_text(keyword, &args_to_map(attrs)),
        EditTarget::Address { address, .. } => address.to_string(),
    }
}

fn unmatched(ir: &IntentModule, edit: &EditIr, role: &MemberRole) -> Diagnostic {
    let keyword = role.keyword();
    let mut notes = Vec::with_capacity(3);
    if !role.is_read_in(BodyKind::Geometry) {
        notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "a `{keyword}` line stands in a `site` body, and an edit reaches only the members \
                 of `struct` and `def` bodies"
            ),
        });
    }
    if matches!(edit.target, EditTarget::Selector { .. }) {
        notes.push(DiagnosticNote {
            span: None,
            message: "an edit selects among the members of every `struct` and `def` body as their \
                      lines write them, before any edit applies: check each attribute against the \
                      lines it means, or name one member by its address"
                .to_owned(),
        });
    }
    notes.push(DiagnosticNote {
        span: None,
        message: standing_addresses(ir, keyword),
    });
    Diagnostic {
        code: DiagnosticCode::EditUnmatched,
        span: edit.target.span().clone(),
        primary: format!(
            "`edit {}` selects no member, so it changes nothing",
            target_text(&edit.target)
        ),
        notes,
        data: None,
    }
}

/// Where the members of `keyword` stand, for an address that names none of them.
fn standing_addresses(ir: &IntentModule, keyword: &str) -> String {
    let mut addresses: Vec<String> = Vec::new();
    for found in ir.addressed_members() {
        if found.member.role.keyword() != keyword {
            continue;
        }
        let address = format!("`{}`", found.address);
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    let more = addresses.len().saturating_sub(LISTED_ADDRESSES);
    addresses.truncate(LISTED_ADDRESSES);
    if more > 0 {
        addresses.push(format!("{more} more"));
    }
    match and_list(&addresses) {
        Some(list) => format!("the `{keyword}` members stand at {list}"),
        None => format!("no `{keyword}` member stands in a `struct` or `def` body"),
    }
}

/// `sets_others` says whether the edit sets some other key, which it applies all the same.
fn unknown_key(
    role: &MemberRole,
    key: &str,
    value: &ValueWithSpan,
    widened: &SelectorKeys<'_>,
    sets_others: bool,
) -> Diagnostic {
    let keyword = role.keyword();
    if key == "id" {
        return Diagnostic {
            code: DiagnosticCode::EditUnknownKey,
            span: value.span.clone(),
            primary: "an edit does not set `id=`".to_owned(),
            notes: vec![DiagnosticNote {
                span: None,
                message: "an `id=` is what edits and references name a member by, so it is \
                          written on the member's own line: move it there"
                    .to_owned(),
            }],
            data: None,
        };
    }
    let mut accepted: Vec<&str> = role.accepted_arguments().unwrap_or_default();
    if let Some(extra) = widened.get(keyword) {
        let coined: Vec<&str> = extra
            .iter()
            .copied()
            .filter(|k| !accepted.contains(k))
            .collect();
        accepted.extend(coined);
    }
    accepted.retain(|accepted| *accepted != "id");
    let mut notes = Vec::with_capacity(2);
    notes.extend(nearest_match(key, accepted.iter().copied()).map(did_you_mean_note));
    notes.push(DiagnosticNote {
        span: None,
        message: format!(
            "an edit of a `{keyword}` sets one of: {}",
            accepted.join(", ")
        ),
    });
    Diagnostic {
        code: DiagnosticCode::EditUnknownKey,
        span: value.span.clone(),
        primary: format!(
            "`{key}=` is not an argument `{keyword}` reads, so no edit of one sets it{}",
            if sets_others {
                "; the edit sets the rest"
            } else {
                ""
            }
        ),
        notes,
        data: None,
    }
}
