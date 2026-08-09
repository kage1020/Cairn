//! Every layer that names a `line:column` names the same one.
//!
//! Two layers in this crate answer that question by different routes: the
//! lexer counts as it walks, and [`LineStarts`] resolves a byte offset
//! after the fact. A diagnostic from one is read next to a diagnostic from
//! the other — and next to an editor's cursor — so a disagreement is not a
//! cosmetic difference, it is a wrong line number in the one message whose
//! whole job is to say where to look.
//!
//! The `Newline` token is where they used to disagree. It was pushed after
//! the line break was consumed, so it recorded the first column of the
//! *following* line and carried an empty span at that line's first byte.
//! Since a parse error at the end of a line reports the position of the
//! token it stopped at, every "expected X, got end of line" pointed one
//! line too far — and for the last line of a file, at a line that does not
//! exist.

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

/// The general invariant, which subsumes the specific cases above: a
/// token's recorded position is the position of its span. It holds for
/// synthetic tokens too — `Indent` and `Dedent` span the leading
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

/// The same invariant stated against the other resolver, since the CLI
/// reaches for whichever is cheaper at the call site.
#[test]
fn the_offset_resolvers_agree_with_each_other() {
    for (label, source) in renderings(SAMPLE) {
        let lines = LineStarts::new(&source);
        for (offset, _) in source.char_indices().chain([(source.len(), ' ')]) {
            assert_eq!(
                lines.position(&source, offset),
                cairn_lang_core::check::position_at(&source, offset),
                "{label} at byte {offset}",
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

/// The reported defect: the trailing newline every real file has moved the
/// error off the line that caused it.
#[test]
fn a_trailing_newline_does_not_move_an_end_of_line_error() {
    let terminated = parse("def foo bar\n").expect_err("`bar` needs a value");
    let bare = parse("def foo bar").expect_err("`bar` needs a value");
    assert_eq!(terminated.position(), bare.position());
    let position = terminated.position();
    assert_eq!((position.line.get(), position.col.get()), (1, 12));
}

/// And it must name a line the file has. `"…\n"` has one line; reporting
/// line 2 sends a reader — or a model repairing its own output — to a line
/// that is not there.
#[test]
fn an_end_of_line_error_names_a_line_the_file_has() {
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
            "{source:?} has {lines} lines, error reported at line {reported}",
        );
        // And specifically at the end of the offending line, one column
        // past its last character.
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

/// Whichever way the file is written.
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
