//! Acceptance tests for the `type_mismatch` pass of
//! `cairn_lang_core::check`.

use cairn_lang_core::{DiagnosticCode, check, lower, parse};

fn diagnose(source: &str) -> Vec<cairn_lang_core::Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn slice<'a>(source: &'a str, diag: &cairn_lang_core::Diagnostic) -> &'a str {
    &source[diag.span.clone()]
}

#[test]
fn tm_1_id_set_to_token_is_flagged_as_label_mismatch() {
    let src = "struct s size=1x1\n  walls id=@oak mat_slot=m\n";
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
    let src = "struct s size=1x1\n  walls class=foo.bar mat_slot=m\n";
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
    let src = "struct s size=5\n  floor mat_slot=m\n";
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
    // `at=` is a door anchor (spec syntax §5.4) and a window does not read
    // it; the second argument is here only so the line under test carries
    // more than the one key it is about.
    let src = "struct s size=4x4\n  window size=2x2 side=front\n";
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

/// Site prologue with one def and one theme, so a `place` row can name
/// both and only the key under test is wrong.
const PLACE_PRELUDE: &str = "theme plain:\n  \
slot floor -> @oak_planks\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n\n";

/// `use=` and `theme=` are label-typed like `id=` / `class=` /
/// `mat_slot=`, but they were outside `LABEL_KEYS` because lowering does
/// not hoist them onto `Member`'s own fields. The author cannot see that
/// difference; what they saw was the resolver's silent `continue`, which
/// drops the entire placement.
#[test]
fn tm_7_place_use_and_theme_with_a_non_label_value_are_flagged() {
    for (row, offending) in [
        ("place id=a use=3 theme=plain at=origin", "3"),
        ("place id=a use=hut theme=7 at=origin", "7"),
        ("place id=a use=@oak theme=plain at=origin", "@oak"),
        ("place id=a use=hut theme=x.y at=origin", "x.y"),
    ] {
        let src = format!("{PLACE_PRELUDE}site s:\n  {row}\n");
        let found: Vec<_> = diagnose(&src)
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::TypeMismatchLabel)
            .collect();
        assert_eq!(found.len(), 1, "for `{row}`, got {found:#?}");
        assert_eq!(slice(&src, &found[0]), offending, "for `{row}`");
    }
}

/// A string literal is a label, so the well-formed forms stay quiet —
/// otherwise the test above would pass on a pass that flagged every
/// `use=`.
#[test]
fn tm_8_place_use_and_theme_accept_identifiers_and_strings() {
    for row in [
        "place id=a use=hut theme=plain at=origin",
        "place id=a use=\"hut\" theme=\"plain\" at=origin",
    ] {
        let src = format!("{PLACE_PRELUDE}site s:\n  {row}\n");
        let found: Vec<_> = diagnose(&src)
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::TypeMismatchLabel)
            .collect();
        assert!(found.is_empty(), "for `{row}`, got {found:#?}");
    }
}

/// An absent key is a different case from a mistyped one: `use=` is
/// optional in the surface grammar, and the resolver's skip for it is
/// deliberate. Only the present-but-wrong shape belongs to this pass.
#[test]
fn tm_9_an_absent_use_or_theme_is_not_a_type_mismatch() {
    for row in [
        "place id=a theme=plain at=origin",
        "place id=a use=hut at=origin",
    ] {
        let src = format!("{PLACE_PRELUDE}site s:\n  {row}\n");
        let found: Vec<_> = diagnose(&src)
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::TypeMismatchLabel)
            .collect();
        assert!(found.is_empty(), "for `{row}`, got {found:#?}");
    }
}
