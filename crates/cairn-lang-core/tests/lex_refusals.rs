//! What the lexer refuses, and what it says when it does.
//!
//! Every case here is text a lexer *could* quietly reshape into something
//! it can represent — a size literal with a third extent, an indent two
//! levels deep, a byte-order mark. The tests assert that it does not, and
//! that the message names the thing that would fix the file: the audience
//! for these diagnostics is a generator reading them back, so a message
//! whose instruction is already satisfied ("must be a multiple of 2" for
//! a 4-space indent) is a defect, not a wording nit.

use cairn_lang_core::error::LexError;
use cairn_lang_core::lex::{TokenKind, lex};

fn kinds(source: &str) -> Vec<TokenKind> {
    lex(source)
        .expect("lex")
        .into_iter()
        .map(|t| t.kind)
        .collect()
}

fn message(source: &str) -> String {
    lex(source).expect_err("lex should refuse").user_message()
}

// -- size literals --------------------------------------------------------

/// `2x2x9` used to lex as `Size(2,2)` followed by `Ident("x9")`, and the
/// stray identifier landed in the parser's positional list where nothing
/// reads it — so the build came out `2x2` with no diagnostic anywhere.
#[test]
fn a_size_literal_with_a_third_extent_is_refused() {
    let err = lex("struct s size=2x2x9\n").unwrap_err();
    assert!(
        matches!(&err, LexError::TrailingSizeSegment { found, .. } if *found == 'x'),
        "{err:?}",
    );
}

/// The same defect wearing a different character: any identifier byte
/// continues the run the author was writing, so the literal they meant is
/// not the one the token would hold.
#[test]
fn a_size_literal_followed_by_a_word_character_is_refused() {
    for source in [
        "struct s size=2x2y\n",
        "struct s size=2x2_\n",
        "struct s size=2x2x\n",
    ] {
        assert!(
            lex(source).is_err(),
            "{source:?} should be refused, not truncated",
        );
    }
}

/// The refusal is a variant of its own, so a quick-fix can dispatch on it
/// without reading prose — and so it is not mistaken for the neighbouring
/// conditions, which is the failure this issue's indent item is about.
#[test]
fn the_trailing_segment_refusal_names_the_character_it_found() {
    let text = message("struct s size=2x2x9\n");
    assert!(
        text.contains("2x2") && text.contains('x'),
        "the message should quote the literal and the character after it: {text}",
    );
}

/// A delimiter ends the literal rather than continuing it, so the ordinary
/// forms keep working.
#[test]
fn a_size_literal_ends_at_a_delimiter() {
    for source in [
        "struct s size=9x7\n",
        "struct s size=9x7 y=1\n",
        "struct s size=9x7:\n",
        "site p:\n  place at=[9x7]\n",
        "site p:\n  place at=[9x7,1]\n",
    ] {
        assert!(lex(source).is_ok(), "{source:?} should still lex");
    }
}

/// Leading zeros in an extent are not a second literal, and were never the
/// thing being refused.
#[test]
fn leading_zeros_in_an_extent_still_lex() {
    assert_eq!(kinds("9x0007\n")[0], TokenKind::Size(9, 7));
}

/// `9x` has no height digits, so `scan_number` never builds a `Size` at
/// all — it is `Int` then `Ident`, and the parser is what refuses it. The
/// fix must not move that refusal into the lexer, because `9` followed by
/// an identifier is legal in other positions.
#[test]
fn a_size_literal_with_no_height_is_still_two_tokens() {
    assert_eq!(
        kinds("9x\n"),
        vec![
            TokenKind::Int {
                value: 9,
                lexeme: "9".into()
            },
            TokenKind::Ident("x".into()),
            TokenKind::Newline,
        ],
    );
}

// -- indentation ----------------------------------------------------------

