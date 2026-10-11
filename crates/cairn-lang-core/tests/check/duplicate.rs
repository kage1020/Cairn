//! Acceptance tests for the `duplicate` pass of `cairn_lang_core::check`.

use cairn_lang_core::{Diagnostic, DiagnosticCode};

use crate::common::{diagnose, exactly_one, notes, nth, slice};

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

/// The text the first note of `diag` points at, the declaration that
/// stands. Every duplicate finding read here carries that one note, and
/// `E_DUPLICATE_CIRCUIT` one more after it, spanless, on the repair.
fn note_text<'a>(source: &'a str, diag: &Diagnostic) -> &'a str {
    let expected = if diag.code == DiagnosticCode::DuplicateCircuit {
        2
    } else {
        1
    };
    assert_eq!(diag.notes.len(), expected, "got {diag:#?}");
    assert!(
        diag.notes[1..].iter().all(|n| n.span.is_none()),
        "got {diag:#?}"
    );
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

#[test]
fn dup_16_a_repeated_refused_place_id_is_still_duplicate_place_id() {
    // The resolver compares an id `PlaceId` refuses with the refused ids
    // above it, so the second row carries `E_DUPLICATE_PLACE_ID` beside its
    // own `E_INVALID_PLACE_ID`, and renaming both rows to one usable id
    // raises nothing new. This pass still leaves the pair alone.
    let src = format!(
        "{HUT}site v:\n  place id=anchor use=hut theme=t at=origin\n  \
         place id=\"a.b\" use=hut theme=t east_of=anchor gap=2\n  \
         place id=\"a.b\" use=hut theme=t east_of=anchor gap=8\n"
    );
    let diags = diagnose(&src);
    let invalid = of_code(&diags, DiagnosticCode::InvalidPlaceId);
    assert_eq!(invalid.len(), 2, "got {diags:#?}");
    let dup = of_code(&diags, DiagnosticCode::DuplicatePlaceId);
    assert_eq!(dup.len(), 1, "got {diags:#?}");
    assert_eq!(diags.len(), 3, "got {diags:#?}");
    assert_eq!(dup[0].primary, "duplicate `id=a.b` in site `v`");
    assert_eq!(dup[0].span.start, nth(&src, "place id=\"a.b\"", 1));
    let note = dup[0].notes[0].span.as_ref().expect("note span");
    assert_eq!(note.start, nth(&src, "place id=\"a.b\"", 0));
}

/// The primary every `E_DUPLICATE_CIRCUIT` carries.
const DUPLICATE_CIRCUIT_PRIMARY: &str =
    "this scope already has a `circuit` line, and a scope reserves one region for its redstone";

/// The repair note every `E_DUPLICATE_CIRCUIT` carries after the note on
/// the first line.
const DUPLICATE_CIRCUIT_REPAIR: &str = "delete one of the two lines, or split the logic across \
                                        several scopes, each with its own `circuit` line";

/// A second `circuit` line in a scope is refused on that line, with a
/// note on the first and a note on the repair, rather than dropped:
/// place-and-route reads one reservation per scope. A third is refused
/// too, against the same first line.
#[test]
fn dup_17_a_second_circuit_line_in_a_scope_is_refused_against_the_first() {
    let src = "struct s size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n  \
               circuit region=basement void=2\n  circuit region=attic void=3\n";
    let diags = diagnose(src);
    let found = of_code(&diags, DiagnosticCode::DuplicateCircuit);
    assert_eq!(found.len(), 2, "{diags:#?}");
    for (finding, line) in found.iter().zip([
        "circuit region=basement void=2",
        "circuit region=attic void=3",
    ]) {
        assert_eq!(slice(src, finding), line);
        assert_eq!(finding.primary, DUPLICATE_CIRCUIT_PRIMARY);
        assert_eq!(
            notes(finding),
            ["first declaration here", DUPLICATE_CIRCUIT_REPAIR]
        );
        assert_eq!(note_text(src, finding), "circuit region=floor void=1");
    }
}

/// Every `circuit` line counts, whether or not it is a usable
/// reservation and in a `def` as in a `struct`: the first line here is
/// not usable, and the second is still a second line.
#[test]
fn dup_18_a_circuit_line_after_an_unusable_one_is_still_a_second_line() {
    let src = "def d size=7x5\n  floor mat_slot=m\n  circuit region=floor void=0\n  \
               circuit region=floor void=2\n";
    let diags = diagnose(src);
    let found = of_code(&diags, DiagnosticCode::DuplicateCircuit);
    assert_eq!(found.len(), 1, "{diags:#?}");
    assert_eq!(slice(src, found[0]), "circuit region=floor void=2");
    assert_eq!(note_text(src, found[0]), "circuit region=floor void=0");
}

/// One `circuit` line per scope is not a duplicate across scopes, and a
/// `circuit` line under a `level` is in that level's body, not among the
/// scope's own members, which are all this check counts. The source
/// raises nothing else either, so a finding of any code here fails.
#[test]
fn dup_19_one_circuit_line_per_scope_is_not_a_duplicate() {
    let src = "struct a size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n\n\
               struct b size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n  \
               level y=1\n    circuit region=floor void=1\n";
    let diags = diagnose(src);
    assert!(diags.is_empty(), "{diags:#?}");
}

