//! `E_UNKNOWN_KEYWORD`: a member or selector keyword no role defines is an error at the keyword.

use cairn_lang_core::DiagnosticCode;

use crate::common::{diagnose, slice};

#[test]
fn unknown_top_level_keyword_emits_e_unknown_keyword() {
    let src = "struct s size=1x1\n  mystery foo=1\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::UnknownKeyword);
    assert_eq!(slice(src, &diags[0]), "mystery foo=1");
    assert!(
        diags[0].primary.contains("mystery"),
        "primary should name the keyword, got: {}",
        diags[0].primary,
    );
    assert_eq!(diags[0].notes.len(), 1, "expected the known-keywords note");
    assert!(
        diags[0].notes[0].message.contains("floor"),
        "note should list the canonical keywords, got: {}",
        diags[0].notes[0].message,
    );
}

#[test]
fn unknown_keyword_inside_nested_level_is_still_flagged() {
    let src = "struct s size=3x3\n  level y=0\n    mystery foo=1\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::UnknownKeyword);
    assert_eq!(slice(src, &diags[0]), "mystery foo=1");
}

#[test]
fn unknown_keyword_note_carries_no_secondary_span() {
    // A note span would make renderers print a second `file:L:C` pointer.
    let src = "struct s size=1x1\n  mystery foo=1\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1);
    assert!(
        diags[0].notes[0].span.is_none(),
        "the known-keywords note should be informational, got span: {:?}",
        diags[0].notes[0].span,
    );
}

#[test]
fn theme_selector_with_unknown_keyword_is_flagged() {
    // Lowering keeps a selector's keyword as a raw string, so this pass re-checks it.
    let src = "theme medieval:\n  windoe[class=small] -> frame=@spruce_wood\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::UnknownKeyword);
    assert!(
        diags[0].primary.contains("windoe"),
        "primary should name the typo, got: {}",
        diags[0].primary,
    );
}

#[test]
fn close_typo_attaches_did_you_mean_note() {
    let src = "struct s size=4x4\n  wals mat_slot=wall height=3\n";
    let diags = diagnose(src);
    let kw = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::UnknownKeyword)
        .unwrap_or_else(|| panic!("no E_UNKNOWN_KEYWORD, got {diags:#?}"));
    assert_eq!(
        kw.notes.len(),
        2,
        "expected a suggestion note plus the candidate list, got: {:#?}",
        kw.notes,
    );
    assert!(
        kw.notes[0].message.contains("did you mean `walls`"),
        "first note should be the targeted suggestion, got: {}",
        kw.notes[0].message,
    );
    assert!(
        kw.notes[1].message.contains("expected one of"),
        "second note should remain the candidate listing, got: {}",
        kw.notes[1].message,
    );
}

#[test]
fn distant_typo_skips_the_suggestion() {
    let src = "struct s size=1x1\n  mystery foo=1\n";
    let diags = diagnose(src);
    let kw = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::UnknownKeyword)
        .unwrap_or_else(|| panic!("no E_UNKNOWN_KEYWORD, got {diags:#?}"));
    assert_eq!(
        kw.notes.len(),
        1,
        "no suggestion expected, got: {:#?}",
        kw.notes
    );
    assert!(kw.notes[0].message.contains("expected one of"));
}

#[test]
fn every_known_keyword_is_silent() {
    let src = "\
struct s size=4x4
  floor
  walls
  door at=center
  window at=center
  roof shape=gable
  stair at=corner
  level y=0
    floor
  pressure_plate at=front
  circuit at=back
site h:
  place use=s
  connect a.entry to b.entry path=@gravel
";
    let diags = diagnose(src);
    let kw_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::UnknownKeyword)
        .collect();
    assert!(
        kw_diags.is_empty(),
        "every known keyword should be silent, got: {kw_diags:#?}",
    );
}
