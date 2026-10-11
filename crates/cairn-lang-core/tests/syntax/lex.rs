//! The lexer: the tokens a source becomes, and where each one sits.

use cairn_lang_core::error::LexError;
use cairn_lang_core::lex::{TokenKind, lex};

use crate::{kinds, pos};

#[test]
fn lexes_simple_command() {
    let kinds = kinds("walls class=outer mat_slot=wall height=4\n");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Ident("walls".into()),
            TokenKind::Ident("class".into()),
            TokenKind::Eq,
            TokenKind::Ident("outer".into()),
            TokenKind::Ident("mat_slot".into()),
            TokenKind::Eq,
            TokenKind::Ident("wall".into()),
            TokenKind::Ident("height".into()),
            TokenKind::Eq,
            TokenKind::Int { lexeme: "4".into() },
            TokenKind::Newline,
        ]
    );
}

#[test]
fn lexes_size_literal() {
    let kinds = kinds("struct cottage size=9x7\n");
    assert!(kinds.contains(&TokenKind::Size(9, 7)));
}

#[test]
fn lexes_header_and_at_token() {
    let kinds = kinds("@cairn 2026.06\n  slot floor -> @oak_planks\n");
    assert_eq!(kinds[0], TokenKind::At);
    assert_eq!(kinds[1], TokenKind::Ident("cairn".into()));
    assert_eq!(
        kinds[2],
        TokenKind::Int {
            lexeme: "2026".into()
        }
    );
    assert_eq!(kinds[3], TokenKind::Dot);
    assert_eq!(
        kinds[4],
        TokenKind::Int {
            lexeme: "06".into()
        }
    );
    assert_eq!(kinds[5], TokenKind::Newline);
    assert_eq!(kinds[6], TokenKind::Indent);
    assert_eq!(kinds[7], TokenKind::Ident("slot".into()));
    assert_eq!(kinds[8], TokenKind::Ident("floor".into()));
    assert_eq!(kinds[9], TokenKind::Arrow);
    assert_eq!(kinds[10], TokenKind::At);
    assert_eq!(kinds[11], TokenKind::Ident("oak_planks".into()));
}

#[test]
fn lexes_indent_and_dedent() {
    let source = "theme medieval:\n  slot floor -> @oak_planks\nstruct cottage size=9x7\n";
    let kinds = kinds(source);
    let indents = kinds
        .iter()
        .filter(|k| matches!(k, TokenKind::Indent))
        .count();
    let dedents = kinds
        .iter()
        .filter(|k| matches!(k, TokenKind::Dedent))
        .count();
    assert_eq!(indents, 1, "one indent for the theme body");
    assert_eq!(dedents, 1, "one dedent before struct");
}

#[test]
fn skips_comments_and_blank_lines() {
    let source = "\
# leading comment
@cairn 2026.06

# blank above
struct keep size=11x9
";
    let kinds = kinds(source);
    assert_eq!(kinds[0], TokenKind::At);
    assert_eq!(kinds[1], TokenKind::Ident("cairn".into()));
}

#[test]
fn tabs_are_rejected() {
    let err = lex("\tstruct\n").unwrap_err();
    assert!(matches!(err, LexError::TabIndent { .. }));
}

#[test]
fn lexes_arrow_and_comparison() {
    let kinds = kinds("@requires version>=1.20\n");
    assert!(kinds.iter().any(|k| matches!(k, TokenKind::GreaterEq)));
}

#[test]
fn lexes_string_list() {
    let kinds = kinds("@intended_targets [\"1.20.4\",\"1.21.4\"]\n");
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, TokenKind::Str(s) if s == "1.20.4"))
    );
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, TokenKind::Str(s) if s == "1.21.4"))
    );
}

#[test]
fn tracks_line_and_column() {
    let tokens = lex("theme medieval:\n  slot floor -> @oak_planks\n").expect("lex");
    let slot_token = tokens
        .iter()
        .find(|t| matches!(&t.kind, TokenKind::Ident(s) if s == "slot"))
        .expect("slot token");
    assert_eq!(slot_token.position, pos(2, 3));
}

#[test]
fn accepts_crlf_line_endings() {
    // What a `core.autocrlf=true` checkout hands the lexer.
    let kinds = kinds("walls class=outer height=4\r\nfloor mat_slot=floor\r\n");
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, TokenKind::Ident(s) if s == "walls"))
    );
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, TokenKind::Ident(s) if s == "floor"))
    );
    let newlines = kinds
        .iter()
        .filter(|k| matches!(k, TokenKind::Newline))
        .count();
    assert_eq!(newlines, 2);
}

#[test]
fn accepts_lone_cr_line_endings() {
    // Pre-OSX Mac line endings — lenient acceptance.
    let kinds = kinds("walls height=4\rfloor mat_slot=floor\r");
    let newlines = kinds
        .iter()
        .filter(|k| matches!(k, TokenKind::Newline))
        .count();
    assert_eq!(newlines, 2);
}

