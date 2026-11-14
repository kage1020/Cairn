//! A decimal, `scale=1.5`, is a value as an integer is, kept as written (`spec/syntax` "Literals
//! and separators").

use cairn_lang_core::ast::{Header, Item, Statement, ValueKind};
use cairn_lang_core::parse;

use crate::{member_args, member_refusal};

fn decimal(text: &str) -> ValueKind {
    ValueKind::Decimal(text.to_owned())
}

#[test]
fn digits_a_dot_and_digits_are_one_decimal_kept_as_written() {
    assert_eq!(
        member_args("walls scale=1.5 tilt=0.25 pad=007.50"),
        [
            ("scale".to_owned(), decimal("1.5")),
            ("tilt".to_owned(), decimal("0.25")),
            ("pad".to_owned(), decimal("007.50")),
        ],
    );
}

#[test]
fn a_decimal_goes_where_an_integer_goes() {
    let args = member_args("walls marks=[1.5, 2, 0.5]");
    let [(key, ValueKind::List(items))] = args.as_slice() else {
        panic!("expected one list argument");
    };
    assert_eq!(key, "marks");
    let kinds: Vec<&ValueKind> = items.iter().map(|item| &item.kind).collect();
    assert_eq!(
        kinds,
        [&decimal("1.5"), &ValueKind::Int(2), &decimal("0.5")]
    );
    assert_eq!(
        member_args("walls height=3 1.5"),
        [("height".to_owned(), ValueKind::Int(3))],
    );
    let module = parse("struct s size=3x3\n  walls height=3 1.5\n").expect("parses");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected a struct");
    };
    let Statement::Generic { positional, .. } = &body[0] else {
        panic!("expected a member line");
    };
    let bare: Vec<&ValueKind> = positional.iter().map(|value| &value.kind).collect();
    assert_eq!(bare, [&decimal("1.5")]);
}

#[test]
fn a_literal_that_is_not_digits_a_dot_and_digits_is_refused_at_the_literal() {
    for (member, message, col) in [
        (
            "walls scale=1.5e3",
            "decimal `1.5` is followed by `e`; a decimal is digits, a `.` and digits, with no \
             exponent or suffix, as in `1.5`",
            15,
        ),
        (
            "walls scale=1.5f",
            "decimal `1.5` is followed by `f`; a decimal is digits, a `.` and digits, with no \
             exponent or suffix, as in `1.5`",
            15,
        ),
        (
            "walls scale=1.5.2",
            "decimal `1.5` is followed by `.`; a decimal is digits, a `.` and digits, with no \
             exponent or suffix, as in `1.5`",
            15,
        ),
        (
            "walls scale=1.",
            "decimal `1.` has no digit after its `.`; write `1.0`, or `1` for a whole number",
            15,
        ),
        (
            "walls scale=.5",
            "decimal `.5` has no digit before its `.`; write `0.5`",
            15,
        ),
        (
            "walls scale=[1, .5]",
            "decimal `.5` has no digit before its `.`; write `0.5`",
            19,
        ),
        ("walls scale=-1.5", "unexpected `-` in value position", 15),
        (
            "walls size=9x7.5",
            "size literal `9x7` is followed by `.`; a size is two extents, as in `9x7`",
            14,
        ),
    ] {
        assert_eq!(
            member_refusal(member),
            (message.to_owned(), col),
            "{member:?}"
        );
    }
}

/// A `.` after a name, a `]` or a `)` is that thing's own, so `sig.1` is still a bad name.
#[test]
fn a_dot_after_a_name_opens_no_decimal() {
    assert_eq!(
        member_refusal("lamp powered_by=sig.1"),
        ("expected identifier, got integer `1`".to_owned(), 23),
    );
}

#[test]
fn a_dotted_version_in_a_raw_directive_value_is_text() {
    let module = parse("@cairn 2026.06\n@requires version>=1.21.4\nstruct s size=3x3\n")
        .expect("a raw value takes any text");
    let values: Vec<String> = module
        .headers
        .iter()
        .map(|header| match header {
            Header::Cairn { version, .. } => version.as_str().to_owned(),
            Header::Requires { requirement, .. } => requirement.as_str().to_owned(),
            other => panic!("unexpected header {other:?}"),
        })
        .collect();
    assert_eq!(values, ["2026.06", "version>=1.21.4"]);
}
