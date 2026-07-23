//! Example integration test.
//!
//! For every `.crn` under `examples/` at the repo root, parse it with the
//! tree-sitter grammar and assert the resulting syntax tree has no
//! `ERROR` node. This is the primary regression guardrail against the
//! reference parser: `cairn-lang-core` already accepts every file in
//! `examples/`, so the grammar must accept them too.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tree_sitter::Parser;

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .join("examples")
}

fn crn_files() -> Vec<PathBuf> {
    fs::read_dir(examples_dir())
        .expect("examples dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "crn"))
        .collect()
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

/// Regression guard for `queries/highlights.scm` and `queries/locals.scm`:
/// shells out to the locally installed `tree-sitter` CLI (a dev dependency
/// installed via `pnpm install`) and compares its `highlight` output for
/// `examples/cottage.crn` against the golden ANSI snapshot frozen in
/// `test/highlight/cottage.ansi`. A diff here means either the grammar or
/// the queries changed in a way that alters highlighting; regenerate the
/// golden deliberately if so.
#[test]
fn cottage_highlight_golden_is_stable() {
    let golden = include_str!("../test/highlight/cottage.ansi");

    let cli = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("node_modules")
        .join(".bin")
        .join(if cfg!(windows) {
            "tree-sitter.cmd"
        } else {
            "tree-sitter"
        });

    // The CLI is installed by `pnpm install` in this crate; the workspace-wide
    // `cargo test --workspace` in `.github/workflows/ci.yml` runs without a
    // Node setup, so the binary is absent there. Skip in that case — the
    // dedicated `tree-sitter.yml` workflow (which does `pnpm install`) is the
    // authoritative gate for highlight-golden drift.
    if !cli.exists() {
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