/// A `circuit` line written with a `[...]` selector is a `circuit` line
/// all the same: it reserves the scope's region as the bare form does,
/// so it counts as a second line after one.
#[test]
fn dup_20_a_circuit_line_with_a_selector_counts() {
    let src = "struct s size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n  \
               circuit[id=x] region=basement void=2\n";
    let diags = diagnose(src);
    let found = of_code(&diags, DiagnosticCode::DuplicateCircuit);
    assert_eq!(found.len(), 1, "{diags:#?}");
    assert_eq!(diags.len(), 1, "{diags:#?}");
    assert_eq!(slice(src, found[0]), "circuit[id=x] region=basement void=2");
    assert_eq!(note_text(src, found[0]), "circuit region=floor void=1");
}

/// A scope with no `size=` reserves nothing from any of its `circuit`
/// lines, and two of them are refused all the same: adding the header
/// would leave the scope two reservations. The missing header is
/// `W_STRUCT_NO_SIZE`, which block-array lowering raises and `check`
/// does not, so this is the only finding here.
#[test]
fn dup_21_two_circuit_lines_in_a_scope_with_no_size_are_refused() {
    let src = "struct s\n  floor mat_slot=m\n  circuit region=floor void=1\n  \
               circuit region=basement void=2\n";
    let diags = diagnose(src);
    let found = of_code(&diags, DiagnosticCode::DuplicateCircuit);
    assert_eq!(found.len(), 1, "{diags:#?}");
    assert_eq!(diags.len(), 1, "{diags:#?}");
    assert_eq!(slice(src, found[0]), "circuit region=basement void=2");
    assert_eq!(note_text(src, found[0]), "circuit region=floor void=1");
}

/// A second `circuit` line that carries an indented body is refused as a
/// second line, and the body under it is refused on its own as
/// `E_UNSUPPORTED_NESTING`: the two findings are separate repairs, and
/// the span of this one is the keyword line, not the body.
#[test]
fn dup_22_a_second_circuit_line_with_an_indented_body_is_refused_beside_the_nesting() {
    let src = "struct s size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n  \
               circuit region=basement void=2\n    floor mat_slot=m\n";
    let diags = diagnose(src);
    let codes: Vec<_> = diags.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        ["E_DUPLICATE_CIRCUIT", "E_UNSUPPORTED_NESTING"],
        "{diags:#?}"
    );
    let found = of_code(&diags, DiagnosticCode::DuplicateCircuit);
    assert_eq!(slice(src, found[0]), "circuit region=basement void=2");
    assert_eq!(note_text(src, found[0]), "circuit region=floor void=1");
}

mod items {
    //! Acceptance tests for the top-level-name and header scopes of
    //! `cairn_lang_core::check`'s `duplicate` pass.
    //!
    //! The pass used to look only *inside* items — header args, theme slots,
    //! statement args, member `id=`. Two `def hut` blocks, or two `@cairn`
    //! lines, reached the resolver with no diagnostic, and one of them was
    //! discarded: the resolver binds a name once, so the loser's body never
    //! reaches an artifact. `spec/lint` "Error vs warning" forbids exactly
    //! that.
    //!
    //! Two invariants are pinned here:
    //!
    //! 1. **Reporting** — a repeat within one kind, or a repeated
    //!    single-valued `@directive`, is an error anchored on the repeat
    //!    with a note on the first declaration. Names are per-kind: the
    //!    resolver keys scopes `struct::` / `def::` / `site::NAME::` and
    //!    holds themes separately, so `theme x` alongside `struct x` is not
    //!    a collision.
    //! 2. **Resolution** — the first declaration to claim a *binding key*
    //!    wins. The key is the name for `theme` / `struct` / `def`, but
    //!    `site::NAME::PLACE_ID` for a placement, so two `site` blocks of
    //!    one name merge rather than shadow. `FIRST_BINDING_WINS` on
    //!    `resolve` records why that direction was picked.

