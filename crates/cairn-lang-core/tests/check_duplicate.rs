//! Acceptance tests for the `duplicate` pass of `cairn_lang_core::check`.

use cairn_lang_core::{Diagnostic, DiagnosticCode};

mod common;
use common::{diagnose, exactly_one, nth, slice};

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

/// The theme and def a `place use=hut theme=t` row names, so the row
/// resolves and the findings are about its `id=`.
const HUT: &str = "theme t:\n  slot floor -> @oak_planks\n\n\
                   def hut size=3x3:\n  floor mat_slot=floor\n\n";

fn of_code(diags: &[Diagnostic], code: DiagnosticCode) -> Vec<&Diagnostic> {
    diags.iter().filter(|d| d.code == code).collect()
}

/// The text the note of `diag` points at. Every duplicate finding carries
/// exactly one note, on the declaration that stands.
fn note_text<'a>(source: &'a str, diag: &Diagnostic) -> &'a str {
    assert_eq!(diag.notes.len(), 1, "got {diag:#?}");
    let span = diag.notes[0]
        .span
        .as_ref()
        .expect("a duplicate's note carries the first declaration's span");
    &source[span.clone()]
}

#[test]
fn dup_8_a_repeated_place_id_is_reported_once_as_duplicate_place_id() {
    // `E_DUPLICATE_PLACE_ID` is the code `spec/lint` "Sites and
    // placements" gives this case, and the one that names the site; the
    // generic `E_DUPLICATE_ID` beside it billed one repair twice. The
    // finding that stays spans the whole second row, and its note the
    // whole first row, where `E_DUPLICATE_ID` spanned their `id=`.
    let src = format!(
        "{HUT}site v:\n  place id=a use=hut theme=t at=origin\n  \
         place id=a use=hut theme=t east_of=a gap=2\n"
    );
    let found = exactly_one(diagnose(&src));
    assert_eq!(found.code, DiagnosticCode::DuplicatePlaceId);
    assert_eq!(found.primary, "duplicate `id=a` in site `v`");
    assert_eq!(found.span.start, nth(&src, "place id=a", 1));
    assert_eq!(
        slice(&src, &found),
        "place id=a use=hut theme=t east_of=a gap=2"
    );
    assert_eq!(
        note_text(&src, &found),
        "place id=a use=hut theme=t at=origin"
    );
}

#[test]
fn dup_9_a_site_body_still_reports_a_repeated_id_on_rows_other_than_place() {
    // Only `place` rows of a `site` body are left to the resolver. Its
    // placement loop reads a `connect` row but not its `id=` (dup_10), and
    // skips a row the body has no reader for entirely, which is
    // `E_MISPLACED_MEMBER` (one per `door` here), so a repeat on either is
    // still this pass's to report.
    let src = "site v:\n  door id=x\n  door id=x\n";
    let diags = diagnose(src);
    assert_eq!(
        of_code(&diags, DiagnosticCode::MisplacedMember).len(),
        2,
        "got {diags:#?}"
    );
    let dup = of_code(&diags, DiagnosticCode::DuplicateId);
    assert_eq!(dup.len(), 1, "got {diags:#?}");
    assert_eq!(diags.len(), 3, "got {diags:#?}");
    assert_eq!(slice(src, dup[0]), "id=x");
    assert_eq!(dup[0].span.start, nth(src, "id=x", 1));
    let note = dup[0].notes[0].span.as_ref().expect("note span");
    assert_eq!(note.start, nth(src, "id=x", 0));
    assert_eq!(note_text(src, dup[0]), "id=x");
}

#[test]
fn dup_10_a_place_row_and_a_connect_row_sharing_an_id_are_duplicate_id() {
    // The resolver's ledger holds `place` rows only, so a `place` row's id
    // has to stay in this pass's ledger too: skipping it there lost the
    // repeat on the `connect` row from both passes.
    let src = format!(
        "{HUT}site v:\n  place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=2\n  \
         connect id=a a.entry to b.entry\n"
    );
    let diags = diagnose(&src);
    let dup = of_code(&diags, DiagnosticCode::DuplicateId);
    assert_eq!(dup.len(), 1, "got {diags:#?}");
    assert_eq!(dup[0].span.start, nth(&src, "id=a", 1));
    assert_eq!(slice(&src, dup[0]), "id=a");
    assert_eq!(note_text(&src, dup[0]), "id=a");
    assert!(
        of_code(&diags, DiagnosticCode::DuplicatePlaceId).is_empty(),
        "got {diags:#?}"
    );
}

