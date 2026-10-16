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

#[test]
fn a_line_inside_a_truth_table_takes_no_indent_or_dedent() {
    let source = "struct s\n  assert truth(a -> b) {\n      0 -> 1\n 1 -> 0\n    }\n  floor x=1\n";
    let int = |lexeme: &str| TokenKind::Int {
        lexeme: lexeme.into(),
    };
    assert_eq!(
        kinds(source),
        vec![
            TokenKind::Ident("struct".into()),
            TokenKind::Ident("s".into()),
            TokenKind::Newline,
            TokenKind::Indent,
            TokenKind::Ident("assert".into()),
            TokenKind::Ident("truth".into()),
            TokenKind::LParen,
            TokenKind::Ident("a".into()),
            TokenKind::Arrow,
            TokenKind::Ident("b".into()),
            TokenKind::RParen,
            TokenKind::LBrace,
            TokenKind::Newline,
            int("0"),
            TokenKind::Arrow,
            int("1"),
            TokenKind::Newline,
            int("1"),
            TokenKind::Arrow,
            int("0"),
            TokenKind::Newline,
            TokenKind::RBrace,
            TokenKind::Newline,
            TokenKind::Ident("floor".into()),
            TokenKind::Ident("x".into()),
            TokenKind::Eq,
            int("1"),
            TokenKind::Newline,
            TokenKind::Dedent,
        ],
    );
}

#[test]
fn a_tab_inside_a_truth_table_is_still_rejected() {
    let err = lex("struct s\n  assert truth(a -> b) {\n  \t0 -> 1\n  }\n").unwrap_err();
    assert_eq!(
        err,
        LexError::TabIndent {
            position: pos(3, 3)
        }
    );
}

/// A directive's raw value is text, so its `{` opens no table.
#[test]
fn a_brace_off_an_assert_line_leaves_the_layout_below_it_alone() {
    assert_eq!(
        kinds("@cairn 2026.06 {\nstruct s\n  floor x=1\n"),
        vec![
            TokenKind::At,
            TokenKind::Ident("cairn".into()),
            TokenKind::Int {
                lexeme: "2026".into()
            },
            TokenKind::Dot,
            TokenKind::Int {
                lexeme: "06".into()
            },
            TokenKind::LBrace,
            TokenKind::Newline,
            TokenKind::Ident("struct".into()),
            TokenKind::Ident("s".into()),
            TokenKind::Newline,
            TokenKind::Indent,
            TokenKind::Ident("floor".into()),
            TokenKind::Ident("x".into()),
            TokenKind::Eq,
            TokenKind::Int { lexeme: "1".into() },
            TokenKind::Newline,
            TokenKind::Dedent,
        ],
    );
}

/// `spec/versioning-editions` "The target is a compile-time parameter" counts `-rc1` as a version.
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
    //! What the lexer refuses, with a message naming what would fix the file.

    use cairn_lang_core::error::LexError;
    use cairn_lang_core::lex::{TokenKind, lex};

    use crate::kinds;

    fn message(source: &str) -> String {
        lex(source).expect_err("lex should refuse").user_message()
    }

    // -- size literals --------------------------------------------------------

    #[test]
    fn a_size_literal_with_a_third_extent_is_refused() {
        let err = lex("struct s size=2x2x9\n").unwrap_err();
        assert!(
            matches!(&err, LexError::TrailingSizeSegment { found, .. } if *found == 'x'),
            "{err:?}",
        );
    }

    #[test]
    fn a_size_literal_followed_by_a_word_character_is_refused() {
        for source in [
            "struct s size=2x2y\n",
            "struct s size=2x2_\n",
            "struct s size=2x2x\n",
        ] {
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

    #[test]
    fn leading_zeros_in_an_extent_still_lex() {
        assert_eq!(kinds("9x0007\n")[0], TokenKind::Size(9, 7));
    }

    /// `9` followed by an identifier is legal elsewhere, so the parser refuses `9x`, not the lexer.
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

    /// `11111111111111111111` is a legal truth-table row; overflow is a value reader's to refuse.
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

    /// `WxH` is a size wherever it appears, so the lexer refuses an extent it cannot hold.
    #[test]
    fn an_extent_past_u32_is_still_the_lexers_to_refuse() {
        let err = lex("struct s size=99999999999999999999x9\n").unwrap_err();
        assert!(matches!(err, LexError::InvalidInt { .. }), "{err:?}");
    }

    // -- indentation ----------------------------------------------------------

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
        assert!(
            text.contains("got 4") && text.contains("expected 2"),
            "the message should name the width found and the width expected, in that order: {text}",
        );
    }

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

    #[test]
    fn an_odd_jump_reports_the_odd_width() {
        let err = lex("struct keep size=3x3\n     floor mat_slot=wall\n").unwrap_err();
        assert!(matches!(err, LexError::OddIndent { got: 5, .. }), "{err:?}");
    }

    // -- byte-order marks and unprintable characters --------------------------

    /// U+FEFF is what a default Windows editor writes.
    #[test]
    fn a_leading_byte_order_mark_is_skipped() {
        let with_bom = lex("\u{feff}struct s size=3x3\n").expect("a leading BOM should lex");
        let without = lex("struct s size=3x3\n").expect("lex");
        let strip = |tokens: Vec<cairn_lang_core::lex::Token>| {
            tokens.into_iter().map(|t| t.kind).collect::<Vec<_>>()
        };
        assert_eq!(strip(with_bom), strip(without));
    }

    #[test]
    fn a_byte_order_mark_after_the_start_is_still_refused() {
        let err = lex("struct s\u{feff} size=3x3\n").unwrap_err();
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

    #[test]
    fn an_invisible_character_is_named_by_codepoint() {
        let text = message("struct s\u{a0}size=3x3\n");
        assert!(
            text.to_ascii_uppercase().contains("A0"),
            "the message should carry the codepoint: {text}",
        );
    }

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

    /// `LineStarts` and the LSP's `LineIndex` both count the mark.
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

    #[test]
    fn a_byte_order_mark_alone_is_an_empty_file() {
        assert!(lex("\u{feff}").expect("lex").is_empty());
        assert!(lex("\u{feff}  struct s\n").is_ok());
    }
}
