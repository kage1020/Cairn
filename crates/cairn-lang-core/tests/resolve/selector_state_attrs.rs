//! A selector attribute that names a blockstate key matches what the member's `intent_state`
//! holds, as one that names an argument matches its arguments.

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::ast::ValueKind;

use crate::common::{codes, selector_extras};

/// An eave stair written `half=top`, and a theme row selecting stairs by `half=`.
fn eave(selected: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot eave -> @oak_stairs\n  \
         stair[half={selected}] -> eave=@spruce_stairs\n\n\
         struct s size=3x1\n  walls mat_slot=wall height=1\n  \
         roof kind=flat mat_slot=wall overhang=1\n  \
         stair kind=stairs mat_slot=eave side=front half=top\n"
    )
}

#[test]
fn a_selector_on_a_state_key_selects_the_member_that_writes_it() {
    let source = eave("top");
    // Only the row's own binding, which nothing lowers.
    assert_eq!(codes(&source), [DiagnosticCode::IgnoredArgument.as_str()]);
    assert_eq!(
        selector_extras(&source),
        [(
            "eave".to_owned(),
            ValueKind::Token("spruce_stairs".to_owned())
        )],
    );
}

#[test]
fn a_selector_on_a_state_key_another_value_does_not_select() {
    let source = eave("bottom");
    assert_eq!(
        codes(&source),
        [
            DiagnosticCode::ThemeSelectorUnmatched.as_str(),
            DiagnosticCode::IgnoredArgument.as_str(),
        ],
    );
    assert_eq!(selector_extras(&source), []);
}
