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

/// The `circuit` findings among `diags`.
fn circuit_findings(
    diags: &[cairn_lang_core::check::Diagnostic],
) -> Vec<&cairn_lang_core::check::Diagnostic> {
    diags
        .iter()
        .filter(|d| d.code == DiagnosticCode::DuplicateCircuit)
        .collect()
}

/// A second `circuit` line in a scope is refused on that line, with a
/// note on the first, rather than dropped: place-and-route reads one
/// reservation per scope. A third is refused too, against the same
/// first line.
#[test]
fn a_second_circuit_line_in_a_scope_is_refused_against_the_first() {
    let src = "struct s size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n  \
               circuit region=basement void=2\n  circuit region=attic void=3\n";
    let diags = diagnose(src);
    let found = circuit_findings(&diags);
    assert_eq!(found.len(), 2, "{diags:#?}");
    for (finding, line) in found.iter().zip([
        "circuit region=basement void=2",
        "circuit region=attic void=3",
    ]) {
        assert_eq!(slice(src, finding), line);
        assert_eq!(
            finding.primary,
            "this scope already has a `circuit` line, and a scope reserves one region for its redstone",
        );
        let first = finding.notes[0]
            .span
            .as_ref()
            .expect("the note points at the first line");
        assert_eq!(&src[first.clone()], "circuit region=floor void=1");
    }
}

/// Every `circuit` line counts, whether or not it is a usable
/// reservation and in a `def` as in a `struct`: the first line here is
/// not usable, and the second is still a second line.
#[test]
fn a_circuit_line_after_an_unusable_one_is_still_a_second_line() {
    let src = "def d size=7x5\n  floor mat_slot=m\n  circuit region=floor void=0\n  \
               circuit region=floor void=2\n";
    let diags = diagnose(src);
    let found = circuit_findings(&diags);
    assert_eq!(found.len(), 1, "{diags:#?}");
    assert_eq!(slice(src, found[0]), "circuit region=floor void=2");
}

/// One `circuit` line per scope is not a duplicate across scopes, and a
/// `circuit` inside a `level` is not one of its scope's own lines, which
/// is all place-and-route reads.
#[test]
fn one_circuit_line_per_scope_is_not_a_duplicate() {
    let src = "struct a size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n\n\
               struct b size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n  \
               level y=1\n    circuit region=floor void=1\n";
    let diags = diagnose(src);
    assert!(circuit_findings(&diags).is_empty(), "{diags:#?}");
}
