//! No place-and-route `Fix:` line advises a second `circuit` line in a
//! scope.
//!
//! `check` refuses a second `circuit` line among a `struct`'s or `def`'s
//! own members as `E_DUPLICATE_CIRCUIT`, so the old advice to "split into
//! multiple `circuit` blocks" asks for a repair the compiler refuses. The
//! Fix lines say to split the logic across several scopes, each with its
//! own `circuit` line, instead. Most of them are reached only by a fixture
//! built to trip that one refusal, so a test per diagnostic would leave
//! any site without one unguarded; reading the crate's source reaches
//! every one, and any added later.
//!
//! The scan is textual, and it holds the phrase rather than the advice: a
//! Fix line that asked for the refused repair in other words would pass.

use std::path::{Path, PathBuf};

/// The phrase every form of the old advice carried.
const REFUSED_ADVICE: &str = "`circuit` block";

/// The advice that replaced it.
const SPLIT_ADVICE: &str =
    "split the logic across several scopes, each with its own `circuit` line";

/// Every `.rs` file under `dir`, recursively, sorted by path.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|error| panic!("cannot read an entry of {}: {error}", dir.display()))
            .path();
        if path.is_dir() {
            found.extend(rust_sources(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// `source` as one line of prose: each line's leading whitespace and
/// comment marker dropped, and lines joined with a space, or with nothing
/// after a string literal's `\` continuation, as the compiler joins them.
/// A phrase a line break splits, in a doc comment or a wrapped string,
/// reads whole here.
fn joined(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut continued = false;
    for line in source.lines() {
        let line = line.trim_start();
        let line = ["//!", "///", "//"]
            .iter()
            .find_map(|marker| line.strip_prefix(marker))
            .map_or(line, str::trim_start);
        if !continued && !out.is_empty() {
            out.push(' ');
        }
        let head = line.strip_suffix('\\');
        out.push_str(head.unwrap_or(line));
        continued = head.is_some();
    }
    out
}

#[test]
fn no_fix_line_advises_a_second_circuit_line_in_a_scope() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offending = Vec::new();
    let mut advised = 0;
    for path in rust_sources(&src) {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let text = joined(&source);
        advised += text.matches(SPLIT_ADVICE).count();
        for (at, _) in text.match_indices(REFUSED_ADVICE) {
            let from = text.floor_char_boundary(at.saturating_sub(60));
            let to = text.ceil_char_boundary(at + REFUSED_ADVICE.len() + 20);
            offending.push(format!("{}: …{}…", path.display(), &text[from..to]));
        }
    }
    // A scan that read nothing would pass; the files that carry the new
    // advice are the ones the old advice was in.
    assert!(
        advised > 0,
        "found no {SPLIT_ADVICE:?} under {}, so the scan read none of the Fix lines",
        src.display(),
    );
    assert!(
        offending.is_empty(),
        "advice to split into several `circuit` blocks, which `check` refuses as \
         `E_DUPLICATE_CIRCUIT` within one scope:\n{}",
        offending.join("\n"),
    );
}
