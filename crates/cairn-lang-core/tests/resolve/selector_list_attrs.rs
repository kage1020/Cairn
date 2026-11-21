//! A selector attribute whose value is a list selects the members that carry an
//! equal list, compared by value and never by source span.

use cairn_lang_core::ast::ValueKind;
use cairn_lang_core::{DiagnosticCode, Severity};

use crate::common::{codes, diagnose, selector_extras};

const MATCHING: &str = "\
theme t:
  slot glass -> @glass_pane
  window[tags=[a,b]] -> frame=@spruce_wood

struct s size=9x7
  window tags=[a,b] class=small side=front offset=2 y=2 size=2x2 mat_slot=glass
";

#[test]
fn a_list_valued_selector_attribute_matches_the_member_that_carries_it() {
    assert_eq!(
        codes(MATCHING),
        Vec::<&str>::new(),
        "a selector that does match must report nothing",
    );
}

#[test]
fn the_binding_that_selector_carries_actually_reaches_the_member() {
    let extras = selector_extras(MATCHING);
    assert_eq!(
        extras,
        vec![(
            "frame".to_owned(),
            ValueKind::Token("spruce_wood".to_owned())
        )],
        "the selector's binding did not reach the member",
    );
}

#[test]
fn a_list_that_differs_still_does_not_match() {
    // Each row rules out a reading: a truncating zip, set semantics, and containment.
    for (label, member_tags) in [
        ("different element", "[a,c]"),
        ("shorter", "[a]"),
        ("reordered", "[b,a]"),
        ("longer", "[a,b,c]"),
    ] {
        let source = MATCHING.replace("window tags=[a,b]", &format!("window tags={member_tags}"));
        assert_ne!(
            source, MATCHING,
            "{label}: the fixture did not change the member"
        );
        let found = codes(&source);
        assert!(
            found.contains(&DiagnosticCode::ThemeSelectorUnmatched.as_str()),
            "{label}: expected the selector to go unmatched, got {found:?}",
        );
    }
}

#[test]
fn a_nested_list_is_compared_by_what_it_holds() {
    let source = "\
theme t:
  slot glass -> @glass_pane
  window[groups=[[a,b],[c]]] -> frame=@spruce_wood

struct s size=9x7
  window groups=[[a,b],[c]] class=small side=front offset=2 y=2 size=2x2 mat_slot=glass
";
    assert_eq!(
        codes(source),
        Vec::<&str>::new(),
        "a nested list should match, leaving nothing to report"
    );
}

#[test]
fn two_identical_list_valued_rows_are_a_duplicate_pair() {
    let source = "\
theme t:
  slot glass -> @glass_pane
  window[tags=[a,b]] -> frame=@spruce_wood
  window[tags=[a,b]] -> frame=@oak_planks

struct s size=9x7
  window tags=[a,b] class=small side=front offset=2 y=2 size=2x2 mat_slot=glass
";
    assert_eq!(
        codes(source),
        vec![DiagnosticCode::DuplicateSelector.as_str()]
    );
    let found = diagnose(source);
    let duplicate = found
        .iter()
        .find(|d| d.code == DiagnosticCode::DuplicateSelector)
        .expect("asserted above");
    assert_eq!(duplicate.severity(), Severity::Error);
}
