//! The parser checks each truth-table row against its header's input arity,
//! which only it has in hand. Whole-table findings are `check`'s (in
//! `check/truth_table.rs`), so a table they refuse, the empty one included,
//! still parses here.

use cairn_lang_core::ast::{Item, Statement};
use cairn_lang_core::parse::parse;

/// The patterns the rows of the first `assert truth` carry, each with its
/// output appended after a space.
///
/// Read back rather than asserted through `is_ok()`: `11--> 0` and `11->0`
/// both parse under some arity, and only the pattern says which row the
/// author got.
fn rows_of(source: &str) -> Vec<String> {
    let module = parse(source).expect("parse should succeed");
    let mut out = Vec::new();
    for item in &module.items {
        let Item::Struct { body, .. } = item else {
            continue;
        };
        for statement in body {
            if let Statement::AssertTruth { rows, .. } = statement {
                for row in rows {
                    let output = match row.output {
                        Some(bit) => u8::from(bit).to_string(),
                        None => "-".to_owned(),
                    };
                    out.push(format!("{} {}", row.inputs, output));
                }
            }
        }
    }
    out
}

fn refusal(source: &str) -> String {
    parse(source)
        .expect_err("parse should refuse")
        .user_message()
}

fn body(rows: &str, inputs: &str) -> String {
    format!("struct s size=3x3\n  assert truth({inputs} -> sig.o) {{ {rows} }}\n")
}

/// Keyed on the alphabet rather than on `is_err()`, because a parser that
/// stopped reading the pattern at the `2` would also refuse, for the width.
#[test]
fn an_input_pattern_holds_only_zero_one_and_a_dash() {
    let text = refusal(&body("2->0", "sig.a"));
    assert!(
        text.contains("`2`") && text.contains("`0`, `1` and `-`"),
        "the message should quote the pattern and the characters allowed: {text}",
    );
}

/// A `-` stands for both values of its input, and sits wherever the
/// author puts it.
#[test]
fn a_dont_care_is_read_wherever_it_sits() {
    for (rows, inputs, want) in [
        ("-0->0", "sig.a, sig.b", "-0 0"),
        ("0-->1", "sig.a, sig.b", "0- 1"),
        ("-1-->0", "sig.a, sig.b, sig.c", "-1- 0"),
        ("--->1", "sig.a, sig.b", "-- 1"),
        ("-->0", "sig.a", "- 0"),
    ] {
        assert_eq!(rows_of(&body(rows, inputs)), vec![want.to_owned()]);
    }
}

/// The lexer takes `->` greedily, so a pattern's last `-` and the arrow's
/// first share a character run: a row ending in a don't-care is written
/// with one more `-` than the pattern has, or with a space.
///
/// Both spellings must build the *same* row: a parser that read `11--> 0`
/// as the two-wide `11` would refuse it for the width, which `is_err()`
/// cannot tell from the right verdict.
#[test]
fn a_dash_and_an_arrow_share_a_character() {
    let want = vec!["11- 0".to_owned()];
    assert_eq!(rows_of(&body("11--> 0", "sig.a, sig.b, sig.c")), want);
    assert_eq!(rows_of(&body("11- -> 0", "sig.a, sig.b, sig.c")), want);
}

/// One bit short is the only width a swallowed `-` can produce, because
/// the lexer takes the last dash of a run and no more. A pattern cut short
/// by a space is the same width and a different mistake, and pointing its
/// author at the arrow would send them to the wrong character.
#[test]
fn the_refusal_names_a_dash_the_arrow_swallowed() {
    let swallowed = refusal(&body("11->0", "sig.a, sig.b, sig.c"));
    assert!(
        swallowed.contains("read as part of the arrow") && swallowed.contains("`11--> 0`"),
        "a pattern one bit short of the arity should name the arrow: {swallowed}",
    );

    let spaced = refusal(&body("0- 1->1", "sig.a, sig.b, sig.c"));
    assert!(
        spaced.contains("2 bits wide") && !spaced.contains("read as part of the arrow"),
        "a pattern ended by a space is not a swallowed dash: {spaced}",
    );

    let far_short = refusal(&body("1->1", "sig.a, sig.b, sig.c"));
    assert!(
        !far_short.contains("read as part of the arrow"),
        "two bits short is not a swallowed dash either: {far_short}",
    );
}

/// `0- 1` is a two-wide pattern and a stray `1`, not a three-wide row, and
/// the width check is what says so.
#[test]
fn whitespace_ends_a_pattern() {
    assert!(parse(&body("0- 1->1", "sig.a, sig.b, sig.c")).is_err());
    assert_eq!(
        rows_of(&body("0-1->1", "sig.a, sig.b, sig.c")),
        vec!["0-1 1".to_owned()],
    );
}

/// Three bits for one signal describes no assignment of that signal.
#[test]
fn an_input_pattern_is_as_wide_as_the_input_list() {
    let text = refusal(&body("000->0", "sig.a"));
    // Both numbers and their roles: a message naming 3 and 1 in the wrong
    // places reads as plausibly as the right one.
    assert!(
        text.contains("3 bits wide") && text.contains("1 input"),
        "the message should name the width found and the width required: {text}",
    );
}

