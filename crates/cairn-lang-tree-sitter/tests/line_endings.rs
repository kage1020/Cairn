//! Line-ending regression test for the external scanner.
//!
//! `src/scanner.c` accepts `\r\n` as well as `\n` on both of its line-break
//! paths: the NEWLINE branch, and the blank/comment-only-line skip loop that
//! precedes indent counting. Nothing else pins that behaviour — the corpus
//! under `test/corpus/` cannot, because the repository's `.gitattributes`
//! normalizes every text file to LF on commit, so a `\r\n` written into a
//! corpus fixture would silently become `\n` and stop testing anything. The
//! source is therefore built here, in Rust, where the bytes are literal.

use std::collections::{BTreeMap, BTreeSet};

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
/// count has to be taken afresh on the line it lands on, since the column
/// cannot supply it.
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

// -- node positions -------------------------------------------------------
//
// The tree being identical is not the same claim as the *positions* being
// right. An editor that drives highlighting from this grammar reads
// `Point.row` / `Point.column`, and compares them against the line numbers
// `cairn check` prints from `cairn-lang-core`. Those two have to name the
// same place.

/// Where each leaf starts, keyed by byte offset: the 1-based row and column
/// tree-sitter reports for it.
///
/// Keyed by offset rather than compared position-by-position because the two
/// implementations do not agree on token *boundaries* — the grammar splits
/// `5x5` into two integers around a literal `x` where the lexer produces one
/// `Size`, and `2026.06` is one `directive_literal` against the lexer's
/// `Int` / `Dot` / `Int`. Those are tokenisation differences, not position
/// defects; the claim under test is that wherever both name the same byte,
/// they name the same place in the file.
fn leaf_positions(source: &str) -> BTreeMap<usize, (u32, u32)> {
    let mut parser = Parser::new();
    parser
        .set_language(&cairn_lang_tree_sitter::LANGUAGE.into())
        .expect("load cairn language");
    let tree = parser.parse(source, None).expect("parse produced no tree");
    let mut cursor = tree.walk();
    let mut leaves = BTreeMap::new();
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        if node.child_count() == 0 {
            let point = node.start_position();
            leaves.insert(
                node.start_byte(),
                (
                    u32::try_from(point.row).expect("row fits u32") + 1,
                    u32::try_from(point.column).expect("column fits u32") + 1,
                ),
            );
        } else {
            pending.extend(node.children(&mut cursor));
        }
    }
    leaves
}

/// The same, from the reference lexer.
///
/// `Point.column` counts bytes and a `Position` counts characters, so the
/// two agree on columns only while the line is ASCII —
/// `a_non_ascii_line_shifts_every_column_after_it` below pins where they
/// stop. `LF_SOURCE` is ASCII, so the comparisons here are of line numbers
/// and of columns alike.
///
/// Tokens whose text is only whitespace are dropped: the synthetic
/// `Indent` / `Dedent` / `Newline` have no node to be compared against, and
/// since the fix a `Newline` spans its terminator rather than nothing, so
/// "has an empty span" would no longer select them.
fn core_positions(source: &str) -> BTreeMap<usize, (u32, u32)> {
    cairn_lang_core::lex::lex(source)
        .expect("the reference lexer accepts the fixture")
        .into_iter()
        .filter(|token| !source[token.span.clone()].trim().is_empty())
        .map(|token| {
            (
                token.span.start,
                (token.position.line.get(), token.position.col.get()),
            )
        })
        .collect()
}

/// One byte offset both implementations start a token at, and where each
/// says that offset is.
struct SharedPosition {
    /// Byte offset into the source — the key the two are joined on.
    offset: usize,
    /// 1-based line and column from the reference lexer.
    reference: (u32, u32),
    /// 1-based row and column from the tree-sitter node.
    node: (u32, u32),
}

/// Join the two implementations on the offsets they both name.
fn shared_positions(source: &str) -> Vec<SharedPosition> {
    let nodes = leaf_positions(source);
    core_positions(source)
        .into_iter()
        .filter_map(|(offset, reference)| {
            nodes.get(&offset).map(|node| SharedPosition {
                offset,
                reference,
                node: *node,
            })
        })
        .collect()
}