#[test]
fn col_counts_characters_not_bytes_in_strings() {
    let source = "@intended_targets [\"日本語\"]\n@cairn 2026.06\n";
    let tokens = lex(source).expect("lex");
    let cairn = tokens
        .iter()
        .find(|t| matches!(&t.kind, TokenKind::Ident(s) if s == "cairn"))
        .expect("cairn token");
    assert_eq!(cairn.position, pos(2, 2));
}

#[test]
fn non_ascii_outside_string_is_reported_as_character() {
    let err = lex("struct あ size=1x1\n").unwrap_err();
    match err {
        LexError::UnexpectedChar { ch, .. } => assert_eq!(ch, 'あ'),
        other => panic!("expected UnexpectedChar, got {other:?}"),
    }
}

#[test]
fn unterminated_string_is_reported() {
    let err = lex("@intended_targets [\"open\n").unwrap_err();
    assert!(matches!(err, LexError::UnterminatedString { .. }));
}

#[test]
fn dedent_emits_per_level_closed() {
    // `struct t` closes two levels at once.
    let source = "struct s\n  level y=0\n    door side=front\nstruct t\n";
    let kinds = kinds(source);
    let dedents = kinds
        .iter()
        .filter(|k| matches!(k, TokenKind::Dedent))
        .count();
    assert_eq!(dedents, 2);
}

/// An `UnexpectedChar` would put a pre-release suffix such as `-rc1` out
/// of reach of `@requires`, and `spec/versioning-editions` "The target is a
/// compile-time parameter" counts such labels as real versions.
#[test]
fn a_lone_dash_lexes_as_a_token_rather_than_failing() {
    let suffixed = kinds("@requires version>=1.21.4-rc1\n");
    assert!(
        suffixed.contains(&TokenKind::Minus),
        "the `-` should be a token: {suffixed:?}",
    );
    assert!(
        !suffixed.contains(&TokenKind::Arrow),
        "and not half an arrow: {suffixed:?}",
    );
    // The arrow still wins where it is one.
    let arrow = kinds("  slot floor -> @oak_planks\n");
    assert!(arrow.contains(&TokenKind::Arrow), "{arrow:?}");
    assert!(!arrow.contains(&TokenKind::Minus), "{arrow:?}");
}

/// No value position reads a `-`, so lexing one does not make a negative
/// integer a value. The parser names the token it found, which is a better
/// answer than a character offset.
#[test]
fn a_lone_dash_is_still_refused_where_no_grammar_reads_one() {
    let err = cairn_lang_core::parse("struct s size=2x2\n  level y=-3\n")
        .expect_err("no value position reads a `-`");
    assert!(
        err.to_string().contains('-'),
        "the message should name what it found: {err}",
    );
}

mod refusals {
    //! What the lexer refuses, and what it says: text it could quietly reshape
    //! (a third size extent, an indent two levels deep, a byte-order mark past
    //! the start) is refused, with a message naming what would fix the file
    //! rather than one the file already satisfies.

    use cairn_lang_core::error::LexError;
    use cairn_lang_core::lex::{TokenKind, lex};

    use crate::kinds;

    fn message(source: &str) -> String {
        lex(source).expect_err("lex should refuse").user_message()
    }

    // -- size literals --------------------------------------------------------

    /// Lexed as `Size(2,2)` followed by `Ident("x9")`, `2x2x9` would be
    /// reported as a bare value at a column past the literal, or in a
    /// declaration header not at all, and neither names the size literal.
    #[test]
    fn a_size_literal_with_a_third_extent_is_refused() {
        let err = lex("struct s size=2x2x9\n").unwrap_err();
        assert!(
            matches!(&err, LexError::TrailingSizeSegment { found, .. } if *found == 'x'),
            "{err:?}",
        );
    }

    /// Any identifier byte continues the run the author was writing, so the
    /// token would not hold the literal they meant.
    #[test]
    fn a_size_literal_followed_by_a_word_character_is_refused() {
        for source in [
            "struct s size=2x2y\n",
            "struct s size=2x2_\n",
            "struct s size=2x2x\n",
        ] {
            // The variant, not merely a refusal: an `UnexpectedChar` would
            // be wrong about what the author needs to change.
            let err = lex(source).unwrap_err();
            assert!(
                matches!(err, LexError::TrailingSizeSegment { .. }),
                "{source:?} should be refused as a size literal: {err:?}",
            );
        }
    }

    #[test]
    fn the_trailing_segment_refusal_names_the_character_it_found() {
        let text = message("struct s size=2x2x9\n");
        // Quoting `2x2` alone would leave the author guessing which `x` was
        // the problem, and a bare `contains('x')` is implied by `2x2`.
        assert!(
            text.contains("`2x2`") && text.contains("`x`"),
            "the message should quote the literal and the character after it: {text}",
        );
    }

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

