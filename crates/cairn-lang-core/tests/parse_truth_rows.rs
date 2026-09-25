//! Truth-table rows are checked against the header they sit under.
//!
//! `assert truth` exists to verify a circuit, so a row the evaluator
//! cannot read is worse than no row at all: it looks like coverage. The
//! parser is where *this* check belongs, because the input arity — the
//! number of signals left of the arrow — is in hand there and nowhere
//! downstream keeps it beside the rows.
//!
//! What one row cannot see is the table around it. No rows at all, two
//! rows assigning one combination, combinations left out: those are
//! `check::truth`, reported as diagnostics rather than as parse errors,
//! and `check_truth_table.rs` covers them. The split is the reason a
//! shape refused there still parses here — including the empty table, so
//! `an_empty_table_still_parses` is a statement about which layer owns
//! the refusal and not about the table being acceptable.

use cairn_lang_core::ast::{Item, Statement};
use cairn_lang_core::parse::parse;

/// The patterns the rows of the first `assert truth` carry, each with its
/// output appended after a space.
///
/// Read back rather than asserted through `is_ok()`, because the
/// don't-care cases are about *what* the parser built: `11--> 0` and
/// `11->0` both parse under some arity, and only the pattern says which
/// row the author got.
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
                    out.push(format!("{} {}", row.inputs, u8::from(row.output)));
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

/// The output side was already checked against `0` / `1`; the input side
/// kept whatever integer lexeme it was handed.
///
/// `-` joined the alphabet with the don't-care, so the message names
/// three characters. Keyed on the alphabet rather than on `is_err()`,
/// because a parser that stopped reading the pattern at the `2` would
/// also refuse — for the width, which is a different bug wearing the
/// same verdict.
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
/// Both spellings are asserted to build the *same* row rather than merely
/// to parse. A parser that read `11--> 0` as the two-wide `11` would
/// refuse it here for the width — the right verdict for the wrong
/// reason, and one `is_err()` cannot tell from the other.
#[test]
fn a_dash_and_an_arrow_share_a_character() {
    let want = vec!["11- 0".to_owned()];
    assert_eq!(rows_of(&body("11--> 0", "sig.a, sig.b, sig.c")), want);
    assert_eq!(rows_of(&body("11- -> 0", "sig.a, sig.b, sig.c")), want);
}

/// And the mistake that shape invites gets its own sentence.
///
/// One bit short is the only width a swallowed `-` can produce, because
/// the lexer takes the last dash of a run and no more. So the sentence is
/// offered there and nowhere else: a pattern cut short by a space is the
/// same width and a different mistake, and pointing its author at the
/// arrow would send them to the wrong character.
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

/// A pattern is the characters the source ran together, so whitespace
/// ends it.
///
/// `0- 1` is a two-wide pattern and a stray `1`, not a three-wide row,
/// and the width check is what says so. Without this the parser could
/// read a pattern across a line and never notice.
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

/// And the other direction: one bit does not cover two signals.
#[test]
fn a_pattern_narrower_than_the_input_list_is_refused() {
    assert!(parse(&body("0->0", "sig.a, sig.b")).is_err());
}

/// The shapes that describe a real assignment still parse.
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

/// A leading zero is data here, not a numeric quirk: `01` and `1` are
/// different rows of a two-input table, and the parser keeps the lexeme
/// for exactly that reason.
#[test]
fn a_leading_zero_is_part_of_the_pattern() {
    assert!(parse(&body("01->1", "sig.a, sig.b")).is_ok());
    assert!(
        parse(&body("01->1", "sig.a")).is_err(),
        "two bits do not describe one signal, whatever their value",
    );
}

/// An empty table parses — the reference parser's row loop accepts zero
/// rows, and the tree-sitter grammar has a `truth_empty` corpus case for
/// the same shape. It is refused a layer later, by `check::truth`, which
/// is where a finding can be a warning-or-error diagnostic with a span
/// and a repair rather than the one hard error a parser can raise.
#[test]
fn an_empty_table_still_parses() {
    assert!(parse(&body("", "sig.a")).is_ok());
}

/// And the table the pass never has to reason about: the input list is
/// read before the arrow with no way to be empty, so no arity of zero
/// reaches `check::truth` and `2^0 = 1` is not a case it has to word.
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
/// Twenty ones used to be an overflow: `scan_number` parsed every digit
/// run into an `i64`, so the table failed during lexing with a message
/// about integer range and no row ever reached the parser. The refusal
/// was value-dependent, which the all-zero row records — same width,
/// same table, and it parsed.
///
/// Forty ones is the assertion that the ceiling is *gone* rather than
/// raised: it is past `u128::MAX`, so no integer type this crate could
/// reach for would accept it. Twenty alone would not say that much —
/// `11111111111111111111` still fits in a `u64`.
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

/// And the rule the ceiling was masking still applies at that width.
///
/// Asserted through the message rather than through `is_err()`: twenty-one
/// ones is past `i64` too, so a bare `is_err()` was already true before
/// the ceiling came off and said nothing about arity. Both numbers and
/// their roles, as in `an_input_pattern_is_as_wide_as_the_input_list` —
/// a message that swapped them would read just as plausibly.
#[test]
fn a_row_past_the_arity_is_refused_by_the_arity_and_not_by_a_ceiling() {
    let inputs: Vec<String> = (0..20).map(|i| format!("sig.s{i}")).collect();
    let text = refusal(&body(&format!("{}->1", "1".repeat(21)), &inputs.join(", ")));
    assert!(
        text.contains("21 bits wide") && text.contains("20 inputs"),
        "the row should be refused for its width against the table, not for its value: {text}",
    );
}
