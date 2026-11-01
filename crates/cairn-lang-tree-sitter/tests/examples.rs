//! The grammar over the shipped examples, every one of which `cairn-lang-core` accepts.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tree_sitter::Parser;

/// Set, it turns the highlight golden's skip on a missing `tree-sitter` CLI into a failure.
const REQUIRE_CLI: &str = "CAIRN_REQUIRE_TREE_SITTER_CLI";

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .join("examples")
}

/// How many `.crn` files `examples/` ships, so a sweep that finds fewer fails.
const SHIPPED_EXAMPLES: usize = 12;

/// Every `.crn` under `examples/`, sorted.
fn crn_files() -> Vec<PathBuf> {
    let dir = examples_dir();
    let mut found: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("examples dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "crn"))
        .collect();
    found.sort();
    assert_eq!(
        found.len(),
        SHIPPED_EXAMPLES,
        "found {} examples under {}, not the {SHIPPED_EXAMPLES} shipped; bump SHIPPED_EXAMPLES \
         when adding one",
        found.len(),
        dir.display(),
    );
    found
}

#[test]
fn all_examples_parse_without_error() {
    let mut parser = Parser::new();
    parser
        .set_language(&cairn_lang_tree_sitter::LANGUAGE.into())
        .expect("load cairn language");

    let mut failures = Vec::new();
    for path in crn_files() {
        let src = fs::read_to_string(&path).unwrap();
        let tree = parser.parse(&src, None).expect("parse produced no tree");
        if tree.root_node().has_error() {
            failures.push(format!("{}", path.display()));
        }
    }
    assert!(failures.is_empty(), "grammar rejected: {failures:#?}");
}

/// A comment line the scanner crosses as layout becomes no node, which no parity verdict shows.
#[test]
fn every_comment_line_in_the_corpus_is_a_node() {
    let mut parser = Parser::new();
    parser
        .set_language(&cairn_lang_tree_sitter::LANGUAGE.into())
        .expect("load cairn language");

    let mut missing = Vec::new();
    for path in crn_files() {
        let src = fs::read_to_string(&path).unwrap();
        let tree = parser.parse(&src, None).expect("parse produced no tree");
        let mut rows = Vec::new();
        comment_rows(tree.root_node(), &mut rows);
        for (row, line) in src.split('\n').enumerate() {
            if line.trim_start().starts_with('#') && !rows.contains(&row) {
                missing.push(format!(
                    "{}:{}: {}",
                    path.file_name().unwrap().to_string_lossy(),
                    row + 1,
                    line.trim(),
                ));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "comment lines the tree does not hold, so nothing highlights them:\n{}",
        missing.join("\n"),
    );
}

/// The row each `comment` node starts on.
fn comment_rows(node: tree_sitter::Node<'_>, out: &mut Vec<usize>) {
    if node.kind() == "comment" {
        out.push(node.start_position().row);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        comment_rows(child, out);
    }
}

/// The golden sits outside `test/highlight/`, where `tree-sitter test` would read it as corpus.
#[test]
fn cottage_highlight_golden_is_stable() {
    let golden = include_str!("data/cottage.ansi");

    let cli = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("node_modules")
        .join(".bin")
        .join(if cfg!(windows) {
            "tree-sitter.cmd"
        } else {
            "tree-sitter"
        });

    if !cli.exists() {
        assert!(
            std::env::var_os(REQUIRE_CLI).is_none_or(|v| v.is_empty()),
            "{REQUIRE_CLI} is set but the tree-sitter CLI is not at {} (run `pnpm install` in \
             this crate)",
            cli.display(),
        );
        eprintln!(
            "skip: tree-sitter CLI not found at {} (run `pnpm install` in this crate first)",
            cli.display()
        );
        return;
    }

    let output = Command::new(cli)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["highlight", "../../examples/cottage.crn"])
        .output()
        .expect("run tree-sitter CLI");

    assert!(output.status.success(), "cli failed: {output:?}");
    let rendered = String::from_utf8(output.stdout).expect("utf-8");

    assert_eq!(rendered, golden, "highlight output drifted from golden");
}
