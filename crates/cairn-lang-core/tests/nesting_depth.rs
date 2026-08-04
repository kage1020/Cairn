//! The parser's two depth bounds, pinned at their exact boundaries.
//!
//! `cairn-lang-cli/tests/cli_nesting_depth.rs` proves the process survives a
//! pathological source, which needs a subprocess to observe. These pin where
//! each wall stands, which that test cannot do cheaply.
//!
//! The two bounds guard different stacks and so are separate numbers:
//!
//! - `MAX_NESTING_DEPTH` bounds how deep the parser itself descends — list
//!   values, parentheses, `not` chains, indented bodies.
//! - `MAX_EXPR_DEPTH` bounds the boolean tree the parser hands back. `or`
//!   and `and` are parsed iteratively, so a flat chain costs the parser
//!   nothing while still building one node of depth per term, and every
//!   consumer of that tree recurses.

use cairn_lang_core::{MAX_EXPR_DEPTH, MAX_NESTING_DEPTH, ParseError, parse};

/// `mat=[[[ … x … ]]]`.
fn nested_list(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  window mat={}x{}\n",
        "[".repeat(depth),
        "]".repeat(depth),
    )
}

/// `logic sig.out = ((( … sig.a … )))`. Parentheses build no AST node, so
/// this costs descent only.
fn nested_parens(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a{}\n",
        "(".repeat(depth),
        ")".repeat(depth),
    )
}

/// `logic sig.out = not not … sig.a`. Costs descent *and* tree depth, so it
/// meets whichever bound is lower.
fn chained_not(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a\n",
        "not ".repeat(depth),
    )
}

/// One indented block per level. `parse_command` and
/// `parse_optional_command_body` are mutually recursive, which is the third
/// recursive production and the one that stayed unguarded longest.
fn nested_blocks(depth: usize) -> String {
    let mut source = String::from("struct a size=1x1\n");
    for level in 1..=depth {
        source.push_str(&"  ".repeat(level));
        source.push_str("level y=0\n");
    }
    source
}

/// Shapes bounded by how deep the parser descends.
fn descent_shapes(depth: usize) -> Vec<(&'static str, String)> {
    vec![
        ("list", nested_list(depth)),
        ("parens", nested_parens(depth)),
        ("not", chained_not(depth)),
        ("blocks", nested_blocks(depth)),
    ]
}

/// `sig.a or sig.a or …`, which the parser walks iteratively.
fn flat_or_chain(terms: usize) -> String {
    let chain = std::iter::repeat_n("sig.a", terms)
        .collect::<Vec<_>>()
        .join(" or ");
    format!("struct a size=1x1\n  logic sig.out = {chain}\n")
}

#[test]
fn descent_at_the_limit_parses() {
    for (shape, source) in descent_shapes(MAX_NESTING_DEPTH) {
        assert!(
            parse(&source).is_ok(),
            "{shape}: {MAX_NESTING_DEPTH} is the deepest legal source, not the first illegal one",
        );
    }
}

#[test]
fn one_level_past_the_descent_limit_is_refused() {
    for (shape, source) in descent_shapes(MAX_NESTING_DEPTH + 1) {
        match parse(&source) {
            Err(ParseError::NestingTooDeep { limit, .. }) => assert_eq!(
                limit, MAX_NESTING_DEPTH,
                "{shape}: the error should carry the bound it hit",
            ),
            other => panic!("{shape}: expected NestingTooDeep, got {other:?}"),
        }
    }
}

#[test]
fn a_flat_chain_at_the_expression_limit_parses() {
    assert!(
        parse(&flat_or_chain(MAX_EXPR_DEPTH)).is_ok(),
        "a chain of {MAX_EXPR_DEPTH} terms builds a tree exactly at the bound",
    );
}

#[test]
fn one_term_past_the_expression_limit_is_refused() {
    // The bound this pins is not the parser's own stack — `or` iterates, so
    // the parser is happy at any length. It is the tree handed back, which
    // `Serialize`, the check passes, and `Box`'s recursive `Drop` all walk.
    // Left unbounded, `cairn parse` overflowed at roughly 570 terms while
    // the parser itself reported no problem at all.
    match parse(&flat_or_chain(MAX_EXPR_DEPTH + 1)) {
        Err(ParseError::NestingTooDeep { limit, .. }) => assert_eq!(
            limit, MAX_EXPR_DEPTH,
            "the expression bound, not the descent one",
        ),
        other => panic!("expected NestingTooDeep, got {other:?}"),
    }
}

/// Terms at which a debug build of `cairn parse` overflowed while
/// serialising the tree, before `MAX_EXPR_DEPTH` existed.
const MEASURED_CONSUMER_OVERFLOW: usize = 570;

// Guards the margin itself, at compile time since both sides are constants.
// Raising `MAX_EXPR_DEPTH` towards the measured overflow would let the
// parser hand back a tree `cairn parse` cannot serialise — the failure
// reappears downstream of the guard, where it is far harder to attribute.
const _: () = assert!(
    MAX_EXPR_DEPTH * 4 <= MEASURED_CONSUMER_OVERFLOW,
    "MAX_EXPR_DEPTH leaves under 4x margin against the term count at which a debug build of \
     `cairn parse` overflowed while serialising the tree",
);

#[test]
fn the_message_tells_the_author_how_to_unwind() {
    // The language is built around an LLM edit loop, so a diagnostic that
    // only says "too deep" without the bound cannot be acted on in one pass.
    for (shape, source) in [
        ("descent", nested_list(MAX_NESTING_DEPTH + 1)),
        ("expression", flat_or_chain(MAX_EXPR_DEPTH + 1)),
    ] {
        let err = parse(&source).expect_err("past the limit");
        let message = err.user_message();
        let limit = match err {
            ParseError::NestingTooDeep { limit, .. } => limit,
            other => panic!("{shape}: expected NestingTooDeep, got {other:?}"),
        };
        assert!(
            message.contains(&limit.to_string()),
            "{shape}: the message must state the bound; got {message}",
        );
    }
}

#[test]
fn the_refusal_points_inside_the_offending_construct() {
    // Not at the opening bracket: the guard fires once the level has been
    // entered, so the position is the first token of the level that went one
    // too far. Pinned because an editor squiggle is only useful where it
    // lands predictably.
    let err = parse(&nested_list(MAX_NESTING_DEPTH + 1)).expect_err("past the limit");
    let position = err.position();
    assert_eq!(position.line.get(), 2, "the offending value is on line 2");
    let first_bracket = u32::try_from("  window mat=".len()).expect("prefix fits") + 1;
    assert!(
        position.col.get() > first_bracket,
        "the position should sit inside the brackets, got col {}",
        position.col,
    );
}

#[test]
fn sibling_values_do_not_accumulate_depth() {
    // Depth is per path, not per node: a list of many shallow items is fine.
    // A counter incremented without a matching decrement would fail here
    // while every nesting test above still passed.
    let items = std::iter::repeat_n("[a]", MAX_NESTING_DEPTH * 4)
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("struct a size=1x1\n  window mat=[{items}]\n");
    assert!(
        parse(&source).is_ok(),
        "siblings sit at the same depth, however many there are",
    );
}

#[test]
fn sibling_blocks_do_not_accumulate_depth() {
    // The same property for the production guarded last: many commands in
    // one body are siblings, not nesting.
    let mut source = String::from("struct a size=1x1\n");
    for _ in 0..MAX_NESTING_DEPTH * 4 {
        source.push_str("  level y=0\n");
    }
    assert!(parse(&source).is_ok(), "a wide body is not a deep one");
}