    /// Leading zeros are part of the extent, not a trailing segment.
    #[test]
    fn leading_zeros_in_an_extent_still_lex() {
        assert_eq!(kinds("9x0007\n")[0], TokenKind::Size(9, 7));
    }

    /// `9x` has no height digits, so it is never a `Size`: it is `Int` then
    /// `Ident`, and the parser is what refuses it. The refusal stays out of
    /// the lexer, because `9` followed by an identifier is legal in other
    /// positions.
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

    /// A truth-table row is digits too, and `11111111111111111111` is a legal
    /// twenty-input pattern rather than an overflow. The refusal belongs where
    /// a value is asked for, which reports `IntContext::IntLiteral`.
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

    /// The size literal is the other way round: `WxH` is a size in every
    /// position it can appear, so the lexer builds one and reports an extent
    /// it cannot hold.
    #[test]
    fn an_extent_past_u32_is_still_the_lexers_to_refuse() {
        let err = lex("struct s size=99999999999999999999x9\n").unwrap_err();
        assert!(matches!(err, LexError::InvalidInt { .. }), "{err:?}");
    }

    // -- indentation ----------------------------------------------------------

    /// "indentation must be a multiple of 2 spaces (got 4)" would tell the
    /// author to do what they already did, so a jump has a variant of its own.
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

    #[test]
    fn the_jump_refusal_names_the_width_that_would_work() {
        let text = message("struct keep size=3x3\n    floor mat_slot=wall\n");
        // Which number is which: swapped is the one way this message can be
        // wrong and still look right.
        assert!(
            text.contains("got 4") && text.contains("expected 2"),
            "the message should name the width found and the width expected, in that order: {text}",
        );
    }

    /// Jumping to 6 is a different repair from inside a body than from the top
    /// level.
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

    #[test]
    fn odd_indentation_is_its_own_refusal() {
        let err = lex("theme medieval:\n   slot floor -> @x\n").unwrap_err();
        assert!(matches!(err, LexError::OddIndent { got: 3, .. }), "{err:?}");
    }

    /// Rounding to a legal width is the first repair, and the level means
    /// something only once the width does.
    #[test]
    fn an_odd_jump_reports_the_odd_width() {
        let err = lex("struct keep size=3x3\n     floor mat_slot=wall\n").unwrap_err();
        assert!(matches!(err, LexError::OddIndent { got: 5, .. }), "{err:?}");
    }

    // -- byte-order marks and unprintable characters --------------------------

    /// U+FEFF is what a default Windows editor writes. Refused as an
    /// unexpected character quoted verbatim, it would render as nothing, and
    /// the message would name an empty string.
    #[test]
    fn a_leading_byte_order_mark_is_skipped() {
        let with_bom = lex("\u{feff}struct s size=3x3\n").expect("a leading BOM should lex");
        let without = lex("struct s size=3x3\n").expect("lex");
        let strip = |tokens: Vec<cairn_lang_core::lex::Token>| {
            tokens.into_iter().map(|t| t.kind).collect::<Vec<_>>()
        };
        assert_eq!(strip(with_bom), strip(without));
    }

    /// A mark past offset 0 is a stray character, and skipping it would
    /// accept a corrupted file in silence.
    #[test]
    fn a_byte_order_mark_after_the_start_is_still_refused() {
        let err = lex("struct s\u{feff} size=3x3\n").unwrap_err();
        // The position too: a skip keyed on a mark anywhere still refuses
        // this file, but with three bytes of real text skipped off the front.
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

    /// U+00A0 renders as a space, so quoted verbatim the message would say a
    /// space was unexpected.
    #[test]
    fn an_invisible_character_is_named_by_codepoint() {
        let text = message("struct s\u{a0}size=3x3\n");
        assert!(
            text.to_ascii_uppercase().contains("A0"),
            "the message should carry the codepoint: {text}",
        );
    }

    /// The codepoint rides along because the characters most worth reporting
    /// look like another: a full-width `＝` is the likeliest break in a
    /// project whose docs are in Japanese, and reads as `=` without one.
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

    /// `LineStarts` here and `LineIndex` in the LSP both count the mark, so a
    /// lexer that skipped it would put its errors one column left of every
    /// other layer's on the first line.
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

    /// Spans stay offsets into the string the caller handed us, so an editor
    /// highlights a diagnostic without knowing whether a mark was skipped.
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

    /// A file holding nothing but a mark holds no tokens, and the mark adds
    /// nothing to the first line's indentation.
    #[test]
    fn a_byte_order_mark_alone_is_an_empty_file() {
        assert!(lex("\u{feff}").expect("lex").is_empty());
        assert!(lex("\u{feff}  struct s\n").is_ok());
    }
}
