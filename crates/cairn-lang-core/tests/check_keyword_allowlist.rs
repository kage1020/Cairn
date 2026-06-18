//! Acceptance tests for the `keyword_allowlist` pass of
//! `cairn_lang_core::check`.

use cairn_lang_core::{DiagnosticCode, check, lower, parse};

fn diagnose(source: &str) -> Vec<cairn_lang_core::Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let ir = lower(&module);
    check(&module, &ir)
}

fn slice<'a>(source: &'a str, diag: &cairn_lang_core::Diagnostic) -> &'a str {
    &source[diag.span.clone()]
}

#[test]
fn kw_1_unknown_top_level_keyword_emits_e_unknown_keyword() {
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
fn kw_2_unknown_keyword_inside_nested_level_is_still_flagged() {
    let src = "struct s size=3x3\n  level y=0\n    mystery foo=1\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::UnknownKeyword);
    assert_eq!(slice(src, &diags[0]), "mystery foo=1");
}

#[test]
fn kw_3_every_m2_known_keyword_is_silent() {
    // Build a source that uses every keyword in the M2 table. None should
    // trigger `E_UNKNOWN_KEYWORD`. `place` and `connect` live in a site;
    // everything else lives in a struct.
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
  connect a to b
";
    let diags = diagnose(src);
    let kw_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::UnknownKeyword)
        .collect();
    assert!(
        kw_diags.is_empty(),
        "every M2 known keyword should be silent, got: {kw_diags:#?}",
    );
}
