//! A `slot` row that binds a member form, a bare word, takes that form's `key=value` arguments
//! (`spec/versioning-editions` "Writing an alternative").

use cairn_lang_core::ast::{Item, ThemeRule, ValueKind};
use cairn_lang_core::parse;

/// The bound value and the `key=` arguments of the one row of `theme t:`.
fn slot_row(row: &str) -> (ValueKind, Vec<(String, ValueKind, String)>) {
    let source = format!("theme t:\n  {row}\n");
    let module = parse(&source).unwrap_or_else(|err| panic!("{row:?}: {err}"));
    let Item::Theme { body: rules, .. } = &module.items[0] else {
        panic!("expected a theme, got {:?}", module.items[0]);
    };
    let ThemeRule::Slot {
        value, arguments, ..
    } = &rules[0]
    else {
        panic!("expected a slot row, got {:?}", rules[0]);
    };
    let arguments = arguments
        .iter()
        .map(|arg| {
            (
                arg.key.clone(),
                arg.value.kind.clone(),
                source[arg.span.clone()].to_owned(),
            )
        })
        .collect();
    (value.kind.clone(), arguments)
}

#[test]
fn a_member_form_row_keeps_its_arguments_in_source_order() {
    assert_eq!(
        slot_row("slot floating_text -> text_display scale=2.0 billboard=center"),
        (
            ValueKind::Ident("text_display".to_owned()),
            vec![
                (
                    "scale".to_owned(),
                    ValueKind::Decimal("2.0".to_owned()),
                    "scale=2.0".to_owned(),
                ),
                (
                    "billboard".to_owned(),
                    ValueKind::Ident("center".to_owned()),
                    "billboard=center".to_owned(),
                ),
            ],
        ),
    );
    assert_eq!(
        slot_row("slot floating_text -> sign"),
        (ValueKind::Ident("sign".to_owned()), Vec::new()),
    );
}