    use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
    use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, lower, parse, resolve};

    use crate::common::{codes, diagnose, nth, slice};

    fn of_code(source: &str, code: DiagnosticCode) -> Vec<Diagnostic> {
        diagnose(source)
            .into_iter()
            .filter(|d| d.code == code)
            .collect()
    }

    const THEME: &str = "theme t:\n  slot floor -> @oak_planks\n\n";

    /// Two items of the same kind sharing a name earn one error, anchored on
    /// the *second* name token — the word the author would rename — with a
    /// note on the first.
    ///
    /// One case per kind because the four are separate namespaces with
    /// separate resolver code paths; a fix applied to the map for one kind
    /// says nothing about the other three.
    #[test]
    fn di_1_each_item_kind_reports_its_own_duplicate_name() {
        let cases = [
            (
                "theme",
                "t",
                format!(
                    "{THEME}theme t:
  slot floor -> @stone

struct s size=3x3
  floor
"
                ),
            ),
            (
                "def",
                "hut",
                format!(
                    "{THEME}def hut size=3x3:
  floor id=f

def hut size=5x5:
  floor id=f

site s:
  place id=a use=hut theme=t at=origin
"
                ),
            ),
            (
                "struct",
                "s",
                format!(
                    "{THEME}struct s size=3x3
  floor

struct s size=5x5
  floor
"
                ),
            ),
            (
                "site",
                "s",
                format!(
                    "{THEME}def hut size=3x3:
  floor id=f

site s:
  place id=a use=hut theme=t at=origin

site s:
  place id=b use=hut theme=t at=origin
"
                ),
            ),
        ];
        for (keyword, name, src) in cases {
            let found = of_code(&src, DiagnosticCode::DuplicateItem);
            assert_eq!(found.len(), 1, "{keyword}: got {found:#?}");
            let d = &found[0];
            assert_eq!(d.severity(), Severity::Error);
            assert_eq!(
                slice(&src, d),
                name,
                "{keyword}: span should cover the name token",
            );
            assert!(
                d.span.start > nth(&src, &format!("{keyword} {name}"), 1),
                "{keyword}: span should be inside the second declaration",
            );
            assert!(
                d.primary.contains(&format!("`{keyword} {name}`")),
                "{keyword}: message should name the kind and the name, got: {}",
                d.primary,
            );
            assert!(
                d.notes.iter().any(|n| n.span.is_some()),
                "{keyword}: a note should point at the first declaration",
            );
        }
    }

    /// The anchor survives extra whitespace between keyword and name.
    ///
    /// This is the case `name_span` exists for: the alternative was to
    /// rebuild the header line as `span.start + keyword.len() + 1`, which
    /// silently slides off the name as soon as the author lines their
    /// declarations up.
    #[test]
    fn di_1b_the_anchor_follows_the_name_through_padded_whitespace() {
        let src = format!(
            "{THEME}struct   s size=3x3
  floor

struct   s size=5x5
  floor
"
        );
        let found = of_code(&src, DiagnosticCode::DuplicateItem);
        assert_eq!(found.len(), 1, "got {found:#?}");
        assert_eq!(slice(&src, &found[0]), "s");
    }

    /// Three declarations earn two errors, not one. Reporting only the first
    /// repeat would send the author round the edit-check loop once per extra
    /// copy.
    #[test]
    fn di_2_every_repeat_after_the_first_is_reported() {
        let src = format!(
            "{THEME}struct s size=3x3\n  floor\n\nstruct s size=4x4\n  floor\n\nstruct s size=5x5\n  floor\n"
        );
        let found = of_code(&src, DiagnosticCode::DuplicateItem);
        assert_eq!(found.len(), 2, "got {found:#?}");
        assert_eq!(
            found[0].span.start,
            nth(&src, "struct s", 1) + "struct ".len()
        );
        assert_eq!(
            found[1].span.start,
            nth(&src, "struct s", 2) + "struct ".len()
        );
    }

    /// The four kinds are separate namespaces, so one name on all four is
    /// legal and must stay silent. Without this, the obvious implementation —
    /// one map keyed by name — would look correct against every other test
    /// here.
    #[test]
    fn di_3_the_same_name_on_different_kinds_is_not_a_collision() {
        let src = "theme x:\n  slot floor -> @oak_planks\n\n\
struct x size=3x3\n  floor mat_slot=floor\n\n\
def x size=3x3:\n  floor id=f mat_slot=floor\n\n\
site x:\n  place id=a use=x theme=x at=origin\n";
        let found = of_code(src, DiagnosticCode::DuplicateItem);
        assert!(found.is_empty(), "got {found:#?}");
    }

    /// Duplicates need not be adjacent. A pass that compared each item with
    /// its predecessor rather than with everything seen so far would pass the
    /// cases above and miss this one.
    #[test]
    fn di_4_duplicates_separated_by_other_items_are_still_reported() {
        let src = format!(
            "{THEME}struct s size=3x3\n  floor\n\nstruct other size=4x4\n  floor\n\nstruct s size=5x5\n  floor\n"
        );
        let found = of_code(&src, DiagnosticCode::DuplicateItem);
        assert_eq!(found.len(), 1, "got {found:#?}");
        assert_eq!(
            found[0].span.start,
            nth(&src, "struct s", 1) + "struct ".len()
        );
    }

    /// A single-valued `@directive` may be declared once.
    ///
    /// `@cairn` and `@intended_targets` each answer a one-answer question,
    /// and neither has a consumer in the compiler yet — so nothing would
    /// choose between two of them.
    #[test]
    fn di_5_each_repeated_single_valued_header_is_reported() {
        let cases = [
            (
                "@cairn",
                "@cairn 2026.06
@cairn 2026.07
",
            ),
            (
                "@intended_targets",
                "@intended_targets [\"1.20.4\"]
@intended_targets [\"1.21.4\"]
",
            ),
        ];
        for (directive, headers) in cases {
            let src = format!(
                "{headers}
{THEME}struct s size=3x3
  floor
"
            );
            let found = of_code(&src, DiagnosticCode::DuplicateHeader);
            assert_eq!(found.len(), 1, "{directive}: got {found:#?}");
            let d = &found[0];
            assert_eq!(d.severity(), Severity::Error);
            assert_eq!(
                d.span.start,
                nth(&src, directive, 1),
                "{directive}: span should start at the repeat",
            );
            assert!(
                d.primary.contains(directive),
                "{directive}: message should name the directive, got: {}",
                d.primary,
            );
            assert!(
                d.notes.iter().any(|n| n.span.is_some()),
                "{directive}: a note should point at the first declaration",
            );
        }
    }

    /// `@requires` is exempt, and that is not an oversight.
    ///
    /// `resolve::version_axes` folds every `version>=X` floor to the
    /// strictest across all of them — the conjunction of the constraints,
    /// not a choice between them — which `RegistryRange`'s doc states and
    /// that module's tests pin. Reporting a repeat here would make an error
    /// of a shape the rest of the crate defines as meaningful, and the
    /// collision would not show up in either test suite because neither
    /// crosses the other's layer. This test is that crossing.
    #[test]
    fn di_5b_repeated_requires_is_legal_because_its_floors_compose() {
        let src = format!(
            "@requires version>=1.20
@requires version>=1.21

{THEME}struct s size=3x3
  floor
"
        );
        let found = of_code(&src, DiagnosticCode::DuplicateHeader);
        assert!(found.is_empty(), "got {found:#?}");
        let module = parse(&src).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        let axes = cairn_lang_core::resolve::compute_axes(&module, &ir, &resolution, Vec::new());
        assert_eq!(
            axes.registry_compat.min, "1.21",
            "the strictest floor must still win",
        );
    }

    /// Three different directives together are the normal case and must stay
    /// silent — the negative space for the test above.
    #[test]
    fn di_6_distinct_header_directives_are_not_duplicates() {
        let src = format!(
            "@cairn 2026.06\n@requires version>=1.20\n@intended_targets [\"1.20.4\"]\n\n{THEME}struct s size=3x3\n  floor\n"
        );
        let found = of_code(&src, DiagnosticCode::DuplicateHeader);
        assert!(found.is_empty(), "got {found:#?}");
    }

    fn resolved(source: &str) -> (cairn_lang_core::resolve::Resolution, BlockArrayIr) {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        let block = lower_to_block_array(&ir, &resolution, None);
        (resolution, block)
    }

    /// Dimensions of a lowered scope, as `(x, y, z)`.
    fn dims(block: &BlockArrayIr, key: &str) -> (u32, u32, u32) {
        let d = block
            .structures
            .get(key)
            .map(|a| a.dims)
            .or_else(|| block.placements.get(key).map(|p| p.dims))
            .unwrap_or_else(|| {
                panic!(
                    "no scope `{key}` in {:?} / {:?}",
                    block.structures.keys().collect::<Vec<_>>(),
                    block.placements.keys().collect::<Vec<_>>(),
                )
            });
        (d.x, d.y, d.z)
    }

    /// The first `def` of a name is the one a `place use=` instantiates *and*
    /// the one the scopes map holds.
    ///
    /// The two used to disagree: the placement lookup walked the def list and
    /// took the first match, while the scopes map was overwritten by the
    /// last. Sizing the placement from one body and resolving its members
    /// from the other is not a diagnostic problem, it is a wrong build, so it
    /// is pinned separately from the diagnostic above.
    #[test]
    fn di_8_the_first_def_binds_for_both_the_placement_and_the_scope() {
        let src = format!(
            "{THEME}def hut size=9x9:\n  floor id=first mat_slot=floor\n\ndef hut size=3x3:\n  floor id=second mat_slot=floor\n\nsite s:\n  place id=a use=hut theme=t at=origin\n"
        );
        let (resolution, block) = resolved(&src);
        assert_eq!(
            dims(&block, "site::s::a"),
            (9, 1, 9),
            "the placement must be sized from the first def",
        );
        let scope = resolution
            .scopes
            .get("def::hut")
            .expect("def scope present");
        let second_decl = nth(&src, "def hut", 1);
        for member_start in scope.members.keys() {
            assert!(
                *member_start < second_decl,
                "the scope must hold the first def's members; got one at byte {member_start}, \
             which is inside the second declaration starting at {second_decl}",
            );
        }
    }

    /// A duplicate `theme` name: the first binding supplies the slot value,
    /// so the lowered palette carries oak rather than stone.
    #[test]
    fn di_9a_the_first_theme_supplies_the_slot_value() {
        let src = "theme t:
  slot floor -> @oak_planks

theme t:
  slot floor -> @stone

struct s size=3x3
  floor mat_slot=floor
";
        let (_, block) = resolved(src);
        let palette: Vec<&str> = block.structures["struct::s"]
            .palette
            .entries
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert!(
            palette.iter().any(|id| id.contains("oak")),
            "the first theme must supply the slot value, got palette {palette:?}",
        );
    }

    /// A duplicate `struct` name: the first header supplies `size=`.
    ///
    /// This one goes through block-array lowering's own `structures` map,
    /// not the resolver's. The resolver's half is invisible in the dims, so
    /// a fix applied only there would leave this test red.
    #[test]
    fn di_9b_the_first_struct_supplies_the_size() {
        let src = format!(
            "{THEME}struct s size=9x9
  floor

struct s size=3x3
  floor
"
        );
        let (_, block) = resolved(&src);
        assert_eq!(dims(&block, "struct::s"), (9, 1, 9));
    }

    /// Two `site` blocks of one name whose places collide on `id=`: the
    /// binding key is `site::NAME::PLACE_ID`, so this is the only site shape
    /// where first-wins has anything to decide.
    #[test]
    fn di_9c_a_place_id_repeated_across_site_blocks_keeps_the_first() {
        let src = format!(
            "{THEME}def big size=9x9:
  floor id=f

def small size=3x3:
  floor id=f

site s:
  place id=a use=big theme=t at=origin

site s:
  place id=a use=small theme=t at=origin
"
        );
        let (_, block) = resolved(&src);
        assert_eq!(dims(&block, "site::s::a"), (9, 1, 9));
        assert_eq!(
            block.placements.len(),
            1,
            "the colliding place must not produce a second entry",
        );
    }

    /// Two `site` blocks of one name whose places do *not* collide: nothing
    /// is dropped. Both build, under one shared `site::s::` namespace.
    ///
    /// This is the shape the diagnostic's note has to describe correctly.
    /// An author told "the first `site s` is the one that resolves" would
    /// delete the second block and lose a placement that was in the build —
    /// so the claim is pinned here rather than left to the prose.
    #[test]
    fn di_9d_site_blocks_of_one_name_merge_rather_than_shadow() {
        let src = format!(
            "{THEME}def big size=9x9:
  floor id=f

def small size=3x3:
  floor id=f

site s:
  place id=a use=big theme=t at=origin

site s:
  place id=b use=small theme=t at=origin
"
        );
        let (resolution, block) = resolved(&src);
        let keys: Vec<&str> = block.placements.keys().map(String::as_str).collect();
        assert_eq!(keys, ["site::s::a", "site::s::b"]);
        assert_eq!(dims(&block, "site::s::a"), (9, 1, 9));
        assert_eq!(dims(&block, "site::s::b"), (3, 1, 3));
        assert!(
            resolution.scopes.contains_key("site::s::a")
                && resolution.scopes.contains_key("site::s::b"),
            "both places must resolve, got {:?}",
            resolution.scopes.keys().collect::<Vec<_>>(),
        );
        // The collision is still worth reporting: the namespace is shared
        // but `east_of=` is not, so the two blocks cannot reference each
        // other's places.
        assert!(codes(&src).contains(&"E_DUPLICATE_ITEM"));
    }

    /// Binding the first declaration does not mean ignoring the rest. The
    /// duplicate's body is still walked, so problems inside it surface in the
    /// same run as the collision that hides it — otherwise fixing the name
    /// would reveal a fresh wave of errors the author never saw.
    #[test]
    fn di_10_the_duplicate_body_still_contributes_its_own_diagnostics() {
        let struct_src = format!(
            "{THEME}struct s size=3x3
  floor mat_slot=floor

struct s size=3x3
  floor mat_slot=nosuchslot
"
        );
        let found = codes(&struct_src);
        assert!(
            found.contains(&"E_DUPLICATE_ITEM") && found.contains(&"E_UNRESOLVED_SLOT"),
            "expected both the collision and the duplicate body's own error, got {found:?}",
        );

        // `theme` is the kind where this is not free. Theme findings are
        // driven by the bound map, which holds one entry per name, so the
        // shadowed body's bad slot value used to vanish with it.
        let theme_src = "theme t:
  slot floor -> @oak_planks

theme t:
  slot floor -> notatoken

struct s size=3x3
  floor mat_slot=floor
";
        let found = codes(theme_src);
        assert!(
            found.contains(&"E_DUPLICATE_ITEM") && found.contains(&"E_UNKNOWN_SLOT_TARGET"),
            "the shadowed theme body's own finding must survive, got {found:?}",
        );
    }
}

