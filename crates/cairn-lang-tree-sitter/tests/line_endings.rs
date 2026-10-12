//! Line endings through the external scanner, and node positions against the reference lexer.
//! Not corpus cases, since `.gitattributes` normalises a fixture's `\r\n` to `\n`.

use std::collections::{BTreeMap, BTreeSet};

use tree_sitter::Parser;

/// Crosses both of the scanner's line-break paths, and ends a line in a comment.
const LF_SOURCE: &str = concat!(
    "@cairn 2026.06\n",
    "struct keep size=5x5\n",
    "  floor mat_slot=floor\n",
    "\n",
    "# a comment on its own line\n",
    "  level id=l0 y=0\n",
    "    door id=entry side=front  # eol note\n",
);

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

#[test]
fn crlf_source_parses_identically_to_lf() {
    let crlf = LF_SOURCE.replace('\n', "\r\n");
    assert_eq!(parse_to_sexp("CRLF", &crlf), parse_to_sexp("LF", LF_SOURCE));
}

/// A column mismeasured after a lone `\r` can nest a member too deep without any parse error.
#[test]
fn lone_cr_source_parses_identically_to_lf() {
    let cr = LF_SOURCE.replace('\n', "\r");
    assert_eq!(
        parse_to_sexp("lone CR", &cr),
        parse_to_sexp("LF", LF_SOURCE)
    );
}

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
// An editor compares a node's `Point` with the line numbers `cairn check` prints.

/// Each leaf's 1-based row and column, keyed by start byte, since the two tokenisers differ.
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

/// The same from the reference lexer, without whitespace-only tokens, which have no node.
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

/// A byte offset both implementations start a token at, and where each says it is.
struct SharedPosition {
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
        // Lines 4 and 5 are the blank and the comment-only line.
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

/// The runtime advances `Point.row` on `\n` alone, and no scanner can move it (`spec/syntax`
/// "Literals and separators"). Once a runtime does, delete this and test lone `\r` above.
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
    let reference: BTreeSet<u32> = core_positions(&cr)
        .into_values()
        .map(|(line, _)| line)
        .collect();
    assert!(reference.len() > 1, "{reference:?}");
}

/// `Point.column` counts bytes and a `Position` characters, so the parity above needs ASCII.
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
    for shared in shared.iter().filter(|s| s.offset < id) {
        assert_eq!(
            shared.reference, shared.node,
            "byte {} is before the label and should agree",
            shared.offset,
        );
    }
}
