//! `unsafe_code` is lifted in exactly one place: the tree-sitter binding's
//! FFI module.
//!
//! The workspace sets the lint to `deny`, not `forbid`, because `forbid`
//! refuses every inner `allow` or `expect` — including the one the generated
//! C parser needs to be reachable from Rust at all. `deny` lets any file say
//! `#[allow(unsafe_code)]`, and a crate manifest with its own `[lints.rust]`
//! table can drop the workspace policy wholesale, since Cargo replaces rather
//! than merges it. This test takes back what `forbid` promised:
//!
//! 1. Every member crate inherits the workspace lints (`[lints]` with
//!    `workspace = true`) and writes no `[lints.*]` table of its own.
//! 2. No Rust file under `crates/` names `unsafe_code` outside a comment
//!    except [`ALLOWED`] (and this test).
//!
//! It lives in `cairn-lang-core` for want of a workspace-level test target,
//! and returns early when run from a packaged crate, which carries no
//! workspace to read.

use std::fs;
use std::path::{Path, PathBuf};

/// The single file allowed to lift `unsafe_code`, relative to the repository
/// root.
const ALLOWED: &str = "crates/cairn-lang-tree-sitter/bindings/rust/lib.rs";

/// This test, which names the lint in its own strings.
const THIS_FILE: &str = "crates/cairn-lang-core/tests/unsafe_code_is_confined.rs";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two directories below the repository root")
        .to_path_buf()
}

/// `None` outside the workspace (a packaged crate unpacked into a registry
/// directory), where there is nothing to check.
fn workspace_manifest(root: &Path) -> Option<String> {
    fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .filter(|manifest| {
            manifest
                .lines()
                .any(|line| line.trim_end() == "[workspace]")
        })
}

/// Member crate directories, read from the `members` array.
fn members(manifest: &str) -> Vec<String> {
    let start = manifest
        .find("members = [")
        .expect("the workspace manifest lists its members");
    let list = &manifest[start..];
    let list = &list[..list.find(']').expect("the members array is closed")];
    list.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_member_inherits_the_workspace_lints() {
    let root = repo_root();
    let Some(manifest) = workspace_manifest(&root) else {
        return;
    };
    let members = members(&manifest);
    assert!(
        !members.is_empty(),
        "no members read from the workspace manifest"
    );

    let mut problems = Vec::new();
    for member in &members {
        let path = root.join(member).join("Cargo.toml");
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        if lines.iter().any(|line| line.starts_with("[lints.")) {
            problems.push(format!("{member}: writes its own [lints.*] table"));
        }
        let inherits = lines
            .iter()
            .position(|line| *line == "[lints]")
            .and_then(|at| lines.get(at + 1))
            .is_some_and(|next| *next == "workspace = true");
        if !inherits {
            problems.push(format!("{member}: has no `[lints]` / `workspace = true`"));
        }
    }
    assert!(
        problems.is_empty(),
        "a crate that does not inherit [workspace.lints] silently drops every lint \
         added there later:\n  {}",
        problems.join("\n  ")
    );
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn unsafe_code_is_lifted_only_by_the_tree_sitter_ffi() {
    let root = repo_root();
    if workspace_manifest(&root).is_none() {
        return;
    }
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(
        files.iter().any(|f| f.ends_with(ALLOWED)),
        "{ALLOWED} was not found; if the FFI moved, move ALLOWED with it"
    );

    let mut hits = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        if rel == Path::new(ALLOWED) || rel == Path::new(THIS_FILE) {
            continue;
        }
        let text =
            fs::read_to_string(file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        for (n, line) in text.lines().enumerate() {
            // Any mention outside a comment, not just a one-line `#[...]`:
            // an attribute split across lines names the lint on a line of
            // its own.
            let code = line.trim_start();
            if !code.starts_with("//") && code.contains("unsafe_code") {
                hits.push(format!("{}:{}: {}", rel.display(), n + 1, code));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "only {ALLOWED} may lift `unsafe_code`:\n  {}",
        hits.join("\n  ")
    );
}
