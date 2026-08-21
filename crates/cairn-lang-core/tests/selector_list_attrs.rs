//! A selector attribute whose value is a list selects the members that
//! carry that list.
//!
//! `ast::Value` derived `PartialEq` over both of its fields, and one of
//! them is the source span. `ValueKind::List(Vec<Value>)` therefore
//! compared its elements' spans, so two lists written identically on two
//! lines were never equal — and `member_attr_matches`, which compares a
//! selector's expected value against the member's `key=value` arg by
//! `ValueKind`, matched no member at all for a list-valued attribute. The
//! author saw `E_THEME_SELECTOR_UNMATCHED`, which reads as "your filter is
//! too narrow" rather than "this attribute type cannot match".

use cairn_lang_core::ast::ValueKind;
use cairn_lang_core::{DiagnosticCode, Severity, check, lower, parse, resolve};

fn diagnose(source: &str) -> Vec<cairn_lang_core::Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn codes(source: &str) -> Vec<&'static str> {
    diagnose(source).iter().map(|d| d.code.as_str()).collect()
}

/// The `key=value` bindings a matching theme selector injected, for every
/// member of every scope.
fn selector_extras(source: &str) -> Vec<(String, ValueKind)> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    resolve(&ir, None)
        .scopes
        .values()
        .flat_map(|scope| scope.members.values())
        .flat_map(|binding| {
            binding
                .selector_extras
                .iter()
                .map(|(key, value)| (key.clone(), value.value.kind.clone()))
        })
        .collect()
}

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
    // The absence of a warning is not the contract — a selector that
    // matches is a selector whose bindings land. Reading
    // `selector_extras` is what distinguishes "matched" from "no longer
    // complained about".
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
    // The other half: the fix is "compare what the value is", not
    // "compare nothing". A member whose list is a different list, a
    // shorter list, or a differently ordered list is not selected.
    for (label, member_tags) in [
        ("different element", "[a,c]"),
        ("shorter", "[a]"),
        ("reordered", "[b,a]"),
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
    // `ValueKind::List` holds `Value`s, so its derived equality recurses
    // through the very impl this fixes. One level of nesting proves the
    // recursion, which a flat list cannot.
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
        "a nested list should match"
    );
}

#[test]
fn two_identical_list_valued_rows_are_a_duplicate_pair() {
    // The inherited half. `select_the_same_members` compares the two rows'
    // attribute values the same way, so while a list never equalled
    // itself, two byte-identical rows were not recognised as coinciding
    // and `E_DUPLICATE_SELECTOR` under-reported.
    let source = "\
theme t:
  slot glass -> @glass_pane
  window[tags=[a,b]] -> frame=@spruce_wood
  window[tags=[a,b]] -> frame=@oak_planks

struct s size=9x7
  window tags=[a,b] class=small side=front offset=2 y=2 size=2x2 mat_slot=glass
";
    let found = diagnose(source);
    let duplicate = found
        .iter()
        .find(|d| d.code == DiagnosticCode::DuplicateSelector)
        .unwrap_or_else(|| {
            panic!(
                "expected E_DUPLICATE_SELECTOR, got {:?}",
                found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
            )
        });
    assert_eq!(duplicate.severity(), Severity::Error);
}
