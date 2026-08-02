//! Field-label coverage guard for the corpus.
//!
//! The tree-sitter CLI strips field labels from the parsed tree whenever the
//! expected S-expression carries none, and dropping a `field(...)` from a
//! grammar rule leaves the tree *shape* untouched. So a corpus written without
//! labels keeps passing even after `field('name', $.identifier)` decays to
//! `$.identifier` — and the labels are public API: `queries/highlights.scm`
//! and `queries/locals.scm` match on them, as does every `child_by_field_name`
//! caller on the Rust and Node side. `test/corpus/` therefore keeps one
//! `(field labels)` case per field-bearing rule, whose expected tree spells the
//! labels out.
//!
//! Which rules those are is otherwise maintained entirely by hand, so a rule
//! that gains a field later gains no test with it. This test closes that gap
//! mechanically: every `(node, field)` pair the generated `src/node-types.json`
//! declares must appear as a label under that node in some expected tree.
//!
//! Division of labour with `tree-sitter test`: labelling is all-or-nothing per
//! case (one label anywhere sets the CLI's `has_fields` flag and the whole tree
//! is then compared with labels), so the CLI already rejects a case whose
//! labels are wrong, misplaced, or incomplete. This test only asserts they are
//! *present* somewhere — never that they are correct.
//!
//! Known limitation: `node-types.json` is per node, not per grammar branch. A
//! rule that declares the same field in several `choice` branches — `directive`
//! declares `name` and `arg` once per directive keyword — is satisfied here by
//! a single branch, because the branches are indistinguishable in the
//! S-expression (`@cairn` and `@requires` both produce
//! `(directive name: (directive_name) arg: (version_expr ...))`). The corpus
//! keeps a labelled case per `directive` branch for that reason; nothing here
//! enforces it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Every `(node, field)` pair the generated grammar declares.
fn declared_fields() -> BTreeSet<(String, String)> {
    let src =
        fs::read_to_string(crate_dir().join("src/node-types.json")).expect("read node-types.json");
    let node_types: serde_json::Value =
        serde_json::from_str(&src).expect("node-types.json is JSON");

    let mut declared = BTreeSet::new();
    for node in node_types
        .as_array()
        .expect("node-types.json holds an array")
    {
        let name = node["type"].as_str().expect("node type name");
        let Some(fields) = node.get("fields").and_then(serde_json::Value::as_object) else {
            continue;
        };
        for field in fields.keys() {
            declared.insert((name.to_owned(), field.clone()));
        }
    }
    declared
}

fn corpus_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(crate_dir().join("test/corpus"))
        .expect("read corpus dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
        .collect();
    files.sort();
    files
}

fn is_rule_of(line: &str, glyph: char) -> bool {
    line.len() >= 3 && line.chars().all(|c| c == glyph)
}

/// Pull the expected S-expression out of every case in one corpus file.
///
/// The corpus format is a `=` rule, a title, another `=` rule, the input, a
/// `-` divider, then the expected tree, running until the next case's opening
/// rule. Only the expected trees are returned: Cairn source has its own
/// parentheses (`truth(a -> b)`) and its own trailing colons (`theme t:`), so
/// scanning the input side would report phantom field labels.
fn expected_trees(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut trees = Vec::new();

    let mut i = 0;
    while i < lines.len() {
        let is_case_header =
            is_rule_of(lines[i], '=') && i + 2 < lines.len() && is_rule_of(lines[i + 2], '=');
        if !is_case_header {
            i += 1;
            continue;
        }
        let title = lines[i + 1];
        i += 3;

        while i < lines.len() && !is_rule_of(lines[i], '-') {
            i += 1;
        }
        assert!(
            i < lines.len(),
            "corpus case `{title}` has no `---` divider"
        );
        i += 1;

        let start = i;
        while i < lines.len() && !is_rule_of(lines[i], '=') {
            i += 1;
        }
        trees.push(lines[start..i].join("\n"));
    }
    trees
}

/// Record every `(enclosing node, field)` pair labelled in one expected tree.
///
/// Tokens are `(`, `)`, quoted anonymous tokens (`'\t'`, `";"`), and bare
/// words; a word ending in `:` is a field label and belongs to the node on top
/// of the stack, a word right after `(` is a node name.
fn collect_labelled_fields(tree: &str, into: &mut BTreeSet<(String, String)>) {
    let bytes = tree.as_bytes();
    let mut stack: Vec<&str> = Vec::new();
    let mut at_node_name = false;

    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => {
                at_node_name = true;
                i += 1;
            }
            b')' => {
                stack.pop();
                at_node_name = false;
                i += 1;
            }
            quote @ (b'\'' | b'"') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += usize::from(bytes[i] == b'\\') + 1;
                }
                i += 1;
                at_node_name = false;
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && !matches!(bytes[i], b'(' | b')')
                {
                    i += 1;
                }
                let word = &tree[start..i];
                if let Some(field) = word.strip_suffix(':') {
                    let node = stack
                        .last()
                        .unwrap_or_else(|| panic!("field label `{word}` outside any node"));
                    into.insert(((*node).to_owned(), field.to_owned()));
                } else if at_node_name {
                    stack.push(word);
                }
                at_node_name = false;
            }
        }
    }
}

#[test]
fn every_declared_field_is_pinned_by_a_corpus_case() {
    let declared = declared_fields();
    assert!(
        !declared.is_empty(),
        "node-types.json declared no fields at all — is it stale?"
    );

    let mut labelled = BTreeSet::new();
    for path in corpus_files() {
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for tree in expected_trees(&text) {
            collect_labelled_fields(&tree, &mut labelled);
        }
    }

    let unpinned: Vec<String> = declared
        .difference(&labelled)
        .map(|(node, field)| format!("{node}.{field}"))
        .collect();

    assert!(
        unpinned.is_empty(),
        "the grammar declares these fields but no corpus case pins them with a label, so \
         removing the `field(...)` would not fail any test: {unpinned:#?}\n\
         Fix: pick a case whose input makes the field present, mark its title `(field labels)`, \
         and write out every field label in its expected tree — labelling is all-or-nothing per \
         case, so a partially labelled tree fails."
    );
}
