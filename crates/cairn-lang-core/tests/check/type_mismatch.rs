//! `E_TYPE_MISMATCH_LABEL` and `E_TYPE_MISMATCH_SIZE`: a label or size key given a value of
//! another shape is an error, and an absent key is not a type mismatch.

use cairn_lang_core::DiagnosticCode;

use crate::common::{diagnose, notes, of_code, slice};

#[test]
fn id_set_to_token_is_flagged_as_label_mismatch() {
    let src = "struct s size=1x1\n  walls id=@oak mat_slot=m\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::TypeMismatchLabel);
    assert_eq!(slice(src, &diags[0]), "@oak");
    assert_eq!(
        notes(&diags[0]),
        ["write a name, such as `id=main`, or a quoted string"]
    );
    assert!(
        diags[0].primary.contains("token"),
        "primary should name the offending kind, got: {}",
        diags[0].primary,
    );
}

#[test]
fn a_mat_slot_given_a_block_says_to_bind_it_in_the_theme() {
    for (src, note) in [
        (
            "struct s size=1x1\n  walls mat_slot=@oak_planks\n",
            "`mat_slot=` names a slot of the bound theme, not a block: bind the block there \
             (`slot NAME -> @oak_planks`) and write `mat_slot=NAME`",
        ),
        (
            "struct s size=1x1\n  walls mat_slot=3\n",
            "write the name of a slot the bound theme declares",
        ),
        (
            "struct s size=1x1\n  walls mat_slot=minecraft:oak_log[axis=x]\n",
            "`mat_slot=` names a slot of the bound theme, not a block: bind its canonical token \
             there (`slot NAME -> @oak_log[axis=x]`) and write `mat_slot=NAME`, or place \
             `minecraft:oak_log[axis=x]` as written with a `raw_block` or `raw_fill`",
        ),
        (
            "struct s size=1x1\n  walls mat_slot=my_mod:copper_lamp\n",
            "`mat_slot=` names a slot of the bound theme, not a block: bind a canonical token \
             there and write `mat_slot=NAME`, or place `my_mod:copper_lamp` as written with a \
             `raw_block` or `raw_fill`",
        ),
    ] {
        let found = of_code(src, DiagnosticCode::TypeMismatchLabel);
        assert_eq!(found.len(), 1, "for {src:?}, got {found:#?}");
        assert_eq!(notes(&found[0]), [note], "for {src:?}");
    }
}

#[test]
fn a_coordinate_on_a_label_or_size_key_is_a_type_mismatch() {
    for (src, code, primary) in [
        (
            "struct s size=1x1\n  walls mat_slot=1,2,3\n",
            DiagnosticCode::TypeMismatchLabel,
            "`mat_slot=` expects a label (identifier or string), got coordinate",
        ),
        (
            "struct s size=1,2,3\n  floor mat_slot=m\n",
            DiagnosticCode::TypeMismatchSize,
            "`size=` expects a `WxH` literal, got coordinate",
        ),
    ] {
        let found = of_code(src, code);
        assert_eq!(found.len(), 1, "for {src:?}, got {found:#?}");
        assert_eq!(
            (slice(src, &found[0]), found[0].primary.as_str()),
            ("1,2,3", primary),
            "for {src:?}",
        );
    }
}

#[test]
fn a_decimal_or_a_compound_on_a_label_or_size_key_is_a_type_mismatch() {
    let label = "`mat_slot=` expects a label (identifier or string), got";
    let size = "`size=` expects a `WxH` literal, got";
    for (src, code, written, primary) in [
        (
            "struct s size=1x1\n  walls mat_slot=1.5\n",
            DiagnosticCode::TypeMismatchLabel,
            "1.5",
            format!("{label} decimal"),
        ),
        (
            "struct s size=1x1\n  walls mat_slot={a:1}\n",
            DiagnosticCode::TypeMismatchLabel,
            "{a:1}",
            format!("{label} NBT compound"),
        ),
        (
            "struct s size=1.5\n  floor mat_slot=m\n",
            DiagnosticCode::TypeMismatchSize,
            "1.5",
            format!("{size} decimal"),
        ),
        (
            "struct s size={a:1}\n  floor mat_slot=m\n",
            DiagnosticCode::TypeMismatchSize,
            "{a:1}",
            format!("{size} NBT compound"),
        ),
    ] {
        let found = of_code(src, code);
        assert_eq!(found.len(), 1, "for {src:?}, got {found:#?}");
        assert_eq!(
            (slice(src, &found[0]), found[0].primary.as_str()),
            (written, primary.as_str()),
            "for {src:?}",
        );
    }
}

