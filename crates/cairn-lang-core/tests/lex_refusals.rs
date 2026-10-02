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

/// `2x2x9` used to lex as `Size(2,2)` followed by `Ident("x9")`. The
/// stray identifier was reported — by `check::positional`, as a bare
/// value, at a column past the literal that produced it — or, in a
/// declaration header, not by that pass at all, since a header keeps no
/// positional list and the parser simply ran to the end of the line.
/// Neither names the size literal.
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
        // The variant, not merely a refusal: `2x2_` becoming an
        // `UnexpectedChar` would still be an error and would still be
        // wrong about what the author needs to change.
        let err = lex(source).unwrap_err();
        assert!(
            matches!(err, LexError::TrailingSizeSegment { .. }),
            "{source:?} should be refused as a size literal: {err:?}",
        );
    }
}

/// The refusal is a variant of its own, so a quick-fix can dispatch on it
/// without reading prose.
#[test]
fn the_trailing_segment_refusal_names_the_character_it_found() {
    let text = message("struct s size=2x2x9\n");
    // The literal as written and the character that ended it. Quoting
    // `2x2` alone would leave the author guessing which `x` was the
    // problem, and `contains('x')` on its own is implied by `2x2`.
    assert!(
        text.contains("`2x2`") && text.contains("`x`"),
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
            TokenKind::Int { lexeme: "9".into() },
            TokenKind::Ident("x".into()),
            TokenKind::Newline,
        ],
    );
}

/// A digit run past `i64` is not the lexer's to refuse.
///
/// `scan_number` used to parse every run into an `i64` on the way through,
/// which made the token type a range check on text the lexer cannot type:
/// a truth-table row is digits too, and `11111111111111111111` is a legal
/// twenty-input pattern rather than an overflow. The refusal still exists
/// where a value is actually asked for — `parse_value` reports
/// `IntContext::IntLiteral` — but it belongs there, not here.
#[test]
fn an_integer_past_i64_still_lexes() {
    assert_eq!(
        kinds("99999999999999999999\n"),
        vec![
            TokenKind::Int {
                lexeme: "99999999999999999999".into()
            },
            TokenKind::Newline,
        ],
    );
}

/// The size literal is the other way round, and stays that way: `WxH` is
/// a size in every position it can appear, so the lexer builds one and
/// reports an extent it cannot hold.
#[test]
fn an_extent_past_u32_is_still_the_lexers_to_refuse() {
    let err = lex("struct s size=99999999999999999999x9\n").unwrap_err();
    assert!(matches!(err, LexError::InvalidInt { .. }), "{err:?}");
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
    // Both numbers *and* which is which: `contains('2') && contains('4')`
    // passes just as well with the two swapped, which is the one way this
    // message can be wrong and still look right.
    assert!(
        text.contains("got 4") && text.contains("expected 2"),
        "the message should name the width found and the width expected, in that order: {text}",
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
/// character and stays one: skipping it anywhere would silently accept a
/// file with a mark buried in it, which is a corruption worth reporting.
#[test]
fn a_byte_order_mark_after_the_start_is_still_refused() {
    let err = lex("struct s\u{feff} size=3x3\n").unwrap_err();
    // The position as well as the variant. Testing "it is refused" alone
    // still passes when the skip is keyed on the mark appearing
    // *anywhere* rather than at the start: the file is still rejected,
    // but three bytes of real text have been skipped off the front and
    // every position after them is wrong.
    assert!(
        matches!(
            err,
            LexError::UnexpectedChar {
                ch: '\u{feff}',
                position,
            } if position.line.get() == 1 && position.col.get() == 9,
        ),
        "{err:?}",
    );
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

/// A visible character keeps its glyph. The codepoint rides along
/// because the characters most worth reporting are the ones that look
/// like another: a full-width `＝` is the likeliest way to break a file
/// in a project whose docs are written in Japanese, and it is
/// indistinguishable from `=` in the message without one.
#[test]
fn a_visible_unexpected_character_keeps_its_glyph() {
    for (source, glyph, codepoint) in [
        ("struct s size=3x3\n  floor ?=1\n", '?', "003F"),
        ("struct s size\u{ff1d}3x3\n", '\u{ff1d}', "FF1D"),
        ("struct \u{3042} size=3x3\n", '\u{3042}', "3042"),
    ] {
        let text = message(source);
        assert!(
            text.contains(glyph) && text.contains(codepoint),
            "expected both the glyph and U+{codepoint} in: {text}",
        );
    }
}

/// A BOM occupies a column in every span-derived position — `LineStarts`
/// here and `LineIndex` in the LSP both count characters from the line's
/// start — so the lexer counts it too. Skipping it in one layer and not
/// the others puts a lexer error one column left of a check diagnostic
/// about the same character, on the first line of exactly the files a
/// Windows editor produces.
#[test]
fn a_byte_order_mark_occupies_a_column() {
    let with_bom = lex("\u{feff}struct s size=3x3\n").expect("lex");
    let without = lex("struct s size=3x3\n").expect("lex");
    assert_eq!(
        with_bom[0].position.col.get(),
        without[0].position.col.get() + 1,
        "the mark should shift the first token's column by one",
    );
}

/// Spans stay offsets into the string the caller handed us, which is what
/// lets an editor highlight a diagnostic without knowing whether a mark
/// was skipped. Trimming the source instead would shift every span by
/// three bytes and break nothing else in this file.
#[test]
fn a_byte_order_mark_shifts_every_span_by_its_own_width() {
    let with_bom = lex("\u{feff}struct s size=3x3\n").expect("lex");
    let without = lex("struct s size=3x3\n").expect("lex");
    let bom_len = "\u{feff}".len();
    for (a, b) in with_bom.iter().zip(without.iter()) {
        assert_eq!(
            a.span.start,
            b.span.start + bom_len,
            "span should be an offset into the source as given",
        );
    }
}

/// A file holding nothing but a mark holds no tokens, and one whose first
/// line is indented is still refused for that.
#[test]
fn a_byte_order_mark_alone_is_an_empty_file() {
    assert!(lex("\u{feff}").expect("lex").is_empty());
    assert!(lex("\u{feff}  struct s\n").is_ok());
}
