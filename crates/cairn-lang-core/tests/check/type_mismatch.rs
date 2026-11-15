//! `E_TYPE_MISMATCH_LABEL` and `E_TYPE_MISMATCH_SIZE`: a label or size key given a value of
//! another shape is an error, and an absent key is not a type mismatch.

use cairn_lang_core::{Diagnostic, DiagnosticCode};

use crate::common::{diagnose, notes, slice};

use DiagnosticCode::{TypeMismatchLabel, TypeMismatchSize};

/// Each finding `check` raises on `src`, as its code and the text it points at, in report order.
fn found(src: &str) -> Vec<(DiagnosticCode, &str)> {
    diagnose(src)
        .iter()
        .map(|d| (d.code, slice(src, d)))
        .collect()
}

/// The one finding `check` raises on `src`, which the caller has seen is the only one.
fn only(src: &str) -> Diagnostic {
    let mut diags = diagnose(src);
    assert_eq!(diags.len(), 1, "for {src:?}, got {diags:#?}");
    diags.remove(0)
}

#[test]
fn id_set_to_token_is_flagged_as_label_mismatch() {
    let src = "struct s size=1x1\n  walls id=@oak mat_slot=m\n";
    assert_eq!(found(src), [(TypeMismatchLabel, "@oak")]);
    let diag = only(src);
    assert_eq!(
        diag.primary,
        "`id=` expects a label (identifier or string), got token"
    );
    assert_eq!(
        notes(&diag),
        ["write a name, such as `id=main`, or a quoted string"]
    );
}

#[test]
fn a_mat_slot_given_a_block_says_to_bind_it_in_the_theme() {
    for (src, written, note) in [
        (
            "struct s size=1x1\n  walls mat_slot=@oak_planks\n",
            "@oak_planks",
            "`mat_slot=` names a slot of the bound theme, not a block: bind the block there \
             (`slot NAME -> @oak_planks`) and write `mat_slot=NAME`",
        ),
        (
            "struct s size=1x1\n  walls mat_slot=3\n",
            "3",
            "write the name of a slot the bound theme declares",
        ),
        (
            "struct s size=1x1\n  walls mat_slot=minecraft:oak_log[axis=x]\n",
            "minecraft:oak_log[axis=x]",
            "`mat_slot=` names a slot of the bound theme, not a block: bind its canonical token \
             there (`slot NAME -> @oak_log[axis=x]`) and write `mat_slot=NAME`, or place \
             `minecraft:oak_log[axis=x]` as written with a `raw_block` or `raw_fill`",
        ),
        (
            "struct s size=1x1\n  walls mat_slot=my_mod:copper_lamp\n",
            "my_mod:copper_lamp",
            "`mat_slot=` names a slot of the bound theme, not a block: bind a canonical token \
             there and write `mat_slot=NAME`, or place `my_mod:copper_lamp` as written with a \
             `raw_block` or `raw_fill`",
        ),
    ] {
        assert_eq!(found(src), [(TypeMismatchLabel, written)], "for {src:?}");
        assert_eq!(notes(&only(src)), [note], "for {src:?}");
    }
}

#[test]
fn a_coordinate_on_a_label_or_size_key_is_a_type_mismatch() {
    for (src, code, primary) in [
        (
            "struct s size=1x1\n  walls mat_slot=1,2,3\n",
            TypeMismatchLabel,
            "`mat_slot=` expects a label (identifier or string), got coordinate",
        ),
        (
            "struct s size=1,2,3\n  floor mat_slot=m\n",
            TypeMismatchSize,
            "`size=` expects a `WxH` literal, got coordinate",
        ),
    ] {
        assert_eq!(found(src), [(code, "1,2,3")], "for {src:?}");
        assert_eq!(only(src).primary, primary, "for {src:?}");
    }
}

#[test]
fn a_decimal_or_a_compound_on_a_label_or_size_key_is_a_type_mismatch() {
    let label = "`mat_slot=` expects a label (identifier or string), got";
    let size = "`size=` expects a `WxH` literal, got";
    for (src, code, written, primary) in [
        (
            "struct s size=1x1\n  walls mat_slot=1.5\n",
            TypeMismatchLabel,
            "1.5",
            format!("{label} decimal"),
        ),
        (
            "struct s size=1x1\n  walls mat_slot={a:1}\n",
            TypeMismatchLabel,
            "{a:1}",
            format!("{label} NBT compound"),
        ),
        (
            "struct s size=1.5\n  floor mat_slot=m\n",
            TypeMismatchSize,
            "1.5",
            format!("{size} decimal"),
        ),
        (
            "struct s size={a:1}\n  floor mat_slot=m\n",
            TypeMismatchSize,
            "{a:1}",
            format!("{size} NBT compound"),
        ),
    ] {
        assert_eq!(found(src), [(code, written)], "for {src:?}");
        assert_eq!(only(src).primary, primary, "for {src:?}");
    }
}