#[test]
fn a_block_id_on_a_size_key_is_a_type_mismatch() {
    let src = "struct s size=minecraft:stone\n  floor mat_slot=m\n";
    let found = of_code(src, DiagnosticCode::TypeMismatchSize);
    assert_eq!(found.len(), 1, "got {found:#?}");
    assert_eq!(
        (slice(src, &found[0]), found[0].primary.as_str()),
        (
            "minecraft:stone",
            "`size=` expects a `WxH` literal, got block id"
        ),
    );
}

#[test]
fn class_set_to_dotted_ref_is_flagged() {
    let src = "struct s size=1x1\n  walls class=foo.bar mat_slot=m\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::TypeMismatchLabel);
    assert_eq!(slice(src, &diags[0]), "foo.bar");
}

#[test]
fn mat_slot_string_value_is_accepted() {
    let src = "struct s size=1x1\n  walls mat_slot=\"wall\"\n";
    let diags = diagnose(src);
    assert!(
        diags.is_empty(),
        "string label values are valid, got {diags:#?}"
    );
}

#[test]
fn struct_size_set_to_integer_is_flagged() {
    let src = "struct s size=5\n  floor mat_slot=m\n";
    let diags = diagnose(src);
    assert_eq!(diags.len(), 1, "got {diags:#?}");
    assert_eq!(diags[0].code, DiagnosticCode::TypeMismatchSize);
    assert_eq!(slice(src, &diags[0]), "5");
    assert_eq!(
        notes(&diags[0]),
        ["write two whole numbers joined by `x`, such as `size=9x7`"]
    );
    assert!(
        diags[0].primary.contains("integer"),
        "primary should name the offending kind, got: {}",
        diags[0].primary,
    );
}

#[test]
fn window_size_with_proper_literal_passes() {
    let src = "struct s size=4x4\n  window size=2x2 side=front\n";
    let diags = diagnose(src);
    assert!(
        diags.is_empty(),
        "`size=WxH` on body statements is legal, got {diags:#?}",
    );
}

#[test]
fn a_chests_size_is_its_state_and_not_a_footprint() {
    let src = "struct s size=5x5\n  chest at=inside.front offset=1 size=single\n  \
               window side=front offset=2 y=1 size=single\n";
    let found = of_code(src, DiagnosticCode::TypeMismatchSize);
    let at: Vec<usize> = found.iter().map(|d| d.span.start).collect();
    assert_eq!(at, [src.rfind("single").expect("the window's size")]);
}

#[test]
fn duplicate_size_and_size_mismatch_pass_independently() {
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

/// A theme and a def a `place` row can name, so only the key under test is wrong.
const PLACE_PRELUDE: &str = "theme plain:\n  \
slot floor -> @oak_planks\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n\n";

/// The resolver drops a placement whose value is not a label without a word.
#[test]
fn place_use_and_theme_with_a_non_label_value_are_flagged() {
    let def_note = "write the name of a `def` this file declares";
    let theme_note = "write the name of a `theme` this file declares";
    for (row, offending, note) in [
        ("place id=a use=3 theme=plain at=origin", "3", def_note),
        ("place id=a use=hut theme=7 at=origin", "7", theme_note),
        (
            "place id=a use=@oak theme=plain at=origin",
            "@oak",
            def_note,
        ),
        ("place id=a use=hut theme=x.y at=origin", "x.y", theme_note),
    ] {
        let src = format!("{PLACE_PRELUDE}site s:\n  {row}\n");
        let found = of_code(&src, DiagnosticCode::TypeMismatchLabel);
        assert_eq!(found.len(), 1, "for `{row}`, got {found:#?}");
        assert_eq!(slice(&src, &found[0]), offending, "for `{row}`");
        assert_eq!(notes(&found[0]), [note], "for `{row}`");
    }
}

#[test]
fn place_use_and_theme_accept_identifiers_and_strings() {
    for row in [
        "place id=a use=hut theme=plain at=origin",
        "place id=a use=\"hut\" theme=\"plain\" at=origin",
    ] {
        let src = format!("{PLACE_PRELUDE}site s:\n  {row}\n");
        let found = of_code(&src, DiagnosticCode::TypeMismatchLabel);
        assert!(found.is_empty(), "for `{row}`, got {found:#?}");
    }
}

/// An absent key is `E_INCOMPLETE_PLACE`'s.
#[test]
fn an_absent_use_or_theme_is_not_a_type_mismatch() {
    for row in [
        "place id=a theme=plain at=origin",
        "place id=a use=hut at=origin",
    ] {
        let src = format!("{PLACE_PRELUDE}site s:\n  {row}\n");
        let found = of_code(&src, DiagnosticCode::TypeMismatchLabel);
        assert!(found.is_empty(), "for `{row}`, got {found:#?}");
    }
}
