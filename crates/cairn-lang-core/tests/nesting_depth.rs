//! The parser's nesting bound, pinned at the exact boundary.
//!
//! `cairn-lang-cli/tests/cli_nesting_depth.rs` proves the process survives a
//! pathological source, which needs a subprocess to observe. These pin where
//! the wall actually stands, which that test cannot do cheaply.

use cairn_lang_core::{ParseError, parse};

/// Depth the parser accepts. Mirrors `parse::MAX_NESTING_DEPTH`, which is
/// private; the assertions below are what keep the two in step.
const LIMIT: usize = 64;

fn nested_list(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  window mat={}x{}\n",
        "[".repeat(depth),
        "]".repeat(depth),
    )
}

fn nested_parens(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a{}\n",
        "(".repeat(depth),
        ")".repeat(depth),
    )
}

fn chained_not(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a\n",
        "not ".repeat(depth),
    )
}

fn shapes(depth: usize) -> Vec<(&'static str, String)> {
    vec![
        ("list", nested_list(depth)),
        ("parens", nested_parens(depth)),
        ("not", chained_not(depth)),
    ]
}

#[test]
fn nesting_at_the_limit_parses() {
    for (shape, source) in shapes(LIMIT) {
        assert!(
            parse(&source).is_ok(),
            "{shape}: depth {LIMIT} is the deepest legal source, not the first illegal one",
        );
    }
}

#[test]
fn one_level_past_the_limit_is_refused() {
    for (shape, source) in shapes(LIMIT + 1) {
        match parse(&source) {
            Err(ParseError::NestingTooDeep { limit, .. }) => {
                assert_eq!(
                    limit, LIMIT,
                    "{shape}: the error should carry the real limit"
                );
            }
            other => panic!(
                "{shape}: expected NestingTooDeep at depth {}, got {other:?}",
                LIMIT + 1
            ),
        }
    }
}

#[test]
fn the_message_tells_the_author_how_to_unwind() {
    // The language is built around an LLM edit loop, so a diagnostic that
    // only says "too deep" without the bound cannot be acted on in one pass.
    let source = nested_list(LIMIT + 1);
    let err = parse(&source).expect_err("depth past the limit");
    let message = err.user_message();
    assert!(
        message.contains(&LIMIT.to_string()),
        "the message must state the limit; got {message}",
    );
}

#[test]
fn a_flat_boolean_chain_costs_no_depth() {
    // `or` and `and` iterate rather than recurse, so length is free. Pinning
    // this stops a future refactor from making the two associative operators
    // recursive and quietly capping expression *length* at the nesting bound.
    let chain = std::iter::repeat_n("sig.a", LIMIT * 8)
        .collect::<Vec<_>>()
        .join(" or ");
    let source = format!("struct a size=1x1\n  logic sig.out = {chain}\n");
    assert!(parse(&source).is_ok(), "a flat `or` chain is not nesting");
}

#[test]
fn sibling_values_do_not_accumulate_depth() {
    // Depth is per path, not per node: a list of many shallow items is fine.
    // A counter incremented without a matching decrement would fail here
    // while every nesting test above still passed.
    let items = std::iter::repeat_n("[a]", LIMIT * 4)
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("struct a size=1x1\n  window mat=[{items}]\n");
    assert!(
        parse(&source).is_ok(),
        "siblings sit at the same depth, however many there are",
    );
}
