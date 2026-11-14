//! The `edit` pass: an `edit` line that selects no member, and a key it sets that the keyword it
//! selects does not take (`spec/components-editing-sites` "Editing model").

use crate::ast::EditTarget;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::{
    BodyKind, EditIr, IntentModule, MemberRole, SelectorKeys, ValueWithSpan, args_to_map,
    edit_may_set, role_of, selector_keys,
};
use crate::prose::{and_list, selector_text};
use crate::suggest::{did_you_mean_note, nearest_match};

use super::DiagnosticSink;

/// The most addresses an unmatched address's note lists.
const LISTED_ADDRESSES: usize = 8;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    let widened = selector_keys(&ir.themes);
    for edit in &ir.edits {
        let role = role_of(edit.target.keyword());
        // An unknown keyword's own finding carries the repair.
        if matches!(role, MemberRole::Other(_)) {
            continue;
        }
        if edit.selected.is_empty() {
            sink.push(unmatched(ir, edit, &role));
        }
        for (key, value) in &edit.set {
            if !edit_may_set(&role, key, &widened) {
                sink.push(unknown_key(&role, key, value, &widened));
            }
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
    let mut notes = Vec::with_capacity(2);
    if !role.is_read_in(BodyKind::Geometry) {
        notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "a `{keyword}` line stands in a `site` body, and an edit reaches only the members \
                 of `struct` and `def` bodies"
            ),
        });
    }
    notes.push(DiagnosticNote {
        span: None,
        message: match &edit.target {
            EditTarget::Selector { .. } => format!(
                "an edit selects among the members of every `struct` and `def` body as their \
                 lines write them, before any edit applies: check each attribute against the \
                 lines it means, or name one member by its address, such as `{keyword}@front[0]`"
            ),
            EditTarget::Address { .. } => standing_addresses(ir, keyword),
        },
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
    for (_, _, member) in ir.addressed_members() {
        if member.role.keyword() != keyword {
            continue;
        }
        if let Some(address) = &member.address {
            let address = format!("`{address}`");
            if !addresses.contains(&address) {
                addresses.push(address);
            }
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

fn unknown_key(
    role: &MemberRole,
    key: &str,
    value: &ValueWithSpan,
    widened: &SelectorKeys<'_>,
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
    let mut accepted: Vec<&str> = role
        .accepted_arguments()
        .unwrap_or_default()
        .into_iter()
        .filter(|accepted| *accepted != "id")
        .collect();
    if let Some(extra) = widened.get(keyword) {
        let coined: Vec<&str> = extra
            .iter()
            .copied()
            .filter(|k| !accepted.contains(k))
            .collect();
        accepted.extend(coined);
    }
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
            "`{key}=` is not an argument `{keyword}` reads, so no edit of one sets it; the edit \
             sets the rest"
        ),
        notes,
        data: None,
    }
}
