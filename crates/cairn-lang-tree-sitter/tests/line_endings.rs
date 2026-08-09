//! Line-ending regression test for the external scanner.
//!
//! `src/scanner.c` accepts `\r\n` as well as `\n` on both of its line-break
//! paths: the NEWLINE branch, and the blank/comment-only-line skip loop that
//! precedes indent counting. Nothing else pins that behaviour — the corpus
//! under `test/corpus/` cannot, because the repository's `.gitattributes`
//! normalizes every text file to LF on commit, so a `\r\n` written into a
//! corpus fixture would silently become `\n` and stop testing anything. The
//! source is therefore built here, in Rust, where the bytes are literal.

use tree_sitter::Parser;

/// Exercises both scanner line-break paths in one file: a nested indent
/// (NEWLINE plus INDENT/DEDENT), a blank line and a comment-only line (the
/// skip loop), and a trailing comment (the normal lexer's `comment` extra,
/// whose regex also has to stop at `\r`).
const LF_SOURCE: &str = concat!(
    "@cairn 2026.06\n",
    "struct keep size=5x5\n",
    "  floor mat_slot=floor\n",
    "\n",
    "# a comment on its own line\n",
    "  level id=l0 y=0\n",
    "    door id=entry side=front  # eol note\n",
);

/// `label` names which line-ending variant is being parsed, and the source is
/// printed escaped: a rejection here can come from either side of the
/// comparison below, and a raw `\r` is invisible in panic output, so neither
/// the failing variant nor its bytes would otherwise be identifiable.
fn parse_to_sexp(label: &str, source: &str) -> String {
    let mut parser = Parser::new();
    parser
        .set_language(&cairn_lang_tree_sitter::LANGUAGE.into())
        .expect("load cairn language");
    let tree = parser.parse(source, None).expect("parse produced no tree");
    let root = tree.root_node();
    assert!(
        !root.has_error(),
        "grammar rejected the {label} source: {source:?}"
    );
    root.to_sexp()
}

/// A CRLF file must produce the same tree as its LF twin — not merely an
/// error-free one. Comparing the whole s-expression catches a `\r` that is
/// consumed as a separate NEWLINE (which would double every line break) or
/// left behind for the normal lexer (which would surface as an extra node).
#[test]
fn crlf_source_parses_identically_to_lf() {
    let crlf = LF_SOURCE.replace('\n', "\r\n");
    assert_eq!(parse_to_sexp("CRLF", &crlf), parse_to_sexp("LF", LF_SOURCE));
}

/// A lone `\r` must too, and this variant is the one that needs the whole
/// s-expression rather than an error check.
///
/// `get_column()` restarts at `\n` and at nothing else, so in a `\r`-only
/// file every line is measured against a column that has been climbing
/// since the file began. Getting that wrong does not reliably fail the
/// parse — it can just as easily nest a member one level too deep, which
/// nothing but a tree comparison sees.
///
/// `LF_SOURCE` is the right fixture because it already holds a blank line
/// and a comment-only line, and those are what carry the scanner across a
/// second line break before it measures the next one — the case where the
/// base column has to move with it.
#[test]
fn lone_cr_source_parses_identically_to_lf() {
    let cr = LF_SOURCE.replace('\n', "\r");
    assert_eq!(
        parse_to_sexp("lone CR", &cr),
        parse_to_sexp("LF", LF_SOURCE)
    );
}

/// The last line of a file need not be terminated; dropping the final line
/// break is what makes the scanner synthesize its zero-width EOF NEWLINE, and
/// that path has to behave the same whichever line ending the rest of the
/// file uses.
#[test]
fn crlf_source_without_final_line_break_parses_identically_to_lf() {
    let lf = LF_SOURCE.trim_end_matches('\n');
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(
        parse_to_sexp("unterminated CRLF", &crlf),
        parse_to_sexp("unterminated LF", lf)
    );
}
