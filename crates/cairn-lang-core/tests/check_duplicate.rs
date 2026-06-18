//! Acceptance tests for the `duplicate` pass of `cairn_lang_core::check`.

use cairn_lang_core::{DiagnosticCode, check, lower, parse};

fn diagnose(source: &str) -> Vec<cairn_lang_core::Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let ir = lower(&module);
    check(&module, &ir)
}

/// Convenience: pull `&source[span]` so AC tables can assert what text the
/// diagnostic actually points at.
fn slice<'a>(source: &'a str, diag: &cairn_lang_core::Diagnostic) -> &'a str {
    &source[diag.span.clone()]
}

#[test]
fn dup_1_duplicate_size_flags_second_occurrence_only() {
    let src = "struct s size=4x4 size=5x5\n  floor\n";
    let diags = diagnose(src);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one diagnostic, got {diags:#?}"
    );
    assert_eq!(diags[0].code, DiagnosticCode::DuplicateSize);
    assert_eq!(slice(src, &diags[0]), "size=5x5");
    assert_eq!(
        diags[0].notes.len(),
        1,
        "note pointing at first declaration"
    );
    assert_eq!(&src[diags[0].notes[0].span.clone()], "size=4x4");
}

#[test]
fn dup_2_duplicate_theme_slot_is_reported() {
    let src = "theme t:\n  slot wall -> @oak\n  slot wall -> @birch\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DiagnosticCode::DuplicateSlot);
    assert!(
        diags[0].primary.contains("slot wall"),
        "primary should name the slot, got: {}",
        diags[0].primary,
    );
}

#[test]
fn dup_3_duplicate_arg_inside_statement_args() {
    let src = "struct s size=1x1\n  walls height=4 height=5\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DiagnosticCode::DuplicateArg);
    assert_eq!(slice(src, &diags[0]), "height=5");
}

#[test]
fn dup_4_duplicate_id_in_same_body_scope() {
    // Diagnostic spans target the `id=NAME` attribute itself (tighter than
    // the surrounding statement), so a UI rendering an underline only
    // highlights the offending assignment.
    let src = "struct s size=3x3\n  door id=front\n  door id=front\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::DuplicateId);
    assert_eq!(slice(src, &diags[0]), "id=front", "second declaration");
    assert_eq!(diags[0].notes.len(), 1);
    assert_eq!(
        &src[diags[0].notes[0].span.clone()],
        "id=front",
        "note points at first occurrence",
    );
}

#[test]
fn dup_5_id_in_nested_scope_does_not_clash_with_outer() {
    // Per-immediate-body id scoping: the outer `door id=front` and the
    // inner one in `level y=0` are independent namespaces.
    let src = "struct s size=3x3\n  door id=front\n  level y=0\n    door id=front\n";
    let diags = diagnose(src);
    assert!(
        diags.is_empty(),
        "scopes should be per-body, got {diags:#?}",
    );
}

#[test]
fn dup_6_duplicate_selector_attribute_is_reported() {
    let src = "struct s size=1x1\n  door[id=front id=back] at=center\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DiagnosticCode::DuplicateArg);
    assert_eq!(slice(src, &diags[0]), "id=back");
}

#[test]
fn dup_7_duplicate_header_arg_other_than_size_is_arg_not_size() {
    // Header has a repeated non-`size` arg; the code should be the generic
    // `E_DUPLICATE_ARG`, not the size-specific `E_DUPLICATE_SIZE`.
    let src = "struct s size=1x1 wood=oak wood=birch\n  floor\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::DuplicateArg);
    assert_eq!(slice(src, &diags[0]), "wood=birch");
}
