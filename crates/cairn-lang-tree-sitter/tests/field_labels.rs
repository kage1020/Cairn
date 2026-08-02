//! Field-label coverage guard for the corpus.
//!
//! The tree-sitter CLI strips field labels from the parsed tree whenever the
//! expected S-expression carries none, and dropping a `field(...)` from a
//! grammar rule leaves the tree *shape* untouched. So a corpus written without
//! labels keeps passing even after `field('name', $.identifier)` decays to
//! `$.identifier` — and the labels are consumed: in-repo by
//! `queries/highlights.scm` and `queries/locals.scm`, and downstream by
//! anything calling `child_by_field_name` on the published grammar.
//! `test/corpus/` therefore keeps a `(field labels)` case per field-bearing
//! rule, whose expected tree spells the labels out.
//!
//! Which rules those are was otherwise maintained entirely by hand, so a rule
//! that gained a field gained no test with it. This test holds the two sides
//! equal: the set of `(node, field)` pairs the generated `src/node-types.json`
//! declares must be exactly the set labelled somewhere in the corpus.
//!
//! Both directions matter, and they catch different regressions:
//!
//! - A declared pair with no label is a rule whose `field(...)` could be
//!   deleted without failing anything.
//! - A labelled pair that is no longer declared is that deletion having
//!   happened. `tree-sitter test` catches it too, but only in the
//!   `tree-sitter.yml` workflow behind a paths filter; this side of the
//!   assertion puts it in `cargo test --workspace`, which runs on every PR.
//!
//! Division of labour with `tree-sitter test`: labelling is all-or-nothing per
//! case (one label anywhere sets the CLI's `has_fields` flag and the whole tree
//! is then compared with labels), so the CLI already rejects a case whose
//! labels are wrong or misplaced. This test asserts *which* labels exist, never
//! that a tree is shaped correctly.
//!
//! Known limitation: `node-types.json` is per node, not per grammar branch. A
//! rule that declares the same field in several `choice` branches is satisfied
//! here by any one of them, because the branches are indistinguishable in the
//! S-expression — `directive` declares `name` and `arg` once per directive
//! keyword and all three produce `(directive name: ... arg: ...)`;
//! `binary_expression` declares `lhs` and `rhs` once for `or` and once for
//! `and`. The corpus labels every such branch anyway — `directive` across one
//! case per keyword, `binary_expression` within the single case whose
//! expression nests an `or` inside an `and` — but nothing here enforces it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Node names tree-sitter uses for parse failures. Labels below one of these
/// describe error recovery, not the grammar, so they pin nothing.
const ERROR_NODES: [&str; 3] = ["ERROR", "MISSING", "UNEXPECTED"];

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

/// Every file under `test/corpus/`, recursively.
///
/// `tree-sitter test` walks that directory and does not care about extensions,
/// so neither does this — a case moved into a subdirectory or saved without
/// `.txt` must not fall out of the ledger unnoticed.
fn corpus_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, into: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read corpus dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, into);
            } else {
                into.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(&crate_dir().join("test/corpus"), &mut files);
    files.sort();
    files
}

/// One corpus case.
struct Case {
    title: String,
    /// `:skip`, `:error`, `:platform(...)`, ... — the attribute lines the CLI
    /// allows between the test name and the closing rule.
    markers: Vec<String>,
    /// Text after the `---` divider. Empty when the case has none.
    tree: String,
}

/// Whether a line opens or closes a corpus section.
///
/// The CLI's header and divider patterns are `^={3,}` and `^-{3,}` with an
/// optional trailing suffix (`====== marker`, `--- note`), not a line of one
/// repeated glyph, so this matches on the leading run only. Requiring the whole
/// line to be uniform would run the divider search past the end of its case and
/// hang the next case's tree off the wrong title.
fn is_rule(line: &str, glyph: char) -> bool {
    line.chars().take_while(|c| *c == glyph).count() >= 3
}

