//! A file that does not parse has a diagnostic like every other finding.
//!
//! Until this existed, a parse failure was a `ParseError` and nothing else:
//! no code, no span, no way to render it beside the check pass's findings.
//! Every consumer that wanted to show one built its own shape — the CLI
//! wrote a bare `error:` line and the language server kept a private
//! converter — and `--format json` had nothing to put on stdout at all.

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::error::{LexError, ParseError};
use cairn_lang_core::{Severity, diagnose_parse_failure, parse};

/// One source per shape a parse can fail in.
///
/// The third column names the variant it must produce, so a source that
/// stops reaching its variant — because the lexer got stricter, or the
/// parser started recovering — fails here rather than quietly leaving that
/// variant unrendered.
///
/// `LexError::UnmatchedDedent` has no row because no source reaches it. A
/// level only enters the indent stack through an indent of exactly one
/// level, and an odd count is refused before the comparison, so every even
/// level below the current one is already on the stack and the arm that
/// raises it cannot be taken. Written down rather than left as a gap: if
/// the indent rules change, this is the note that says the row is now
/// missing.
const REFUSED: &[(&str, &str, &str)] = &[
    ("tab_indent", "theme t:\n\tslot a -> @b\n", "TabIndent"),
    ("odd_indent", "theme t:\n   slot a -> @b\n", "OddIndent"),
    (
        "indent_jump",
        "struct keep size=3x3\n      floor mat_slot=wall\n",
        "IndentJump",
    ),
    (
        "unterminated_string",
        "theme t:\n  slot a -> \"x\n",
        "UnterminatedString",
    ),
    (
        "unexpected_char",
        "struct s size=3x3\n  floor a=%\n",
        "UnexpectedChar",
    ),
    ("syntax_stray_token", "struct s size=2x2\n;;;\n", "Syntax"),
    (
        "trailing_size_segment",
        "struct s size=2x2x9\n",
        "TrailingSizeSegment",
    ),
    (
        "invalid_int",
        "struct s size=3x3\n  floor n=99999999999999999999\n",
        "InvalidInt",
    ),
    ("syntax", "widget w\n  floor mat_slot=f\n", "Syntax"),
];

/// A value nested past the parser's depth limit, built rather than written
/// out: the limit is a constant, and a literal deep enough to pass it today
/// would stop passing it the moment the constant moves.
fn nested_too_deep() -> String {
    let depth = cairn_lang_core::MAX_NESTING_DEPTH + 6;
    format!(
        "struct s size=3x3\n  floor a={}{}\n",
        "[".repeat(depth),
        "]".repeat(depth),
    )
}

/// The variant name a `ParseError` reports itself as, for the table above.
fn variant_of(err: &ParseError) -> &'static str {
    match err {
        ParseError::Lex(LexError::TabIndent { .. }) => "TabIndent",
        ParseError::Lex(LexError::OddIndent { .. }) => "OddIndent",
        ParseError::Lex(LexError::IndentJump { .. }) => "IndentJump",
        ParseError::Lex(LexError::UnmatchedDedent { .. }) => "UnmatchedDedent",
        ParseError::Lex(LexError::UnterminatedString { .. }) => "UnterminatedString",
        ParseError::Lex(LexError::UnexpectedChar { .. }) => "UnexpectedChar",
        ParseError::Lex(LexError::TrailingSizeSegment { .. }) => "TrailingSizeSegment",
        ParseError::Lex(LexError::InvalidInt { .. }) | ParseError::InvalidInt { .. } => {
            "InvalidInt"
        }
        ParseError::Syntax { .. } => "Syntax",
        ParseError::NestingTooDeep { .. } => "NestingTooDeep",
        _ => "unknown",
    }
}

/// The table's sources plus the generated one, as owned rows.
fn rows() -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = REFUSED
        .iter()
        .map(|(name, source, _)| (*name, (*source).to_owned()))
        .collect();
    out.push(("nesting_too_deep", nested_too_deep()));
    out
}

fn refusal(source: &str) -> ParseError {
    parse(source).expect_err("the fixture is supposed to be refused; it no longer is")
}

/// Every shape a parse fails in renders under one code.
#[test]
fn every_refusal_carries_the_parse_code() {
    for (name, source) in rows() {
        let diagnostic = diagnose_parse_failure(&source, &refusal(&source));
        assert_eq!(
            diagnostic.code.as_str(),
            "E_PARSE",
            "{name} should render under the parse code",
        );
        assert_eq!(
            diagnostic.severity(),
            Severity::Error,
            "{name} should be an error",
        );
    }
}

/// The table above still reaches every shape it names, and the generated
/// source still reaches the depth limit.
#[test]
fn the_refusals_cover_the_shapes_they_claim() {
    for (name, source, expected) in REFUSED {
        assert_eq!(
            variant_of(&refusal(source)),
            *expected,
            "{name} no longer reaches the variant it was written for",
        );
    }
    assert_eq!(
        variant_of(&refusal(&nested_too_deep())),
        "NestingTooDeep",
        "the generated source no longer reaches the depth limit",
    );
}

/// The diagnostic points where the error says it does, and reaches the end
/// of the line it starts on.
///
/// A parse failure has a position and no span — the parser stops at a
/// token, it does not carry a range for the construct that failed. Giving
/// the diagnostic the rest of the line is what makes it visible in an
/// editor: a zero-width range renders as a caret with nothing under it.
#[test]
fn the_span_runs_from_the_reported_position_to_the_end_of_its_line() {
    for (name, source) in rows() {
        let err = refusal(&source);
        let diagnostic = diagnose_parse_failure(&source, &err);
        let lines = LineStarts::new(&source);
        assert_eq!(
            lines.position(&source, diagnostic.span.start),
            err.position(),
            "{name} should start where the error says it does",
        );
        let rest = &source[diagnostic.span.end..];
        assert!(
            rest.is_empty() || rest.starts_with('\n') || rest.starts_with('\r'),
            "{name} should end at a line break or at the end of the source, \
             but the text after it starts {:?}",
            rest.chars().next(),
        );
        assert!(
            !source[diagnostic.span.clone()].contains(['\n', '\r']),
            "{name} should not span a line break",
        );
    }
}

/// The message is the parse error's own, with no position glued to it.
///
/// Every renderer puts the position in front of the message from the span,
/// so a message carrying its own `line:col` would print it twice.
#[test]
fn the_message_is_the_error_without_its_position() {
    for (name, source) in rows() {
        let err = refusal(&source);
        let diagnostic = diagnose_parse_failure(&source, &err);
        assert_eq!(
            diagnostic.primary,
            err.user_message(),
            "{name} should carry the error's own message",
        );
        assert!(
            !diagnostic.primary.starts_with(&err.position().to_string()),
            "{name} repeats the position the renderer already prints: {}",
            diagnostic.primary,
        );
    }
}

/// A parse failure obeys the prose rules every other diagnostic obeys.
///
/// `diagnostic_text.rs` sweeps the check pass's findings, and it cannot
/// reach these: its corpus parses first. The same rules are asserted here
/// so a message with a dropped `\` continuation, or one that breaks the
/// one-diagnostic-per-line shape, fails somewhere.
#[test]
fn no_refusal_message_breaks_the_line_it_is_rendered_on() {
    for (name, source) in rows() {
        let text = diagnose_parse_failure(&source, &refusal(&source)).primary;
        assert!(
            !text.contains("  "),
            "{name} renders a run of spaces, which is a dropped `\\` line \
             continuation in the literal: {text:?}",
        );
        assert!(
            !text.contains('\n') && !text.contains('\t'),
            "{name} embeds its own line break: {text:?}",
        );
    }
}
