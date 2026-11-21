//! A selector's `level=NAME` selects the members that stand under a `level` whose `id=` is `NAME`
//! (`spec/materials-themes` "Slots as dependency injection").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::ast::ValueKind;

use crate::common::{codes, selector_extras};

/// A window under the struct, one under `level id=floor2`, one under a `level` with no `id=`, and
/// a theme row selecting `window[level=LEVEL]`.
fn floors(level: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  window[level={level}] -> frame=@spruce_wood\n\n\
         struct s size=9x7\n  walls mat_slot=wall height=8\n  \
         window side=front offset=1 y=1\n  \
         level id=floor2 y=4\n    window side=front offset=3 y=1\n  \
         level y=6\n    window side=front offset=5 y=1\n"
    )
}

#[test]
fn a_level_attribute_selects_the_members_under_a_level_of_that_id() {
    for level in ["floor2", "\"floor2\""] {
        let source = floors(level);
        assert_eq!(
            codes(&source),
            [DiagnosticCode::IgnoredArgument.as_str()],
            "{level}"
        );
        assert_eq!(
            selector_extras(&source),
            [(
                "frame".to_owned(),
                ValueKind::Token("spruce_wood".to_owned())
            )],
            "{level}",
        );
    }
}

#[test]
fn a_level_attribute_no_level_carries_selects_nothing() {
    let source = floors("floor3");
    assert_eq!(
        codes(&source),
        [
            DiagnosticCode::ThemeSelectorUnmatched.as_str(),
            DiagnosticCode::IgnoredArgument.as_str(),
        ],
    );
    assert_eq!(selector_extras(&source), []);
}