mod selector {
    //! Acceptance tests for `E_DUPLICATE_SELECTOR`.
    //!
    //! A theme's selector rows merge into one overlay per member. `resolve`
    //! walks the rows in source order and, for each one the member matches,
    //! inserts every binding into the member's `selector_extras` — so a key
    //! bound by two rows keeps the later value. When the two rows select the
    //! *same* members there is no member left anywhere whose merged bindings
    //! keep the earlier one: the binding the author wrote is dead text, and
    //! would stay dead whatever a binding comes to mean. `check::duplicate`
    //! reported a repeated `slot NAME ->` line and a key repeated inside one
    //! row's brackets, and nothing about the row as a whole, so `cairn check`
    //! exited 0 on it.
    //!
    //! No pass lowers a merged binding yet (`spec/materials-themes` "Slots as
    //! dependency injection"), so neither row's value reaches a block today,
    //! and every binding here also earns `W_IGNORED_ARGUMENT`. This finding is
    //! about the merge, which is decided now, not about what gets built.
    //!
    //! Two shapes are deliberately not findings.
    //!
    //! - **Disjoint bindings.** Two rows with one selector and no key in common
    //!   compose: every binding reaches the merged bindings of every member
    //!   both rows select and nothing is displaced. Splitting a long binding
    //!   list over two lines is a thing an author means to do, and this is the
    //!   same reason `@requires` is exempt from `E_DUPLICATE_HEADER` in the
    //!   same pass.
    //! - **Different attributes.** `window[class=small]` and
    //!   `window[class=small,side=front]` overlap without coinciding — a member
    //!   the first selects and the second does not keeps the first row's
    //!   binding. Which of two *overlapping* rows wins is the cascade, and the
    //!   cascade is source order by design; only a coincidence is a duplicate.

