//! A member line the check pass refuses is that pass's alone to report: block-array lowering
//! builds nothing for it and adds no `W_DEFERRED_MEMBER` (`spec/lint` "Lowering").

use cairn_lang_core::Edition;

use crate::common::{checked_and_lowered, positioned};

/// What `check` and block-array lowering for `edition` report for `source`, each finding as its
/// code and the 1-based `line:column` it starts at.
fn reported(source: &str, edition: Option<Edition>) -> Vec<(&'static str, String)> {
    positioned(source, &checked_and_lowered(source, edition))
}

/// A theme with one slot, and a struct that floors itself with it.
const FLOORED: &str = "theme t:\n  slot a -> @stone\nstruct s size=3x3\n  floor mat_slot=a\n";

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
    let source = format!("{FLOORED}  @edition java\n    flor mat_slot=a\n");
    for edition in [None, Some(Edition::Java), Some(Edition::Bedrock)] {
        assert_eq!(
            reported(&source, edition),
            [("E_UNKNOWN_KEYWORD", "6:5".to_owned())],
            "{edition:?}",
        );
    }
}

#[test]
fn a_refused_line_under_a_level_is_only_refused() {
    for (row, code) in [
        ("flor mat_slot=a", "E_UNKNOWN_KEYWORD"),
        ("connect a.b to c.d", "E_MISPLACED_MEMBER"),
    ] {
        let source = format!("{FLOORED}  level y=1\n    {row}\n");
        assert_eq!(
            reported(&source, Some(Edition::Java)),
            [(code, "6:5".to_owned())],
            "{row:?}",
        );
    }
}

#[test]
fn a_misspelled_keyword_under_a_guard_in_a_level_is_only_unknown() {
    let source = format!("{FLOORED}  level y=1\n    @edition java\n      flor mat_slot=a\n");
    assert_eq!(
        reported(&source, Some(Edition::Java)),
        [("E_UNKNOWN_KEYWORD", "7:7".to_owned())]
    );
}

#[test]
fn a_refused_line_in_a_def_a_site_places_is_only_refused() {
    for (row, code) in [
        ("flor mat_slot=a", "E_UNKNOWN_KEYWORD"),
        ("connect a.b to c.d", "E_MISPLACED_MEMBER"),
    ] {
        let source = format!(
            "theme t:\n  slot a -> @stone\ndef hut size=3x3:\n  floor mat_slot=a\n  {row}\n\n\
             site s:\n  place id=h use=hut theme=t at=origin\n"
        );
        assert_eq!(
            reported(&source, Some(Edition::Java)),
            [(code, "5:3".to_owned())],
            "{row:?}",
        );
    }
}
