//! Acceptance tests for the `duplicate` pass of `cairn_lang_core::check`.

use cairn_lang_core::DiagnosticCode;

mod common;
use common::{diagnose, slice};

#[test]
fn dup_1_duplicate_size_flags_second_occurrence_only() {
    let src = "struct s size=4x4 size=5x5\n  floor mat_slot=m\n";
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
    let first_span = diags[0].notes[0]
        .span
        .as_ref()
        .expect("duplicate notes carry the first-occurrence span");
    assert_eq!(&src[first_span.clone()], "size=4x4");
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
    let src = "struct s size=1x1\n  walls height=4 height=5 mat_slot=m\n";
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
    let first_span = diags[0].notes[0]
        .span
        .as_ref()
        .expect("DuplicateId notes carry the first-occurrence span");
    assert_eq!(
        &src[first_span.clone()],
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
    // `E_DUPLICATE_ARG`, not the size-specific `E_DUPLICATE_SIZE`. The
    // header vocabulary is closed at `size=` and `class=`, so `class=` is
    // the only key left to repeat: any other, `wood=` included, is also an
    // `E_UNKNOWN_ARGUMENT`. No pass reads a header's `class=` yet, so the
    // arguments pass reports the surviving value beside the duplicate.
    let src = "struct s size=1x1 class=house class=hut\n  floor mat_slot=m\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 2, "got {diags:#?}");
    let dup: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::DuplicateArg)
        .collect();
    assert_eq!(dup.len(), 1, "got {diags:#?}");
    assert_eq!(slice(src, dup[0]), "class=hut");
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::IgnoredArgument && slice(src, d) == "hut"),
        "got {diags:#?}",
    );
}

#[test]
fn dup_8_a_repeated_place_id_is_reported_once_as_duplicate_place_id() {
    // `E_DUPLICATE_PLACE_ID` is the code `spec/lint` "Sites and
    // placements" gives this case, and the one that names the site; the
    // generic `E_DUPLICATE_ID` beside it billed one repair twice.
    let src = "theme t:\n  slot floor -> @oak_planks\n\n\
               def hut size=3x3:\n  floor mat_slot=floor\n\n\
               site v:\n  place id=a use=hut theme=t at=origin\n  \
               place id=a use=hut theme=t east_of=a gap=2\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::DuplicatePlaceId);
}

#[test]
fn dup_9_a_site_body_still_reports_a_repeated_id_on_rows_other_than_place() {
    // Only `place` rows are left to the resolver. A member a `site` body has
    // no reader for is `E_MISPLACED_MEMBER`, and the resolver never sees
    // its `id=`, so a repeat of it is still this pass's to report.
    let src = "site v:\n  door id=x\n  door id=x\n";
    let diags = diagnose(src);
    let dup: Vec<_> = diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::DuplicateId)
        .collect();
    assert_eq!(dup.len(), 1, "got {diags:#?}");
    assert_eq!(slice(src, dup[0]), "id=x");
    assert_eq!(dup[0].span.start, src.rfind("id=x").expect("second id"));
}