/// Split one corpus file into cases.
///
/// The format is an `=` rule, the test name, zero or more attribute lines,
/// another `=` rule, the input, a `-` divider, then the expected tree, running
/// until the next case's opening rule. Only the expected side is kept: Cairn
/// source has parentheses of its own (`truth(a -> b)`) and trailing colons of
/// its own (`theme t:`), both of which would read as phantom field labels.
fn parse_cases(text: &str) -> Vec<Case> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut cases = Vec::new();

    let mut i = 0;
    while i < lines.len() {
        if !is_rule(lines[i], '=') {
            i += 1;
            continue;
        }

        // The header runs to the next `=` rule; everything between is the test
        // name followed by its attribute lines.
        let mut close = i + 1;
        while close < lines.len() && !is_rule(lines[close], '=') {
            close += 1;
        }
        assert!(
            close < lines.len(),
            "corpus case header starting `{}` is never closed",
            lines[i]
        );
        assert!(close > i + 1, "corpus case header has no test name");

        let title = lines[i + 1].to_owned();
        let markers: Vec<String> = lines[i + 2..close]
            .iter()
            .filter(|line| line.starts_with(':'))
            .map(|line| (*line).to_owned())
            .collect();

        // The body ends where the next case's opening rule begins.
        let body_start = close + 1;
        let mut body_end = body_start;
        while body_end < lines.len() && !is_rule(lines[body_end], '=') {
            body_end += 1;
        }

        let divider = (body_start..body_end).find(|n| is_rule(lines[*n], '-'));
        // A case that asserts a parse failure carries no expected tree. Anything
        // else without a divider is malformed.
        assert!(
            divider.is_some() || markers.iter().any(|m| m.starts_with(":error")),
            "corpus case `{title}` has no `---` divider"
        );
        let tree = divider.map_or_else(String::new, |n| lines[n + 1..body_end].join("\n"));

        cases.push(Case {
            title,
            markers,
            tree,
        });
        i = body_end;
    }

    cases
}

/// Record every `(enclosing node, field)` pair labelled in one expected tree.
///
/// Tokens are `(`, `)`, quoted anonymous tokens (`'\t'`, `";"`), and bare
/// words; a word ending in `:` is a field label belonging to the node on top of
/// the stack, and a word right after `(` names the node just opened. Every `(`
/// pushes, named or not, so that a quoted token in node position cannot leave
/// the following `)` popping the *parent* and re-parenting the labels after it.
fn collect_labelled_fields(tree: &str, into: &mut BTreeSet<(String, String)>) {
    let bytes = tree.as_bytes();
    let mut stack: Vec<&str> = Vec::new();
    let mut open_error_nodes = 0usize;
    let mut at_node_name = false;

    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => {
                stack.push("");
                at_node_name = true;
                i += 1;
            }
            b')' => {
                let closed = stack.pop().expect("unbalanced `)` in an expected tree");
                if ERROR_NODES.contains(&closed) {
                    open_error_nodes -= 1;
                }
                at_node_name = false;
                i += 1;
            }
            quote @ (b'\'' | b'"') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    // A backslash escapes the next byte, so `'\''` does not end
                    // here. The corpus reaches this through `(UNEXPECTED '\t')`.
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
                    assert!(
                        !node.is_empty(),
                        "field label `{word}` before its node name"
                    );
                    if open_error_nodes == 0 {
                        into.insert(((*node).to_owned(), field.to_owned()));
                    }
                } else if at_node_name {
                    *stack.last_mut().expect("a node was opened") = word;
                    if ERROR_NODES.contains(&word) {
                        open_error_nodes += 1;
                    }
                }
                at_node_name = false;
            }
        }
    }

    assert!(stack.is_empty(), "unbalanced `(` in an expected tree");
}