/// A 4-space indent is a multiple of 2, so "indentation must be a multiple
/// of 2 spaces (got 4)" tells the author to do what they already did. The
/// two conditions get separate variants for that reason.
#[test]
fn a_multi_level_jump_is_not_reported_as_odd_indentation() {
    let err = lex("struct keep size=3x3\n    floor mat_slot=wall\n").unwrap_err();
    assert!(
        matches!(
            err,
            LexError::IndentJump {
                got: 4,
                expected: 2,
                ..
            }
        ),
        "{err:?}",
    );
}

/// The message has to name the width that would work, since naming the
/// rule alone is what made the old one unactionable.
#[test]
fn the_jump_refusal_names_the_width_that_would_work() {
    let text = message("struct keep size=3x3\n    floor mat_slot=wall\n");
    assert!(
        text.contains('2') && text.contains('4'),
        "the message should name both the width found and the one expected: {text}",
    );
}

/// The expected width is relative to the level currently open, not a
/// constant — jumping to 6 is a different repair from inside a body than
/// from the top level.
#[test]
fn the_expected_width_follows_the_enclosing_level() {
    let from_top = lex("struct keep size=3x3\n      floor mat_slot=wall\n").unwrap_err();
    assert!(
        matches!(
            from_top,
            LexError::IndentJump {
                got: 6,
                expected: 2,
                ..
            }
        ),
        "{from_top:?}",
    );

    let from_one_level =
        lex("struct keep size=3x3\n  level y=0\n      floor mat_slot=wall\n").unwrap_err();
    assert!(
        matches!(
            from_one_level,
            LexError::IndentJump {
                got: 6,
                expected: 4,
                ..
            }
        ),
        "{from_one_level:?}",
    );
}

/// Odd indentation keeps its own variant and its own message: that one was
/// always actionable.
#[test]
fn odd_indentation_is_still_its_own_refusal() {
    let err = lex("theme medieval:\n   slot floor -> @x\n").unwrap_err();
    assert!(matches!(err, LexError::OddIndent { got: 3, .. }), "{err:?}");
}

/// An odd width that is *also* more than one level deep reports the odd
/// count, because rounding to a legal width is the first repair and the
/// level is only meaningful once the width is.
#[test]
fn an_odd_jump_reports_the_odd_width() {
    let err = lex("struct keep size=3x3\n     floor mat_slot=wall\n").unwrap_err();
    assert!(matches!(err, LexError::OddIndent { got: 5, .. }), "{err:?}");
}

// -- byte-order marks and unprintable characters --------------------------

/// U+FEFF is what a default Windows editor writes, and it used to fail as
/// an unexpected character quoted verbatim — which renders as nothing, so
/// the message named an empty string.
#[test]
fn a_leading_byte_order_mark_is_skipped() {
    let with_bom = lex("\u{feff}struct s size=3x3\n").expect("a leading BOM should lex");
    let without = lex("struct s size=3x3\n").expect("lex");
    let strip = |tokens: Vec<cairn_lang_core::lex::Token>| {
        tokens.into_iter().map(|t| t.kind).collect::<Vec<_>>()
    };
    assert_eq!(strip(with_bom), strip(without));
}

/// Only at offset 0. A BOM in the middle of a file is a real stray
/// character and stays one — skipping it there would hide the same class
/// of bug this issue is about.
#[test]
fn a_byte_order_mark_after_the_start_is_still_refused() {
    let err = lex("struct s\u{feff} size=3x3\n").unwrap_err();
    assert!(matches!(err, LexError::UnexpectedChar { .. }), "{err:?}");
}

/// U+00A0 renders as a space, so quoting it verbatim produced a message
/// saying a space was unexpected. Unprintable and space-confusable
/// characters are named by codepoint instead.
#[test]
fn an_invisible_character_is_named_by_codepoint() {
    let text = message("struct s\u{a0}size=3x3\n");
    assert!(
        text.to_ascii_uppercase().contains("A0"),
        "the message should carry the codepoint: {text}",
    );
}

/// A printable character still shows itself — a codepoint would be worse
/// for the common case.
#[test]
fn a_printable_unexpected_character_shows_itself() {
    let text = message("struct s size=3x3\n  floor ?=1\n");
    assert!(text.contains('?'), "{text}");
}
