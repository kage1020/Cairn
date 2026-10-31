//! The parser's two depth bounds hold at their exact boundaries: `MAX_NESTING_DEPTH` on descent,
//! and `MAX_EXPR_DEPTH` on the boolean tree, which an infix chain deepens without descent.

use cairn_lang_core::{MAX_EXPR_DEPTH, MAX_NESTING_DEPTH, ParseError, parse};

/// `mat=[[[ … x … ]]]`.
fn nested_list(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  window mat={}x{}\n",
        "[".repeat(depth),
        "]".repeat(depth),
    )
}

/// `logic sig.out = ((( … sig.a … )))`, which costs descent only.
fn nested_parens(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a{}\n",
        "(".repeat(depth),
        ")".repeat(depth),
    )
}

/// `logic sig.out = not not … sig.a`, which costs descent and tree depth.
fn chained_not(depth: usize) -> String {
    format!(
        "struct a size=1x1\n  logic sig.out = {}sig.a\n",
        "not ".repeat(depth),
    )
}

/// One indented block per level, the third recursive production.
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

/// `terms` distinct signals joined by `primitive`, which the parser walks iteratively.
fn chain(primitive: &str, terms: usize) -> String {
    (0..terms)
        .map(|i| format!("sig.a{i}"))
        .collect::<Vec<_>>()
        .join(&format!(" {primitive} "))
}

/// `logic sig.out = {expression}`.
fn logic(expression: &str) -> String {
    format!("struct a size=1x1\n  logic sig.out = {expression}\n")
}

/// `sig.a0 or sig.a1 or …`, `terms` deep.
fn flat_or_chain(terms: usize) -> String {
    logic(&chain("or", terms))
}

/// The `not`s around the chain in [`expression_shapes`], short of the descent bound.
const NOTS: usize = 60;

/// Shapes bounded by how deep a tree the parser hands back, each exactly `depth` deep.
fn expression_shapes(depth: usize) -> Vec<(&'static str, String)> {
    vec![
        ("or chain", flat_or_chain(depth)),
        ("and chain", logic(&chain("and", depth))),
        (
            "nots around an or chain",
            logic(&format!(
                "{}({})",
                "not ".repeat(NOTS),
                chain("or", depth - NOTS)
            )),
        ),
        (
            "and chain right of an or",
            logic(&format!("sig.z or {}", chain("and", depth - 1))),
        ),
        (
            "parenthesised or chain right of an or",
            logic(&format!("sig.z or ({})", chain("or", depth - 1))),
        ),
        (
            "parenthesised or chain",
            logic(&format!("({})", chain("or", depth))),
        ),
    ]
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
fn a_tree_at_the_expression_limit_parses() {
    for (shape, source) in expression_shapes(MAX_EXPR_DEPTH) {
        assert!(
            parse(&source).is_ok(),
            "{shape}: a tree {MAX_EXPR_DEPTH} deep is the deepest legal one",
        );
    }
}

#[test]
fn a_tree_one_past_the_expression_limit_is_refused() {
    // The bound is on the tree handed back, which `Serialize`, the check passes and `Drop` walk.
    for (shape, source) in expression_shapes(MAX_EXPR_DEPTH + 1) {
        match parse(&source) {
            Err(ParseError::NestingTooDeep { limit, .. }) => assert_eq!(
                limit, MAX_EXPR_DEPTH,
                "{shape}: the expression bound, not the descent one",
            ),
            other => panic!("{shape}: expected NestingTooDeep, got {other:?}"),
        }
    }
}

/// Terms at which a debug build of `cairn parse` overflows serialising an unbounded tree, as
/// measured on Linux with a 1 MiB stack, the size Windows gives the main thread.
const MEASURED_CONSUMER_OVERFLOW: usize = 751;

// A `MAX_EXPR_DEPTH` near the measured overflow would fail downstream, far from this guard.
const _: () = assert!(
    MAX_EXPR_DEPTH * 4 <= MEASURED_CONSUMER_OVERFLOW,
    "MAX_EXPR_DEPTH leaves under 4x margin against the term count at which a debug build of \
     `cairn parse` overflowed while serialising the tree",
);

#[test]
fn the_message_tells_the_author_how_to_unwind() {
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
    // A counter incremented without a matching decrement fails here and nowhere else.
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
    let mut source = String::from("struct a size=1x1\n");
    for _ in 0..MAX_NESTING_DEPTH * 4 {
        source.push_str("  level y=0\n");
    }
    assert!(parse(&source).is_ok(), "a wide body is not a deep one");
}
