//! Every layer that names a `line:column` names the same one: the lexer
//! counting as it walks and [`LineStarts`] resolving a byte offset agree,
//! including for a `Newline` token, which sits on the line it ends.

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::lex::{TokenKind, lex};
use cairn_lang_core::parse::parse;

/// One source, written the three ways Cairn accepts. Positions must not
/// depend on which one the author's editor produced.
fn renderings(lf_source: &str) -> [(&'static str, String); 3] {
    [
        ("LF", lf_source.to_owned()),
        ("CRLF", lf_source.replace('\n', "\r\n")),
        ("CR", lf_source.replace('\n', "\r")),
    ]
}

/// Exercises a header, a nested body, a dedent, a comment, and a blank
/// line — the shapes that make the lexer emit synthetic tokens, which are
/// the ones whose positions are chosen rather than read off a byte.
const SAMPLE: &str = "\
struct keep size=9x7
  # a comment
  level y=0
    floor mat_slot=stone

  walls mat_slot=stone
struct hut size=3x3
";

// -- the `Newline` token --------------------------------------------------

#[test]
fn a_newline_token_spans_its_terminator() {
    for (source, expected) in [("a\nb\n", 1..2), ("a\r\nb\r\n", 1..3), ("a\rb\r", 1..2)] {
        let tokens = lex(source).expect("lex");
        let newline = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Newline)
            .expect("a newline token");
        assert_eq!(newline.span, expected, "{source:?}");
    }
}

#[test]
fn a_newline_token_sits_at_the_end_of_the_line_it_ends() {
    for source in ["a\nb\n", "a\r\nb\r\n", "a\rb\r"] {
        let tokens = lex(source).expect("lex");
        let newline = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Newline)
            .expect("a newline token");
        assert_eq!(
            (newline.position.line.get(), newline.position.col.get()),
            (1, 2),
            "{source:?}: the newline ends line 1, so it is at line 1",
        );
    }
}

/// A file that ends without a terminator still ends its last line, and the
/// synthetic `Newline` that closes it belongs at the end of that line.
#[test]
fn an_unterminated_last_line_still_ends_where_its_text_ends() {
    let tokens = lex("a\nbc").expect("lex");
    let last = tokens.last().expect("tokens");
    assert_eq!(last.kind, TokenKind::Newline);
    assert_eq!((last.position.line.get(), last.position.col.get()), (2, 3));
    assert_eq!(last.span, 4..4);
}

// -- the two layers agree -------------------------------------------------

/// Synthetic tokens included: `Indent` and `Dedent` span the leading
/// whitespace, and the `Newline` spans its terminator.
#[test]
fn every_token_position_is_the_position_of_its_span() {
    for (label, source) in renderings(SAMPLE) {
        let lines = LineStarts::new(&source);
        for token in lex(&source).expect("lex") {
            assert_eq!(
                token.position,
                lines.position(&source, token.span.start),
                "{label}: {:?} at byte {}",
                token.kind,
                token.span.start,
            );
        }
    }
}

/// The lone-CR case on its own, because it is the one a `\n`-only line
/// counter gets wrong while looking right on every other input.
#[test]
fn a_lone_carriage_return_starts_a_line_in_both_layers() {
    let source = "ab\rcd $\n";
    let dollar = source.find('$').expect("fixture holds a `$`");
    let resolved = LineStarts::new(source).position(source, dollar);
    let lexed = lex(source).expect_err("`$` is not a token").position();
    assert_eq!(resolved, lexed);
    assert_eq!((resolved.line.get(), resolved.col.get()), (2, 4));
}

// -- what the author reads ------------------------------------------------

/// Every real file ends in a newline, which must not move an end-of-line
/// error off the line that caused it.
#[test]
fn a_trailing_newline_does_not_move_an_end_of_line_error() {
    let terminated = parse("def foo bar\n").expect_err("`bar` needs a value");
    let bare = parse("def foo bar").expect_err("`bar` needs a value");
    assert_eq!(terminated.position(), bare.position());
    let position = terminated.position();
    assert_eq!((position.line.get(), position.col.get()), (1, 12));
}

/// A file ending in a terminator has an empty line after it, which
/// `lines::starts` counts, but nothing on it is wrong. `lines::split` drops
/// that line, so its count is the lines with text on them.
#[test]
fn an_end_of_line_error_names_a_line_with_text_on_it() {
    for source in [
        "def foo bar\n",
        "site s:\n  walls height=\n",
        "site s:\n  walls mat=[a, b\n",
    ] {
        let error = parse(source).expect_err("each source is incomplete");
        let reported = error.position().line.get() as usize;
        let lines = cairn_lang_core::lines::split(source).count();
        assert!(
            reported <= lines,
            "{source:?} has {lines} lines with text, error reported at line {reported}",
        );
        let offending = cairn_lang_core::lines::split(source)
            .nth(reported - 1)
            .expect("the reported line exists");
        assert_eq!(
            error.position().col.get() as usize,
            offending.chars().count() + 1,
            "{source:?} should point one past the end of {offending:?}",
        );
    }
}

#[test]
fn an_end_of_line_error_reports_the_same_place_for_every_line_ending() {
    let mut reported = Vec::new();
    for (label, source) in renderings("site s:\n  walls height=\n") {
        let error = parse(&source).expect_err("`height` needs a value");
        reported.push((label, error.position()));
    }
    let [(_, first), rest @ ..] = &reported[..] else {
        unreachable!("three renderings")
    };
    for (label, position) in rest {
        assert_eq!(position, first, "{label} disagrees with LF");
    }
    assert_eq!((first.line.get(), first.col.get()), (2, 16));
}

mod byte_order_mark {
    //! A byte-order mark shifts every layer's columns by the same amount: the
    //! lexer counts it, as `LineStarts::position` and the LSP's `LineIndex` do,
    //! so a lexer error and a check finding about one character share a column.

    use cairn_lang_core::check::LineStarts;
    use cairn_lang_core::lex::lex;

    /// The character both layers are asked about is the `$`, whose column the
    /// BOM moves by one wherever it is measured from.
    const WITHOUT_BOM: &str = "struct s size=3x3 $\n";

    fn with_bom() -> String {
        format!("\u{feff}{WITHOUT_BOM}")
    }

    /// The lexer's own position for a stray character, with and without the
    /// mark in front of it.
    fn lexer_column(source: &str) -> u32 {
        lex(source)
            .expect_err("the `$` is not a token")
            .position()
            .col
            .get()
    }

    /// The same character's column, resolved from a byte offset the way every
    /// check diagnostic resolves one.
    fn span_column(source: &str) -> u32 {
        let offset = source.find('$').expect("fixture holds a `$`");
        LineStarts::new(source).position(source, offset).col.get()
    }

    #[test]
    fn the_lexer_and_span_positions_agree_about_a_byte_order_mark() {
        let bom = with_bom();
        assert_eq!(
            lexer_column(&bom),
            span_column(&bom),
            "a lexer error and a check diagnostic disagree about the same character",
        );
        assert_eq!(
            lexer_column(WITHOUT_BOM),
            span_column(WITHOUT_BOM),
            "they should already agree without a mark",
        );
    }

    /// And the mark moves both by exactly its own width — one character, not
    /// its three bytes.
    #[test]
    fn a_byte_order_mark_costs_one_column_in_both() {
        let bom = with_bom();
        assert_eq!(lexer_column(&bom), lexer_column(WITHOUT_BOM) + 1);
        assert_eq!(span_column(&bom), span_column(WITHOUT_BOM) + 1);
    }
}