#[test]
fn a_pattern_narrower_than_the_input_list_is_refused() {
    assert!(parse(&body("0->0", "sig.a, sig.b")).is_err());
}

#[test]
fn a_well_formed_table_still_parses() {
    for (rows, inputs) in [
        ("0->0; 1->1", "sig.a"),
        ("00->0; 01->1; 10->1; 11->0", "sig.a, sig.b"),
        ("000->0; 111->1", "sig.a, sig.b, sig.c"),
        ("0->1;", "sig.a"),
    ] {
        let source = body(rows, inputs);
        assert!(parse(&source).is_ok(), "{source:?} should parse");
    }
}

/// `01` and `1` are different rows of a two-input table, which is why the
/// parser keeps the lexeme.
#[test]
fn a_leading_zero_is_part_of_the_pattern() {
    assert!(parse(&body("01->1", "sig.a, sig.b")).is_ok());
    assert!(
        parse(&body("01->1", "sig.a")).is_err(),
        "two bits do not describe one signal, whatever their value",
    );
}

/// The tree-sitter grammar's `truth_empty` parity case has the same shape.
/// `check::truth` refuses it, where a finding can carry a span and a repair
/// rather than be the one hard error a parser can raise.
#[test]
fn an_empty_table_still_parses() {
    assert!(parse(&body("", "sig.a")).is_ok());
}

/// No arity of zero reaches `check::truth`, so `2^0 = 1` is not a case it
/// has to word.
#[test]
fn a_table_with_no_inputs_does_not_parse() {
    assert!(parse("struct s size=3x3\n  assert truth( -> sig.o) { }\n").is_err());
}

/// A bare identifier is a degenerate dotted ref, and counts as one input
/// like any other.
#[test]
fn bare_identifiers_count_as_inputs() {
    assert!(parse(&body("00->1", "a, b")).is_ok());
    assert!(parse(&body("0->1", "a, b")).is_err());
}

/// A row's pattern is bounded by the input arity and by nothing else.
///
/// Twenty ones are past `i64` but fit a `u64`; forty ones are past
/// `u128::MAX`, so no integer type could hold them. The all-zero row of each
/// width pins that the verdict does not depend on the value.
#[test]
fn a_row_wider_than_an_i64_is_a_row_like_any_other() {
    for width in [20, 40] {
        let inputs: Vec<String> = (0..width).map(|i| format!("sig.s{i}")).collect();
        let inputs = inputs.join(", ");
        for bit in ['1', '0'] {
            let pattern: String = std::iter::repeat_n(bit, width).collect();
            let source = body(&format!("{pattern}->1"), &inputs);
            assert!(parse(&source).is_ok(), "{source:?} should parse");
        }
    }
}

/// Asserted through the message rather than through `is_err()`, because
/// twenty-one ones is past `i64` too, and an integer-range refusal would
/// also be an error. Both numbers and their roles, as in
/// `an_input_pattern_is_as_wide_as_the_input_list`.
#[test]
fn a_row_past_the_arity_is_refused_by_the_arity_and_not_by_a_ceiling() {
    let inputs: Vec<String> = (0..20).map(|i| format!("sig.s{i}")).collect();
    let text = refusal(&body(&format!("{}->1", "1".repeat(21)), &inputs.join(", ")));
    assert!(
        text.contains("21 bits wide") && text.contains("20 inputs"),
        "the row should be refused for its width against the table, not for its value: {text}",
    );
}

/// A `-` on the output side says the row's combinations are deliberately
/// unconstrained. The arrow is already behind the parser, so `-> -` and
/// `->-` are the same row.
#[test]
fn an_output_may_be_a_dash() {
    for (rows, inputs, want) in [
        ("00 -> -", "sig.a, sig.b", "00 -"),
        ("00->-", "sig.a, sig.b", "00 -"),
        ("-- -> -", "sig.a, sig.b", "-- -"),
        ("0--> -", "sig.a, sig.b", "0- -"),
    ] {
        assert_eq!(rows_of(&body(rows, inputs)), vec![want.to_owned()]);
    }
}

/// Keyed on the message rather than on `is_err()`: a `2` output and a
/// missing output are both refusals, and only one is this test's subject.
#[test]
fn an_output_must_be_zero_one_or_a_dash() {
    let text = refusal(&body("00->2", "sig.a, sig.b"));
    assert!(
        text.contains("`2`") && text.contains("`0`, `1`, or `-`"),
        "the message should quote the output and the characters allowed: {text}",
    );
}

/// A don't-care output is written as `-`, not left out.
#[test]
fn an_output_is_still_required() {
    assert!(!refusal(&body("00->", "sig.a, sig.b")).is_empty());
    assert!(!refusal(&body("00", "sig.a, sig.b")).is_empty());
}