    use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, lower, parse};

    use crate::common::{diagnose, exactly_one, notes};

    fn selector_only(source: &str) -> Vec<Diagnostic> {
        diagnose(source)
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DuplicateSelector)
            .collect()
    }

    /// Every code `source` raises but the `W_IGNORED_ARGUMENT` each selector
    /// binding earns.
    ///
    /// Every row in this file binds something, no pass lowers a binding, and
    /// `check/arguments.rs` pins that finding; counting one per binding in each
    /// table below would make it restate how many bindings its rows carry. What
    /// is dropped is still checked: exactly one binding finding per binding the
    /// rows carry after the merge into a map, and nothing else, so a regression
    /// that adds or loses one, or hides another `W_IGNORED_ARGUMENT` in here,
    /// fails.
    fn row_codes(source: &str) -> Vec<&'static str> {
        let found = diagnose(source);
        let (dropped, kept): (Vec<_>, Vec<_>) = found
            .iter()
            .partition(|d| d.code == DiagnosticCode::IgnoredArgument);
        let module = parse(source).expect("fixtures parse");
        let bindings: usize = lower(&module)
            .themes
            .iter()
            .flat_map(|theme| &theme.selectors)
            .map(|row| row.bindings.len())
            .sum();
        assert_eq!(
            dropped.len(),
            bindings,
            "one W_IGNORED_ARGUMENT per binding, got {found:#?}",
        );
        for d in &dropped {
            assert!(
                d.primary.contains("is bound by a theme selector"),
                "only binding findings may be filtered out here: {d:#?}",
            );
        }
        kept.iter().map(|d| d.code.as_str()).collect()
    }

    fn one(source: &str) -> Diagnostic {
        exactly_one(selector_only(source))
    }

    /// The selector rows are the variable; the struct below them carries one
    /// `window` every row in this file selects. A control that reports nothing
    /// is then a control whose rows *matched* and were still judged distinct,
    /// rather than one whose rows reached no member at all.
    fn theme_with(rows: &str) -> String {
        format!(
            "theme medieval:\n  \
         slot floor -> @oak_planks\n  \
         slot glass -> @glass_pane\n\
         {rows}\n\
         struct cottage size=9x7\n  \
         floor  mat_slot=floor\n  \
         window id=front class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n"
        )
    }

    /// The reported row is the later one — the line the author edits — and the
    /// finding is an error, like every other duplicate this pass reports.
    #[test]
    fn ds_1_the_rebinding_row_is_reported_once_as_an_error() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
        );
        let d = one(&src);
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(
            &src[d.span.clone()],
            "window[class=small] -> frame=@dark_oak_wood",
        );
    }

    /// Every arity of the rebound-key list, with the sentence each renders.
    /// Table-driven because the join has three branches and the serial comma is
    /// a three-or-more rule: the two-key arm is the one that reads
    /// `` `frame=`, and `sill=` `` when the branch is written as one `join`.
    #[test]
    fn ds_2_the_rebound_key_list_reads_at_every_arity() {
        for (second, listed) in [
            ("frame=@dark_oak_wood", "`frame=`"),
            (
                "frame=@dark_oak_wood sill=@stone_slab",
                "`frame=` and `sill=`",
            ),
            (
                "frame=@dark_oak_wood sill=@stone_slab trim=@brick",
                "`frame=`, `sill=`, and `trim=`",
            ),
        ] {
            let src = theme_with(&format!(
                "  window[class=small] -> frame=@spruce_wood sill=@oak_slab trim=@stone\n  \
             window[class=small] -> {second}\n"
            ));
            let d = one(&src);
            assert_eq!(
                d.primary,
                format!(
                    "`window[class=small]` in theme `medieval` selects the same members as an earlier row and rebinds {listed}"
                ),
                "for `{second}`",
            );
        }
    }

    /// The notes: where the displaced binding is, why nothing can still read
    /// it, and what to do. The first is anchored so an editor can jump to the
    /// line whose value is being thrown away.
    #[test]
    fn ds_3_the_notes_point_at_the_displaced_binding_and_say_what_to_do() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
        );
        let d = one(&src);
        let anchored = d.notes[0]
            .span
            .as_ref()
            .expect("the first note points at the row being displaced");
        assert_eq!(
            &src[anchored.clone()],
            "window[class=small] -> frame=@spruce_wood"
        );
        assert_eq!(
            notes(&d),
            vec![
                "`frame=` bound here",
                "rows with the same attributes match exactly the same members, and bindings merge in source order, so every member's merged bindings keep this row's `frame=` and the earlier value is lost (no pass lowers a selector's bindings yet, so neither value is built today)",
                "merge the rows, or narrow one selector so they pick different members",
            ],
        );
        assert!(
            d.notes[1].span.is_none() && d.notes[2].span.is_none(),
            "the explanation and the repair are about the pair, not about a line: {:#?}",
            d.notes,
        );
    }

    /// Notes are one per displaced *row*, not one per displaced key. Two keys
    /// taken from one row read as one sentence about that line; two keys taken
    /// from two rows are two notes, each naming only what its own line lost.
    #[test]
    fn ds_3b_the_anchored_notes_are_one_per_displaced_row() {
        let from_one_row = theme_with(
            "  window[class=small] -> frame=@spruce_wood sill=@oak_slab\n  \
         window[class=small] -> frame=@dark_oak_wood sill=@stone_slab\n",
        );
        let d = one(&from_one_row);
        assert_eq!(
            notes(&d)[0],
            "`frame=` and `sill=` bound here",
            "one line lost both, so naming it twice would be two notes about one edit",
        );
        assert_eq!(d.notes.len(), 3, "and no second anchor: {:#?}", d.notes);

        // The offending row binds `sill=` first, so insertion order would put
        // the note about line 3 ahead of the one about line 2. Notes are sorted
        // by span instead, so they read down the file.
        let from_two_rows = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> sill=@oak_slab\n  \
         window[class=small] -> sill=@stone_slab frame=@dark_oak_wood\n",
        );
        let d = one(&from_two_rows);
        assert_eq!(notes(&d)[0], "`frame=` bound here");
        assert_eq!(notes(&d)[1], "`sill=` bound here");
        let anchors: Vec<&str> = d.notes[..2]
            .iter()
            .map(|n| &from_two_rows[n.span.clone().expect("anchored note")])
            .collect();
        assert_eq!(
            anchors,
            vec![
                "window[class=small] -> frame=@spruce_wood",
                "window[class=small] -> sill=@oak_slab",
            ],
        );
    }

    /// Attribute order is not part of the selector. `window[class=small,side=front]`
    /// and `window[side=front,class=small]` are the same filter, because the
    /// matcher tests every attribute independently.
    #[test]
    fn ds_4_attribute_order_does_not_make_two_selectors_different() {
        let src = theme_with(
            "  window[class=small,side=front] -> frame=@spruce_wood\n  \
         window[side=front,class=small] -> frame=@dark_oak_wood\n",
        );
        assert!(one(&src).primary.contains("rebinds `frame=`"));
    }

    /// Whether `small` and `"small"` are the same attribute is a property of
    /// the *matcher*, not of this pass, and it differs by key: `id` / `class` /
    /// `mat_slot` are compared as label text, everything else by value kind.
    /// Both halves check the matcher's own answer alongside the finding —
    /// `E_THEME_SELECTOR_UNMATCHED` is the matcher saying, in the second half,
    /// that the quoted row reaches no member the bare one reaches.
    #[test]
    fn ds_5_value_form_matters_exactly_where_the_matcher_says_it_does() {
        // `id` / `class` / `mat_slot` are lifted onto the member during
        // lowering and compared as label text, so the quotes are not part of
        // the filter and the rows coincide.
        for attr in ["id=front", "class=small", "mat_slot=glass"] {
            let src = theme_with(&rows_quoting(attr));
            assert_eq!(
                row_codes(&src),
                vec!["E_DUPLICATE_SELECTOR"],
                "a quoted `{attr}` selects the same members, and nothing went unmatched",
            );
        }
        // Every other attribute stays a generic `key=value` and is compared by
        // value kind, where the two spellings are two values. The lone
        // `E_THEME_SELECTOR_UNMATCHED` is the matcher's own answer that the
        // quoted row reaches nothing the bare one reaches.
        for attr in ["side=front", "offset=2"] {
            let src = theme_with(&rows_quoting(attr));
            assert_eq!(
                row_codes(&src),
                vec!["E_THEME_SELECTOR_UNMATCHED"],
                "a quoted `{attr}` selects nothing the bare one selects, so the rows are not a pair",
            );
        }
    }

    /// Two rows binding one key, filtering on `attr` and on `attr` with its
    /// value quoted.
    fn rows_quoting(attr: &str) -> String {
        let (key, value) = attr.split_once('=').expect("`key=value`");
        format!(
            "  window[{attr}] -> frame=@spruce_wood\n  \
         window[{key}=\"{value}\"] -> frame=@dark_oak_wood\n"
        )
    }

    /// A selector that differs in any attribute is a different selector, even
    /// when one row's members are a subset of the other's: the members only the
    /// wider row selects still read its binding.
    #[test]
    fn ds_6_rows_with_different_attributes_are_not_a_pair() {
        for second in [
            "window[class=large] -> frame=@dark_oak_wood",
            "window[class=small,side=front] -> frame=@dark_oak_wood",
            "window[] -> frame=@dark_oak_wood",
        ] {
            let src = theme_with(&format!(
                "  window[class=small] -> frame=@spruce_wood\n  {second}\n"
            ));
            assert!(
                selector_only(&src).is_empty(),
                "`{second}` selects a different set: {:#?}",
                selector_only(&src),
            );
        }
    }

    /// Two rows with one selector and no key in common compose. Nothing is
    /// displaced, so nothing is reported — the same exemption `@requires` has
    /// from `E_DUPLICATE_HEADER`.
    #[test]
    fn ds_7_rows_that_bind_different_keys_compose() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> sill=@stone_slab\n",
        );
        assert!(
            selector_only(&src).is_empty(),
            "got {:#?}",
            selector_only(&src),
        );
    }

    /// Only the shared keys are named. A row that rebinds one key and adds
    /// another is half a duplicate, and the message has to say which half.
    #[test]
    fn ds_8_only_the_keys_both_rows_bind_are_listed() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood sill=@stone_slab\n",
        );
        let d = one(&src);
        assert!(
            d.primary.ends_with("rebinds `frame=`"),
            "`sill=` is added, not rebound: {}",
            d.primary,
        );
    }

    /// With three rows the note points at the row whose value is actually being
    /// replaced, which is the most recent row to bind the key rather than the
    /// first row of the group. Pointing at the first would name a value that
    /// was already gone. The middle row displaces nothing and says nothing.
    #[test]
    fn ds_9_the_note_names_the_binding_that_is_actually_replaced() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@birch_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
        );
        let found = selector_only(&src);
        assert_eq!(
            found.len(),
            2,
            "rows two and three each displace one: {found:#?}"
        );
        let anchors: Vec<&str> = found
            .iter()
            .map(|d| &src[d.notes[0].span.clone().expect("anchored note")])
            .collect();
        assert_eq!(
            anchors,
            vec![
                "window[class=small] -> frame=@spruce_wood",
                "window[class=small] -> frame=@birch_wood",
            ],
        );
    }

    /// A row between two duplicates that binds a key neither of them binds is
    /// not part of the pair, and does not become the anchor.
    #[test]
    fn ds_10_an_intervening_row_binding_another_key_is_not_reported() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> sill=@stone_slab\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
        );
        let d = one(&src);
        assert_eq!(
            &src[d.notes[0].span.clone().expect("anchored note")],
            "window[class=small] -> frame=@spruce_wood",
        );
    }

    /// Selectors are scoped to their theme, so two themes binding the same key
    /// with the same filter are two independent overlays and neither displaces
    /// the other.
    #[test]
    fn ds_11_identical_rows_in_two_themes_are_not_a_pair() {
        let src = "theme medieval:\n  \
         slot glass -> @glass_pane\n  \
         window[class=small] -> frame=@spruce_wood\n\n\
         theme modern:\n  \
         slot glass -> @white_stained_glass\n  \
         window[class=small] -> frame=@dark_oak_wood\n\n\
         struct cottage size=9x7\n  \
         window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
        assert!(
            selector_only(src).is_empty(),
            "got {:#?}",
            selector_only(src),
        );
    }

    /// The keyword is half the filter: two rows that agree on attributes but
    /// name different roles never select one member in common.
    #[test]
    fn ds_12_rows_with_different_keywords_are_not_a_pair() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         door[class=small] -> frame=@dark_oak_wood\n",
        );
        assert!(
            selector_only(&src).is_empty(),
            "got {:#?}",
            selector_only(&src),
        );
    }

    /// The payload carries the rebound keys, so a quick-fix does not parse the
    /// sentence back apart — what `spec/lint` "Machine-readable payload" exists
    /// to stop.
    #[test]
    fn ds_13_the_payload_lists_the_rebound_keys() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood sill=@oak_slab\n  \
         window[class=small] -> sill=@stone_slab frame=@dark_oak_wood\n",
        );
        let d = one(&src);
        let value = serde_json::to_value(&d.data).expect("serialise payload");
        assert_eq!(
            value,
            serde_json::json!({"kind": "duplicate_selector", "rebound": ["sill", "frame"]}),
            "in the order the message lists them, which is the offending row's order",
        );
    }

    /// The finding coexists with the arg-level duplicate inside one row rather
    /// than masking it: the two are different scopes, and both are on lines the
    /// author has to edit.
    #[test]
    fn ds_14_it_coexists_with_a_key_repeated_inside_one_row() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@birch_wood frame=@dark_oak_wood\n",
        );
        let mut seen = row_codes(&src);
        seen.sort_unstable();
        assert_eq!(seen, vec!["E_DUPLICATE_ARG", "E_DUPLICATE_SELECTOR"]);
    }

    /// The selector is rendered, not elided, so two distinct pairs in one
    /// theme do not produce byte-identical primaries — the reason
    /// `incomplete_place_diag` names its subject. The rendering keeps source
    /// order and the source spelling of each value, so it reads as the line
    /// the author has to find.
    #[test]
    fn ds_16_the_primary_renders_the_selector_that_identifies_the_row() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n  \
         window[side=front,y=2] -> frame=@birch_wood\n  \
         window[side=front,y=2] -> frame=@oak_wood\n",
        );
        let rendered: Vec<String> = selector_only(&src)
            .iter()
            .map(|d| {
                d.primary
                    .split_once("` in theme")
                    .expect("the primary opens with the rendered selector")
                    .0
                    .trim_start_matches('`')
                    .to_owned()
            })
            .collect();
        assert_eq!(rendered, ["window[class=small]", "window[side=front,y=2]"]);
    }

    /// A label key holding a non-label value pairs with nothing — not even a
    /// byte-identical row. `value_eq_label` rejects such a value for every
    /// member, so neither row binds anything the other could take over, and
    /// `select_the_same_members` is not even reflexive there. The rows are
    /// already an error by a different scope, which is what keeps the gap from
    /// costing the author anything.
    #[test]
    fn ds_17_a_label_key_holding_a_non_label_value_pairs_with_nothing() {
        for attr in ["id=5", "class=5", "mat_slot=5", "class=true"] {
            let src = theme_with(&format!(
                "  window[{attr}] -> frame=@spruce_wood\n  \
             window[{attr}] -> frame=@dark_oak_wood\n"
            ));
            let mut seen = row_codes(&src);
            seen.sort_unstable();
            assert_eq!(
                seen,
                vec![
                    "E_THEME_SELECTOR_UNMATCHED",
                    "E_THEME_SELECTOR_UNMATCHED",
                    "E_TYPE_MISMATCH_LABEL",
                    "E_TYPE_MISMATCH_LABEL",
                ],
                "`{attr}` matches no member, so the identical rows displace nothing",
            );
        }
    }

    /// A `theme` block whose name lost the binding is still checked. `lower`
    /// keeps one `ThemeIr` per block with no keying by name, and this pass
    /// walks that list — the resolver's first-write-wins dedupe happens later
    /// and elsewhere. Without this, keying themes by name in `lower` would
    /// silently stop checking the shadowed body.
    #[test]
    fn ds_18_a_theme_whose_name_lost_the_binding_is_still_checked() {
        let src = "theme medieval:\n  \
         slot glass -> @glass_pane\n  \
         window[class=small] -> frame=@spruce_wood\n\n\
         theme medieval:\n  \
         slot glass -> @white_stained_glass\n  \
         window[class=small] -> frame=@birch_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n\n\
         struct cottage size=9x7\n  \
         window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
        let found = selector_only(src);
        assert_eq!(found.len(), 1, "got {found:#?}");
        assert_eq!(
            &src[found[0].span.clone()],
            "window[class=small] -> frame=@dark_oak_wood",
            "the pair inside the shadowed block, not the one spanning the blocks",
        );
    }

    /// Two coinciding rows that reach no member are still a pair. The pass is
    /// syntactic — it reads the theme body, not the struct bodies — and the
    /// rows are as redundant with no member as with one. `W_`-style reporting
    /// of the dead rows is `E_THEME_SELECTOR_UNMATCHED`'s job, and both fire.
    #[test]
    fn ds_15_a_pair_that_matches_nothing_is_still_a_pair() {
        let src = theme_with(
            "  window[class=enormous] -> frame=@spruce_wood\n  \
         window[class=enormous] -> frame=@dark_oak_wood\n",
        );
        let mut seen = row_codes(&src);
        seen.sort_unstable();
        assert_eq!(
            seen,
            vec![
                "E_DUPLICATE_SELECTOR",
                "E_THEME_SELECTOR_UNMATCHED",
                "E_THEME_SELECTOR_UNMATCHED",
            ],
        );
        assert!(
            !one(&src).primary.contains("no member"),
            "the primary states the coincidence, not a loss it cannot know happened: {}",
            one(&src).primary,
        );
    }
}