#[test]
fn corpus_labels_match_the_fields_the_grammar_declares() {
    let declared = declared_fields();
    assert!(
        !declared.is_empty(),
        "node-types.json declared no fields at all — is it stale?"
    );

    let mut labelled = BTreeSet::new();
    let mut uncounted = Vec::new();
    let mut cases = 0usize;
    let mut rules = 0usize;

    for path in corpus_files() {
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        rules += text
            .lines()
            .filter(|line| is_rule(line.trim_end(), '='))
            .count();

        for case in parse_cases(&text) {
            cases += 1;
            // Every marker either skips the case outright or narrows the
            // platforms and languages it runs on, so its labels cannot be
            // relied on to pin anything.
            if case.markers.is_empty() {
                collect_labelled_fields(&case.tree, &mut labelled);
            } else {
                uncounted.push(format!("{} {}", case.title, case.markers.join(" ")));
            }
        }
    }

    // Each case is delimited by exactly two `=` rules. If that does not add up,
    // a case was silently dropped and the coverage below is measured against a
    // corpus smaller than the one the CLI runs.
    assert_eq!(
        rules,
        cases * 2,
        "found {rules} `===` rules but only {cases} corpus cases — a case was not recognised"
    );

    let unpinned: Vec<String> = declared
        .difference(&labelled)
        .map(|(node, field)| format!("{node}.{field}"))
        .collect();
    let stale: Vec<String> = labelled
        .difference(&declared)
        .map(|(node, field)| format!("{node}.{field}"))
        .collect();

    assert!(
        unpinned.is_empty(),
        "the grammar declares these fields but no corpus case labels them, so removing the \
         `field(...)` would leave the corpus green: {unpinned:#?}\n\
         Fix: pick a case whose input makes the field present, mark its title `(field labels)`, \
         and write out every field label in its expected tree — labelling is all-or-nothing per \
         case, so a partially labelled tree fails.\n\
         Cases excluded from the count because they carry markers: {uncounted:#?}"
    );
    assert!(
        stale.is_empty(),
        "the corpus labels these fields but the grammar no longer declares them: {stale:#?}\n\
         Either a `field(...)` was dropped from grammar.js — restore it — or the removal was \
         deliberate, in which case drop the labels too."
    );
}

#[test]
fn attribute_lines_do_not_hide_a_case() {
    let cases = parse_cases(
        "===\nWith markers\n:platform(linux)\n:fail_fast\n===\n\nin\n\n---\n\n(a b: (c))\n",
    );
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].title, "With markers");
    assert_eq!(cases[0].markers, [":platform(linux)", ":fail_fast"]);
    assert_eq!(cases[0].tree.trim(), "(a b: (c))");
}

#[test]
fn error_cases_may_omit_the_divider() {
    let cases = parse_cases("===\nRejected\n:error\n===\n\nin\n");
    assert_eq!(cases.len(), 1);
    assert!(cases[0].tree.is_empty());
}

#[test]
#[should_panic(expected = "has no `---` divider")]
fn a_plain_case_may_not_omit_the_divider() {
    parse_cases("===\nNo divider\n===\n\nin\n");
}

#[test]
fn rules_and_dividers_may_carry_a_suffix() {
    let cases = parse_cases(
        "====== one\nFirst\n====== one\n\nin\n\n--- note\n\n(a b: (c))\n\
         \n===\nSecond\n===\n\nin\n\n---\n\n(d e: (f))\n",
    );
    let titles: Vec<&str> = cases.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, ["First", "Second"]);
    assert_eq!(cases[0].tree.trim(), "(a b: (c))");
    assert_eq!(cases[1].tree.trim(), "(d e: (f))");
}

#[test]
fn labels_under_an_error_node_pin_nothing() {
    let mut found = BTreeSet::new();
    collect_labelled_fields(
        "(source_file\n  (struct_decl name: (identifier)\n    (ERROR keyword: (identifier))))",
        &mut found,
    );
    assert_eq!(
        found,
        BTreeSet::from([("struct_decl".to_owned(), "name".to_owned())])
    );
}

#[test]
fn quoted_tokens_do_not_shift_the_stack() {
    let mut found = BTreeSet::new();
    collect_labelled_fields(
        "(binary_expression lhs: (a) \"and\" (UNEXPECTED '\\t') rhs: (b))",
        &mut found,
    );
    assert_eq!(
        found,
        BTreeSet::from([
            ("binary_expression".to_owned(), "lhs".to_owned()),
            ("binary_expression".to_owned(), "rhs".to_owned()),
        ])
    );
}
