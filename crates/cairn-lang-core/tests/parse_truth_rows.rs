//! Truth-table rows are checked against the header they sit under.
//!
//! `assert truth` exists to verify a circuit, so a row the evaluator
//! cannot read is worse than no row at all: it looks like coverage. The
//! parser is where *this* check belongs, because the input arity — the
//! number of signals left of the arrow — is in hand there and nowhere
//! downstream keeps it beside the rows.
//!
//! What one row cannot see is the table around it. No rows at all, a
//! pattern assigned twice, combinations left out: those are
//! `check::truth`, reported as diagnostics rather than as parse errors,
//! and `check_truth_table.rs` covers them. The split is the reason a
//! shape refused there still parses here — including the empty table, so
//! `an_empty_table_still_parses` is a statement about which layer owns
//! the refusal and not about the table being acceptable.

use cairn_lang_core::parse::parse;

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
#[test]
fn an_input_pattern_holds_only_zero_and_one() {
    let text = refusal(&body("2->0", "sig.a"));
    assert!(
        text.contains("`2`") && text.contains("`0` and `1`"),
        "the message should quote the pattern and the digits allowed: {text}",
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
