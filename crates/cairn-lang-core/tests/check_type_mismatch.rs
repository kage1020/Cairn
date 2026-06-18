//! Acceptance tests for the `type_mismatch` pass of
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
fn tm_1_id_set_to_token_is_flagged_as_label_mismatch() {
    let src = "struct s size=1x1\n  walls id=@oak\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::TypeMismatchLabel);
    assert_eq!(slice(src, &diags[0]), "@oak");
    assert!(
        diags[0].primary.contains("token"),
        "primary should name the offending kind, got: {}",
        diags[0].primary,
    );
}

#[test]
fn tm_2_class_set_to_dotted_ref_is_flagged() {
    let src = "struct s size=1x1\n  walls class=foo.bar\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::TypeMismatchLabel);
    assert_eq!(slice(src, &diags[0]), "foo.bar");
}

#[test]
fn tm_3_mat_slot_string_value_is_accepted() {
    let src = "struct s size=1x1\n  walls mat_slot=\"wall\"\n";
    let diags = diagnose(src);
    assert!(
        diags.is_empty(),
        "string label values are valid, got {diags:#?}"
    );
}

#[test]
fn tm_4_struct_size_set_to_integer_is_flagged() {
    let src = "struct s size=5\n  floor\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::TypeMismatchSize);
    assert_eq!(slice(src, &diags[0]), "5");
    assert!(
        diags[0].primary.contains("integer"),
        "primary should name the offending kind, got: {}",
        diags[0].primary,
    );
}

#[test]
fn tm_5_window_size_with_proper_literal_passes() {
    let src = "struct s size=4x4\n  window size=2x2 at=center\n";
    let diags = diagnose(src);
    assert!(
        diags.is_empty(),
        "`size=WxH` on body statements is legal, got {diags:#?}",
    );
}

#[test]
fn tm_6_duplicate_size_and_size_mismatch_pass_independently() {
    // `struct s size=2x2 size=foo` is BOTH a duplicate size= AND a
    // size= with the wrong value type. The two passes are independent.
    let src = "struct s size=2x2 size=foo\n  floor\n";
    let diags = diagnose(src);
    let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
    assert!(
        codes.contains(&DiagnosticCode::DuplicateSize),
        "expected E_DUPLICATE_SIZE among {codes:?}",
    );
    assert!(
        codes.contains(&DiagnosticCode::TypeMismatchSize),
        "expected E_TYPE_MISMATCH_SIZE among {codes:?}",
    );
}