#[test]
fn node_positions_match_the_reference_lexer_for_lf_and_crlf() {
    for (label, source) in [
        ("LF", LF_SOURCE.to_owned()),
        ("CRLF", LF_SOURCE.replace('\n', "\r\n")),
    ] {
        let shared = shared_positions(&source);
        // Every line of the fixture that holds a token has to be
        // represented, or a comparison that only ever saw line 1 would pass
        // on a broken row counter. Lines 4 and 5 are the blank and the
        // comment-only line; neither produces one.
        let rows: BTreeSet<u32> = shared.iter().map(|s| s.reference.0).collect();
        assert_eq!(
            rows,
            BTreeSet::from([1, 2, 3, 6, 7]),
            "{label}: the wrong lines were compared",
        );
        for shared in shared {
            assert_eq!(
                shared.reference, shared.node,
                "{label}: byte {}",
                shared.offset,
            );
        }
    }
}

/// A lone `\r` is where their *lines* part, and the grammar cannot close
/// the gap.
///
/// `Point.row` is maintained by the tree-sitter *runtime*'s lexer, which
/// advances it on `\n` and nothing else; an external scanner is handed
/// `advance` and `get_column`, not the point. So a CR-only file parses to
/// the right tree — `lone_cr_source_parses_identically_to_lf` above — while
/// every node stays on the first row and its column keeps climbing across
/// what should have been a line break.
///
/// Pinned rather than left implicit: if a future runtime widens its line
/// rule this test fails, which is the signal to delete it and fold the CR
/// case into the parity test above.
#[test]
fn a_lone_carriage_return_leaves_every_node_on_the_first_row() {
    let cr = LF_SOURCE.replace('\n', "\r");
    let nodes = leaf_positions(&cr);
    let rows: BTreeSet<u32> = nodes.values().map(|(row, _)| *row).collect();
    assert_eq!(
        rows,
        BTreeSet::from([1]),
        "the runtime advanced a row on a lone `\\r`; the divergence this pins is gone",
    );
    // The column keeps climbing rather than restarting, which is the other
    // half of the same defect and the half a row check cannot see: the last
    // token of the file is reported further right than the file is wide.
    let widest = nodes.values().map(|(_, col)| *col).max().expect("nodes");
    let longest_line = LF_SOURCE
        .lines()
        .map(|line| u32::try_from(line.chars().count()).expect("fits u32"))
        .max()
        .expect("lines");
    assert!(
        widest > longest_line,
        "columns restarted somewhere: widest {widest} against a longest line of {longest_line}",
    );
    // And the reference lexer does place them on separate lines, so the
    // two really are describing the same file differently.
    let reference: BTreeSet<u32> = core_positions(&cr)
        .into_values()
        .map(|(line, _)| line)
        .collect();
    assert!(reference.len() > 1, "{reference:?}");
}

/// And a non-ASCII character is where their *columns* part.
///
/// `Point.column` is a byte count; a `Position` column is a character
/// count. Neither is wrong — tree-sitter's API is byte-addressed
/// throughout, and a consumer converts — but the parity test above holds
/// only because its fixture is ASCII, and a Japanese label in a `.crn` file
/// is an ordinary thing to write in this project. Pinned so that claim
/// cannot quietly widen.
#[test]
fn a_non_ascii_line_shifts_every_column_after_it() {
    let source = "struct s size=3x3 label=\"日本\" id=x\n";
    let shared = shared_positions(source);
    let id = source.find("id=").expect("fixture holds `id=`");
    let entry = shared
        .iter()
        .find(|s| s.offset == id)
        .expect("both name the `id` token");
    // Two three-byte characters where the lexer counted two: four columns.
    assert_eq!(entry.node.1 - entry.reference.1, 4);
    // Everything before the label still agrees, so the shift is the label's
    // and not a whole-line offset.
    for shared in shared.iter().filter(|s| s.offset < id) {
        assert_eq!(
            shared.reference, shared.node,
            "byte {} is before the label and should agree",
            shared.offset,
        );
    }
}