#[test]
fn a_block_id_on_a_size_key_is_a_type_mismatch() {
    let src = "struct s size=minecraft:stone\n  floor mat_slot=m\n";
    assert_eq!(found(src), [(TypeMismatchSize, "minecraft:stone")]);
    assert_eq!(
        only(src).primary,
        "`size=` expects a `WxH` literal, got block id"
    );
}

#[test]
fn class_set_to_dotted_ref_is_flagged() {
    let src = "struct s size=1x1\n  walls class=foo.bar mat_slot=m\n";
    assert_eq!(found(src), [(TypeMismatchLabel, "foo.bar")]);
}

#[test]
fn mat_slot_string_value_is_accepted() {
    assert_eq!(found("struct s size=1x1\n  walls mat_slot=\"wall\"\n"), []);
}

#[test]
fn struct_size_set_to_integer_is_flagged() {
    let src = "struct s size=5\n  floor mat_slot=m\n";
    assert_eq!(found(src), [(TypeMismatchSize, "5")]);
    let diag = only(src);
    assert_eq!(diag.primary, "`size=` expects a `WxH` literal, got integer");
    assert_eq!(
        notes(&diag),
        ["write two whole numbers joined by `x`, such as `size=9x7`"]
    );
}

#[test]
fn window_size_with_proper_literal_passes() {
    assert_eq!(
        found("struct s size=4x4\n  window size=2x2 side=front\n"),
        []
    );
}

#[test]
fn a_chests_size_is_its_state_and_not_a_footprint() {
    let src = "struct s size=5x5\n  chest at=inside.front offset=1 size=single\n  \
               window side=front offset=2 y=1 size=single\n";
    let at: Vec<(DiagnosticCode, usize)> = diagnose(src)
        .iter()
        .map(|d| (d.code, d.span.start))
        .collect();
    assert_eq!(
        at,
        [(
            TypeMismatchSize,
            src.rfind("single").expect("the window's size")
        )]
    );
}

#[test]
fn duplicate_size_and_size_mismatch_pass_independently() {
    assert_eq!(
        found("struct s size=2x2 size=foo\n  floor\n"),
        [
            (DiagnosticCode::DuplicateSize, "size=foo"),
            (TypeMismatchSize, "foo"),
            (DiagnosticCode::MissingMaterial, "floor"),
        ],
    );
}

/// A theme and a def a `place` row can name, so only the key under test is wrong.
const PLACE_PRELUDE: &str = "theme plain:\n  \
slot floor -> @oak_planks\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n\n";

/// The def `hut` as [`PLACE_PRELUDE`] writes it, which a row that names no `use=hut` leaves unused.
const HUT: &str = "def hut size=3x3:\n  floor id=floor mat_slot=floor";

/// The resolver drops a placement whose value is not a label without a word.
#[test]
fn place_use_and_theme_with_a_non_label_value_are_flagged() {
    let def_note = "write the name of a `def` this file declares";
    let theme_note = "write the name of a `theme` this file declares";
    for (row, expected, note) in [
        (
            "place id=a use=3 theme=plain at=origin",
            vec![(DiagnosticCode::UnusedDef, HUT), (TypeMismatchLabel, "3")],
            def_note,
        ),
        (
            "place id=a use=hut theme=7 at=origin",
            vec![(TypeMismatchLabel, "7")],
            theme_note,
        ),
        (
            "place id=a use=@oak theme=plain at=origin",
            vec![
                (DiagnosticCode::UnusedDef, HUT),
                (TypeMismatchLabel, "@oak"),
            ],
            def_note,
        ),
        (
            "place id=a use=hut theme=x.y at=origin",
            vec![(TypeMismatchLabel, "x.y")],
            theme_note,
        ),
    ] {
        let src = format!("{PLACE_PRELUDE}site s:\n  {row}\n");
        assert_eq!(found(&src), expected, "for `{row}`");
        let mismatch = diagnose(&src)
            .into_iter()
            .find(|d| d.code == TypeMismatchLabel)
            .expect("the mismatch");
        assert_eq!(notes(&mismatch), [note], "for `{row}`");
    }
}

#[test]
fn place_use_and_theme_accept_identifiers_and_strings() {
    for row in [
        "place id=a use=hut theme=plain at=origin",
        "place id=a use=\"hut\" theme=\"plain\" at=origin",
    ] {
        let src = format!("{PLACE_PRELUDE}site s:\n  {row}\n");
        assert_eq!(found(&src), [], "for `{row}`");
    }
}

/// An absent key is `E_INCOMPLETE_PLACE`'s.
#[test]
fn an_absent_use_or_theme_is_not_a_type_mismatch() {
    for (row, expected) in [
        (
            "place id=a theme=plain at=origin",
            vec![
                (DiagnosticCode::UnusedDef, HUT),
                (
                    DiagnosticCode::IncompletePlace,
                    "place id=a theme=plain at=origin",
                ),
            ],
        ),
        (
            "place id=a use=hut at=origin",
            vec![(
                DiagnosticCode::IncompletePlace,
                "place id=a use=hut at=origin",
            )],
        ),
    ] {
        let src = format!("{PLACE_PRELUDE}site s:\n  {row}\n");
        assert_eq!(found(&src), expected, "for `{row}`");
    }
}
