//! A member line the check pass refuses is that pass's alone to report: block-array lowering
//! builds nothing for it and adds no `W_DEFERRED_MEMBER` (`spec/lint` "Lowering").

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::{Edition, check, lower, parse, resolve};

/// What `check` and block-array lowering for `edition` report for `source`, each finding as its
/// code and the 1-based `line:column` it starts at.
fn reported(source: &str, edition: Option<Edition>) -> Vec<(&'static str, String)> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    let checked = check(&module, &ir, None);
    let lowered = lower_to_block_array(&ir, &resolve(&ir, edition), None).diagnostics;
    checked
        .iter()
        .chain(&lowered)
        .map(|d| {
            let before = &source[..d.span.start];
            let line = before.matches('\n').count() + 1;
            let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
            (d.code.as_str(), format!("{line}:{column}"))
        })
        .collect()
}

#[test]
fn a_misspelled_member_keyword_is_only_unknown() {
    let source = "theme t:\n  slot a -> @stone\nstruct s size=3x3\n  flor mat_slot=a\n";
    assert_eq!(
        reported(source, None),
        [("E_UNKNOWN_KEYWORD", "4:3".to_owned())]
    );
}

#[test]
fn a_keyword_the_language_does_not_define_is_only_unknown() {
    for source in [
        "struct s size=3x3\n  room id=a\n",
        "struct s size=3x3\n  torch at=1,1,1\n",
    ] {
        assert_eq!(
            reported(source, None),
            [("E_UNKNOWN_KEYWORD", "2:3".to_owned())],
            "{source:?}",
        );
    }
}

#[test]
fn a_connect_row_in_a_struct_body_is_only_misplaced() {
    let source = "struct s size=3x3\n  connect a.b to c.d\n";
    assert_eq!(
        reported(source, None),
        [("E_MISPLACED_MEMBER", "2:3".to_owned())]
    );
}

#[test]
fn a_misspelled_keyword_under_its_editions_guard_is_only_unknown() {
    let source = "theme t:\n  slot a -> @stone\nstruct s size=3x3\n  floor mat_slot=a\n  \
                  @edition java\n    flor mat_slot=a\n";
    assert_eq!(
        reported(source, Some(Edition::Java)),
        [("E_UNKNOWN_KEYWORD", "6:5".to_owned())]
    );
}
