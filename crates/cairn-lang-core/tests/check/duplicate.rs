//! The `duplicate` pass: a name, key, header or selector row stated twice errs on the repeat.

use cairn_lang_core::{Diagnostic, DiagnosticCode};

use crate::common::{diagnose, exactly_one, notes, nth, slice};

#[test]
fn duplicate_size_flags_second_occurrence_only() {
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
fn duplicate_theme_slot_is_reported() {
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
fn duplicate_arg_inside_statement_args() {
    let src = "struct s size=1x1\n  walls height=4 height=5 mat_slot=m\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DiagnosticCode::DuplicateArg);
    assert_eq!(slice(src, &diags[0]), "height=5");
}

#[test]
fn duplicate_id_in_same_body_scope() {
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
fn id_in_nested_scope_does_not_clash_with_outer() {
    let src = "struct s size=3x3\n  door id=front\n  level y=0\n    door id=front\n";
    let diags = diagnose(src);
    assert!(
        diags.is_empty(),
        "scopes should be per-body, got {diags:#?}",
    );
}

#[test]
fn duplicate_selector_attribute_is_reported() {
    let src = "struct s size=1x1\n  door[id=front id=back] at=center\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, DiagnosticCode::DuplicateArg);
    assert_eq!(slice(src, &diags[0]), "id=back");
}

#[test]
fn duplicate_header_arg_other_than_size_is_arg_not_size() {
    // The surviving `class=` is also `W_IGNORED_ARGUMENT`, since no pass reads it.
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

/// The theme and def a `place use=hut theme=t` row names.
const HUT: &str = "theme t:\n  slot floor -> @oak_planks\n\n\
                   def hut size=3x3:\n  floor mat_slot=floor\n\n";

fn of_code(diags: &[Diagnostic], code: DiagnosticCode) -> Vec<&Diagnostic> {
    diags.iter().filter(|d| d.code == code).collect()
}

/// The text the first note of `diag` points at: the declaration that stands.
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
fn a_repeated_place_id_is_reported_once_as_duplicate_place_id() {
    // `spec/lint` "Sites and placements" gives this case `E_DUPLICATE_PLACE_ID` alone.
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
fn a_site_body_still_reports_a_repeated_id_on_rows_other_than_place() {
    // Each `door` here is also `E_MISPLACED_MEMBER`.
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
fn a_place_row_and_a_connect_row_sharing_an_id_are_duplicate_id() {
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
fn a_place_row_and_a_misplaced_row_sharing_an_id_are_duplicate_id() {
    // Either order: the exemption needs both rows to be `place` rows.
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
fn place_rows_nested_in_a_site_body_are_still_duplicate_id() {
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
fn place_rows_in_a_def_body_are_still_duplicate_id() {
    // A `place` row in a `def` is also `E_MISPLACED_MEMBER`.
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
fn a_repeated_string_place_id_is_reported_once_as_duplicate_place_id() {
    let src = format!(
        "{HUT}site v:\n  place id=\"a\" use=hut theme=t at=origin\n  \
         place id=\"a\" use=hut theme=t east_of=a gap=2\n"
    );
    let found = exactly_one(diagnose(&src));
    assert_eq!(found.code, DiagnosticCode::DuplicatePlaceId);
    assert_eq!(found.span.start, nth(&src, "place id=", 1));
}

#[test]
fn a_place_row_repeating_a_connect_row_and_a_place_row_is_reported_twice() {
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
fn a_repeated_refused_place_id_is_still_duplicate_place_id() {
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

/// The repair note every `E_DUPLICATE_CIRCUIT` carries after the one on the first line.
const DUPLICATE_CIRCUIT_REPAIR: &str = "delete one of the two lines, or split the logic across \
                                        several scopes, each with its own `circuit` line";

#[test]
fn a_second_circuit_line_in_a_scope_is_refused_against_the_first() {
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

#[test]
fn a_circuit_line_after_an_unusable_one_is_still_a_second_line() {
    let src = "def d size=7x5\n  floor mat_slot=m\n  circuit region=floor void=0\n  \
               circuit region=floor void=2\n";
    let diags = diagnose(src);
    let found = of_code(&diags, DiagnosticCode::DuplicateCircuit);
    assert_eq!(found.len(), 1, "{diags:#?}");
    assert_eq!(slice(src, found[0]), "circuit region=floor void=2");
    assert_eq!(note_text(src, found[0]), "circuit region=floor void=0");
}

#[test]
fn one_circuit_line_per_scope_is_not_a_duplicate() {
    let src = "struct a size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n\n\
               struct b size=7x5\n  floor mat_slot=m\n  level y=1\n    floor mat_slot=m\n  \
               circuit region=floor void=1\n";
    let diags = diagnose(src);
    assert!(diags.is_empty(), "{diags:#?}");
}

/// A line under a `level` is `E_MISPLACED_MEMBER` on its own, so it is not also counted here.
#[test]
fn a_circuit_line_under_a_level_is_not_counted_toward_the_scope() {
    let src = "struct s size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n  \
               level y=1\n    circuit region=basement void=2\n  \
               circuit region=attic void=3\n";
    let diags = diagnose(src);
    let at = |line: &str| nth(src, line, 0)..nth(src, line, 0) + line.len();
    let found: Vec<_> = diags
        .iter()
        .map(|d| (d.code.as_str(), d.span.clone()))
        .collect();
    assert_eq!(
        found,
        [
            ("E_MISPLACED_MEMBER", at("circuit region=basement void=2")),
            ("E_DUPLICATE_CIRCUIT", at("circuit region=attic void=3")),
        ],
        "{diags:#?}"
    );
    assert_eq!(note_text(src, &diags[1]), "circuit region=floor void=1");
}

#[test]
fn a_circuit_line_with_a_selector_counts() {
    let src = "struct s size=7x5\n  floor mat_slot=m\n  circuit region=floor void=1\n  \
               circuit[id=x] region=basement void=2\n";
    let diags = diagnose(src);
    let found = of_code(&diags, DiagnosticCode::DuplicateCircuit);
    assert_eq!(found.len(), 1, "{diags:#?}");
    assert_eq!(diags.len(), 1, "{diags:#?}");
    assert_eq!(slice(src, found[0]), "circuit[id=x] region=basement void=2");
    assert_eq!(note_text(src, found[0]), "circuit region=floor void=1");
}

/// The missing size is `W_STRUCT_NO_SIZE`, which block-array lowering raises and `check` does not.
#[test]
fn two_circuit_lines_in_a_scope_with_no_size_are_refused() {
    let src = "struct s\n  floor mat_slot=m\n  circuit region=floor void=1\n  \
               circuit region=basement void=2\n";
    let diags = diagnose(src);
    let found = of_code(&diags, DiagnosticCode::DuplicateCircuit);
    assert_eq!(found.len(), 1, "{diags:#?}");
    assert_eq!(diags.len(), 1, "{diags:#?}");
    assert_eq!(slice(src, found[0]), "circuit region=basement void=2");
    assert_eq!(note_text(src, found[0]), "circuit region=floor void=1");
}

#[test]
fn a_second_circuit_line_with_an_indented_body_is_refused_beside_the_nesting() {
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
    //! Top-level names and headers: a repeat errs on the repeat and the first declaration binds.
    //! Names are per kind, and `site` blocks of one name merge.

    use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
    use cairn_lang_core::{DiagnosticCode, Severity, lower, parse, resolve};

    use crate::common::{codes, nth, of_code, slice};

    const THEME: &str = "theme t:\n  slot floor -> @oak_planks\n\n";

    #[test]
    fn each_item_kind_reports_its_own_duplicate_name() {
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

    #[test]
    fn the_anchor_follows_the_name_through_padded_whitespace() {
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

    #[test]
    fn every_repeat_after_the_first_is_reported() {
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

    #[test]
    fn the_same_name_on_different_kinds_is_not_a_collision() {
        let src = "theme x:\n  slot floor -> @oak_planks\n\n\
struct x size=3x3\n  floor mat_slot=floor\n\n\
def x size=3x3:\n  floor id=f mat_slot=floor\n\n\
site x:\n  place id=a use=x theme=x at=origin\n";
        let found = of_code(src, DiagnosticCode::DuplicateItem);
        assert!(found.is_empty(), "got {found:#?}");
    }

    #[test]
    fn duplicates_separated_by_other_items_are_still_reported() {
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

    #[test]
    fn each_repeated_single_valued_header_is_reported() {
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

    #[test]
    fn repeated_requires_is_legal_because_its_floors_compose() {
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
        let axes = cairn_lang_core::resolve::compute_axes(&module, Vec::new());
        assert_eq!(
            axes.registry_compat.min, "1.21",
            "the strictest floor must still win",
        );
    }

    #[test]
    fn distinct_header_directives_are_not_duplicates() {
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

    #[test]
    fn the_first_def_binds_for_both_the_placement_and_the_scope() {
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

    #[test]
    fn the_first_theme_supplies_the_slot_value() {
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

    #[test]
    fn the_first_struct_supplies_the_size() {
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

    #[test]
    fn a_place_id_repeated_across_site_blocks_keeps_the_first() {
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

    #[test]
    fn site_blocks_of_one_name_merge_rather_than_shadow() {
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
        // Still reported: the two blocks cannot reference each other's places.
        assert!(codes(&src).contains(&"E_DUPLICATE_ITEM"));
    }

    #[test]
    fn the_duplicate_body_still_contributes_its_own_diagnostics() {
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
    //! `E_DUPLICATE_SELECTOR`: a theme row rebinding a key that a row with the same selector binds.
    //! Rows binding different keys compose, and overlapping selectors cascade.

    use cairn_lang_core::{DiagnosticCode, Severity, lower, parse};

    use crate::common::{diagnose, notes, of_code, one_of_code};

    /// Every code `source` raises but the one `W_IGNORED_ARGUMENT` per selector binding.
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

    /// `rows` above a struct whose one `window` every row in this file selects.
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

    #[test]
    fn the_rebinding_row_is_reported_once_as_an_error() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
        );
        let d = one_of_code(&src, DiagnosticCode::DuplicateSelector);
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(
            &src[d.span.clone()],
            "window[class=small] -> frame=@dark_oak_wood",
        );
    }

    #[test]
    fn the_rebound_key_list_reads_at_every_arity() {
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
            let d = one_of_code(&src, DiagnosticCode::DuplicateSelector);
            assert_eq!(
                d.primary,
                format!(
                    "`window[class=small]` in theme `medieval` selects the same members as an earlier row and rebinds {listed}"
                ),
                "for `{second}`",
            );
        }
    }

    #[test]
    fn the_notes_point_at_the_displaced_binding_and_say_what_to_do() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
        );
        let d = one_of_code(&src, DiagnosticCode::DuplicateSelector);
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

    #[test]
    fn the_anchored_notes_are_one_per_displaced_row() {
        let from_one_row = theme_with(
            "  window[class=small] -> frame=@spruce_wood sill=@oak_slab\n  \
         window[class=small] -> frame=@dark_oak_wood sill=@stone_slab\n",
        );
        let d = one_of_code(&from_one_row, DiagnosticCode::DuplicateSelector);
        assert_eq!(
            notes(&d)[0],
            "`frame=` and `sill=` bound here",
            "one line lost both, so naming it twice would be two notes about one edit",
        );
        assert_eq!(d.notes.len(), 3, "and no second anchor: {:#?}", d.notes);

        // The offending row binds `sill=` first, so only sorting by span puts line 2's note first.
        let from_two_rows = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> sill=@oak_slab\n  \
         window[class=small] -> sill=@stone_slab frame=@dark_oak_wood\n",
        );
        let d = one_of_code(&from_two_rows, DiagnosticCode::DuplicateSelector);
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

    #[test]
    fn attribute_order_does_not_make_two_selectors_different() {
        let src = theme_with(
            "  window[class=small,side=front] -> frame=@spruce_wood\n  \
         window[side=front,class=small] -> frame=@dark_oak_wood\n",
        );
        assert!(
            one_of_code(&src, DiagnosticCode::DuplicateSelector)
                .primary
                .contains("rebinds `frame=`")
        );
    }

    #[test]
    fn value_form_matters_exactly_where_the_matcher_says_it_does() {
        // `id`, `class` and `mat_slot` are lifted onto the member and compared as label text.
        for attr in ["id=front", "class=small", "mat_slot=glass"] {
            let src = theme_with(&rows_quoting(attr));
            assert_eq!(
                row_codes(&src),
                vec!["E_DUPLICATE_SELECTOR"],
                "a quoted `{attr}` selects the same members, and nothing went unmatched",
            );
        }
        // Every other attribute is compared by value kind.
        for attr in ["side=front", "offset=2"] {
            let src = theme_with(&rows_quoting(attr));
            assert_eq!(
                row_codes(&src),
                vec!["E_THEME_SELECTOR_UNMATCHED"],
                "a quoted `{attr}` selects nothing the bare one selects, so the rows are not a pair",
            );
        }
    }

    /// Two rows binding one key, filtering on `attr` bare and quoted.
    fn rows_quoting(attr: &str) -> String {
        let (key, value) = attr.split_once('=').expect("`key=value`");
        format!(
            "  window[{attr}] -> frame=@spruce_wood\n  \
         window[{key}=\"{value}\"] -> frame=@dark_oak_wood\n"
        )
    }

    #[test]
    fn rows_with_different_attributes_are_not_a_pair() {
        for second in [
            "window[class=large] -> frame=@dark_oak_wood",
            "window[class=small,side=front] -> frame=@dark_oak_wood",
            "window[] -> frame=@dark_oak_wood",
        ] {
            let src = theme_with(&format!(
                "  window[class=small] -> frame=@spruce_wood\n  {second}\n"
            ));
            assert!(
                of_code(&src, DiagnosticCode::DuplicateSelector).is_empty(),
                "`{second}` selects a different set: {:#?}",
                of_code(&src, DiagnosticCode::DuplicateSelector),
            );
        }
    }

    #[test]
    fn rows_that_bind_different_keys_compose() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> sill=@stone_slab\n",
        );
        assert!(
            of_code(&src, DiagnosticCode::DuplicateSelector).is_empty(),
            "got {:#?}",
            of_code(&src, DiagnosticCode::DuplicateSelector),
        );
    }

    #[test]
    fn only_the_keys_both_rows_bind_are_listed() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood sill=@stone_slab\n",
        );
        let d = one_of_code(&src, DiagnosticCode::DuplicateSelector);
        assert!(
            d.primary.ends_with("rebinds `frame=`"),
            "`sill=` is added, not rebound: {}",
            d.primary,
        );
    }

    #[test]
    fn the_note_names_the_binding_that_is_actually_replaced() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@birch_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
        );
        let found = of_code(&src, DiagnosticCode::DuplicateSelector);
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

    #[test]
    fn an_intervening_row_binding_another_key_is_not_reported() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> sill=@stone_slab\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
        );
        let d = one_of_code(&src, DiagnosticCode::DuplicateSelector);
        assert_eq!(
            &src[d.notes[0].span.clone().expect("anchored note")],
            "window[class=small] -> frame=@spruce_wood",
        );
    }

    #[test]
    fn identical_rows_in_two_themes_are_not_a_pair() {
        let src = "theme medieval:\n  \
         slot glass -> @glass_pane\n  \
         window[class=small] -> frame=@spruce_wood\n\n\
         theme modern:\n  \
         slot glass -> @white_stained_glass\n  \
         window[class=small] -> frame=@dark_oak_wood\n\n\
         struct cottage size=9x7\n  \
         window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
        assert!(
            of_code(src, DiagnosticCode::DuplicateSelector).is_empty(),
            "got {:#?}",
            of_code(src, DiagnosticCode::DuplicateSelector),
        );
    }

    #[test]
    fn rows_with_different_keywords_are_not_a_pair() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         door[class=small] -> frame=@dark_oak_wood\n",
        );
        assert!(
            of_code(&src, DiagnosticCode::DuplicateSelector).is_empty(),
            "got {:#?}",
            of_code(&src, DiagnosticCode::DuplicateSelector),
        );
    }

    /// `spec/lint` "Machine-readable payload".
    #[test]
    fn the_payload_lists_the_rebound_keys() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood sill=@oak_slab\n  \
         window[class=small] -> sill=@stone_slab frame=@dark_oak_wood\n",
        );
        let d = one_of_code(&src, DiagnosticCode::DuplicateSelector);
        let value = serde_json::to_value(&d.data).expect("serialise payload");
        assert_eq!(
            value,
            serde_json::json!({"kind": "duplicate_selector", "rebound": ["sill", "frame"]}),
            "in the order the message lists them, which is the offending row's order",
        );
    }

    #[test]
    fn it_coexists_with_a_key_repeated_inside_one_row() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@birch_wood frame=@dark_oak_wood\n",
        );
        let mut seen = row_codes(&src);
        seen.sort_unstable();
        assert_eq!(seen, vec!["E_DUPLICATE_ARG", "E_DUPLICATE_SELECTOR"]);
    }

    #[test]
    fn the_primary_renders_the_selector_that_identifies_the_row() {
        let src = theme_with(
            "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n  \
         window[side=front,y=2] -> frame=@birch_wood\n  \
         window[side=front,y=2] -> frame=@oak_wood\n",
        );
        let rendered: Vec<String> = of_code(&src, DiagnosticCode::DuplicateSelector)
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

    /// Not even a byte-identical row: such a value matches no member.
    #[test]
    fn a_label_key_holding_a_non_label_value_pairs_with_nothing() {
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

    #[test]
    fn a_theme_whose_name_lost_the_binding_is_still_checked() {
        let src = "theme medieval:\n  \
         slot glass -> @glass_pane\n  \
         window[class=small] -> frame=@spruce_wood\n\n\
         theme medieval:\n  \
         slot glass -> @white_stained_glass\n  \
         window[class=small] -> frame=@birch_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n\n\
         struct cottage size=9x7\n  \
         window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
        let found = of_code(src, DiagnosticCode::DuplicateSelector);
        assert_eq!(found.len(), 1, "got {found:#?}");
        assert_eq!(
            &src[found[0].span.clone()],
            "window[class=small] -> frame=@dark_oak_wood",
            "the pair inside the shadowed block, not the one spanning the blocks",
        );
    }

    #[test]
    fn a_pair_that_matches_nothing_is_still_a_pair() {
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
            !one_of_code(&src, DiagnosticCode::DuplicateSelector)
                .primary
                .contains("no member"),
            "the primary states the coincidence, not a loss it cannot know happened: {}",
            one_of_code(&src, DiagnosticCode::DuplicateSelector).primary,
        );
    }
}