#[test]
fn dup_11_a_place_row_and_a_misplaced_row_sharing_an_id_are_duplicate_id() {
    // Either order: the exemption needs both rows to be `place` rows, not
    // just the first or just the repeat.
    for (rows, repeat) in [
        (
            "  place id=a use=hut theme=t at=origin\n  door id=a\n",
            "door id=a",
        ),
        (
            "  door id=a\n  place id=a use=hut theme=t at=origin\n",
            "place id=a",
        ),
    ] {
        let src = format!("{HUT}site v:\n{rows}");
        let diags = diagnose(&src);
        let dup = of_code(&diags, DiagnosticCode::DuplicateId);
        assert_eq!(dup.len(), 1, "{repeat}: got {diags:#?}");
        assert_eq!(
            dup[0].span.start,
            nth(&src, repeat, 0) + repeat.len() - "id=a".len(),
            "{repeat}: the repeat is the second row's `id=`",
        );
        assert_eq!(note_text(&src, dup[0]), "id=a");
        assert_eq!(
            of_code(&diags, DiagnosticCode::MisplacedMember).len(),
            1,
            "{repeat}: got {diags:#?}"
        );
    }
}

#[test]
fn dup_12_place_rows_nested_in_a_site_body_are_still_duplicate_id() {
    // The resolver's placement loop walks the `site` body one level deep,
    // so the two `place` rows under `level` never reach it: this pass is
    // the only reporter, whatever kind of body the rows sit in.
    let src = format!(
        "{HUT}site v:\n  level y=0\n    place id=a use=hut theme=t at=origin\n    \
         place id=a use=hut theme=t east_of=a gap=2\n"
    );
    let diags = diagnose(&src);
    let dup = of_code(&diags, DiagnosticCode::DuplicateId);
    assert_eq!(dup.len(), 1, "got {diags:#?}");
    assert_eq!(dup[0].span.start, nth(&src, "id=a", 1));
    assert_eq!(note_text(&src, dup[0]), "id=a");
    assert!(
        of_code(&diags, DiagnosticCode::DuplicatePlaceId).is_empty(),
        "got {diags:#?}"
    );
}

#[test]
fn dup_13_place_rows_in_a_def_body_are_still_duplicate_id() {
    // Only a `site` body's own rows reach the resolver; a `place` row in a
    // `def` is `E_MISPLACED_MEMBER`, and its repeated id is this pass's.
    let src = "def hut size=3x3:\n  floor mat_slot=floor\n  \
               place id=a use=hut theme=t at=origin\n  \
               place id=a use=hut theme=t at=origin\n";
    let diags = diagnose(src);
    let dup = of_code(&diags, DiagnosticCode::DuplicateId);
    assert_eq!(dup.len(), 1, "got {diags:#?}");
    assert_eq!(dup[0].span.start, nth(src, "id=a", 1));
    assert_eq!(note_text(src, dup[0]), "id=a");
    assert!(
        of_code(&diags, DiagnosticCode::DuplicatePlaceId).is_empty(),
        "got {diags:#?}"
    );
}

#[test]
fn dup_14_a_repeated_string_place_id_is_reported_once_as_duplicate_place_id() {
    // This pass leaves the pair to the resolver, so the resolver has to see
    // the same id: `id_declaration` and `intent::lower`'s `hoist_label`
    // both take a string as well as an identifier. Were either to stop,
    // this would be no finding at all, or two.
    let src = format!(
        "{HUT}site v:\n  place id=\"a\" use=hut theme=t at=origin\n  \
         place id=\"a\" use=hut theme=t east_of=a gap=2\n"
    );
    let found = exactly_one(diagnose(&src));
    assert_eq!(found.code, DiagnosticCode::DuplicatePlaceId);
    assert_eq!(found.span.start, nth(&src, "place id=", 1));
}

#[test]
fn dup_15_a_place_row_repeating_a_connect_row_and_a_place_row_is_reported_twice() {
    // The edge of comparing a repeat with the first row only. After
    // `connect id=a`, both `place` rows repeat the `connect` row here; the
    // last one also repeats the `place` row above it in the resolver, so
    // that row carries both codes, with notes on different rows.
    let src = format!(
        "{HUT}site v:\n  place id=b use=hut theme=t at=origin\n  \
         connect id=a b.entry to b.entry\n  \
         place id=a use=hut theme=t east_of=b gap=2\n  \
         place id=a use=hut theme=t east_of=b gap=8\n"
    );
    let diags = diagnose(&src);
    let dup = of_code(&diags, DiagnosticCode::DuplicateId);
    let starts: Vec<usize> = dup.iter().map(|d| d.span.start).collect();
    assert_eq!(
        starts,
        [nth(&src, "id=a", 1), nth(&src, "id=a", 2)],
        "got {diags:#?}"
    );
    for d in &dup {
        let note = d.notes[0].span.as_ref().expect("note span");
        assert_eq!(note.start, nth(&src, "id=a", 0), "the `connect` row's id");
    }
    let place = exactly_one(
        diags
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DuplicatePlaceId)
            .collect(),
    );
    assert_eq!(place.span.start, nth(&src, "place id=a", 1));
    let note = place.notes[0].span.as_ref().expect("note span");
    assert_eq!(note.start, nth(&src, "place id=a", 0));
}
