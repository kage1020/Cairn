//! Every field the tree-sitter grammar declares is labelled in some corpus case, and nothing else
//! is: dropping a `field(...)` leaves tree shapes alone, so an unlabelled corpus would not notice.

use std::collections::BTreeSet;
use std::path::Path;

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "grammar-fields",
    summary: "the tree-sitter corpus labels exactly the fields the grammar declares",
    check,
};

const GRAMMAR: &str = "crates/cairn-lang-tree-sitter";

/// Node names for parse failures; labels under them describe recovery, not the grammar.
const ERROR_NODES: [&str; 3] = ["ERROR", "MISSING", "UNEXPECTED"];

type Pairs = BTreeSet<(String, String)>;

fn declared_fields(node_types: &str) -> Result<Pairs, String> {
    let nodes: serde_json::Value = serde_json::from_str(node_types)
        .map_err(|error| format!("node-types.json is not JSON: {error}"))?;
    let mut declared = BTreeSet::new();
    for node in nodes.as_array().ok_or("node-types.json is not an array")? {
        let name = node["type"].as_str().ok_or("a node type has no name")?;
        for field in node
            .get("fields")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flat_map(serde_json::Map::keys)
        {
            declared.insert((name.to_owned(), field.clone()));
        }
    }
    Ok(declared)
}

struct Case {
    title: String,
    /// The header's attribute lines, such as `:skip`.
    markers: Vec<String>,
    /// The expected tree; empty when the case has none.
    tree: String,
}

/// Whether `line` is a header rule or divider: three or more of `glyph`, then anything.
fn is_rule(line: &str, glyph: char) -> bool {
    line.chars().take_while(|c| *c == glyph).count() >= 3
}

/// The cases of one corpus file, keeping only the expected tree: Cairn source has its own `(`.
fn parse_cases(text: &str) -> Result<Vec<Case>, String> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut cases = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !is_rule(lines[i], '=') {
            i += 1;
            continue;
        }
        let close = (i + 1..lines.len())
            .find(|&n| is_rule(lines[n], '='))
            .ok_or_else(|| format!("the case header at line {} is never closed", i + 1))?;
        if close == i + 1 {
            return Err(format!("the case header at line {} has no name", i + 1));
        }
        let title = lines[i + 1].to_owned();
        let markers = lines[i + 2..close]
            .iter()
            .filter(|line| line.starts_with(':'))
            .map(|line| (*line).to_owned())
            .collect::<Vec<_>>();
        let body_end = (close + 1..lines.len())
            .find(|&n| is_rule(lines[n], '='))
            .unwrap_or(lines.len());
        let divider = (close + 1..body_end).find(|&n| is_rule(lines[n], '-'));
        if divider.is_none() && !markers.iter().any(|marker| marker.starts_with(":error")) {
            return Err(format!("case `{title}` has no `---` divider"));
        }
        let tree = divider.map_or_else(String::new, |n| lines[n + 1..body_end].join("\n"));
        cases.push(Case {
            title,
            markers,
            tree,
        });
        i = body_end;
    }
    Ok(cases)
}

/// Records every `(node, field)` labelled in `tree`. Every `(` pushes, named or not, so a quoted
/// token in node position cannot make its `)` pop the parent.
fn collect_labels(tree: &str, into: &mut Pairs) -> Result<(), String> {
    let bytes = tree.as_bytes();
    let mut stack: Vec<&str> = Vec::new();
    let mut inside_error = 0usize;
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
                let closed = stack.pop().ok_or("unbalanced `)` in an expected tree")?;
                if ERROR_NODES.contains(&closed) {
                    inside_error -= 1;
                }
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
                        .filter(|node| !node.is_empty())
                        .ok_or_else(|| format!("field label `{word}` outside a named node"))?;
                    if inside_error == 0 {
                        into.insert(((*node).to_owned(), field.to_owned()));
                    }
                } else if at_node_name {
                    if let Some(top) = stack.last_mut() {
                        *top = word;
                    }
                    if ERROR_NODES.contains(&word) {
                        inside_error += 1;
                    }
                }
                at_node_name = false;
            }
        }
    }
    if stack.is_empty() {
        Ok(())
    } else {
        Err("unbalanced `(` in an expected tree".into())
    }
}

fn check(repo: &Repo) -> Vec<Finding> {
    let grammar = Path::new(GRAMMAR);
    let node_types = grammar.join("src/node-types.json");
    let declared = match repo
        .read(&node_types)
        .and_then(|text| declared_fields(&text))
    {
        Ok(declared) if !declared.is_empty() => declared,
        Ok(_) => {
            return vec![Finding::new(&node_types, None, "declares no fields")];
        }
        Err(error) => return vec![Finding::new(&node_types, None, error)],
    };

    let mut findings = Vec::new();
    let mut labelled = BTreeSet::new();
    let corpus = grammar.join("test/corpus");
    for path in repo.files().iter().filter(|path| path.starts_with(&corpus)) {
        let cases = match repo.read(path).and_then(|text| parse_cases(&text)) {
            Ok(cases) => cases,
            Err(error) => {
                findings.push(Finding::new(path, None, error));
                continue;
            }
        };
        // A marker skips a case or narrows where it runs, so its labels pin nothing.
        for case in cases.iter().filter(|case| case.markers.is_empty()) {
            if let Err(error) = collect_labels(&case.tree, &mut labelled) {
                findings.push(Finding::new(
                    path,
                    None,
                    format!("case `{}`: {error}", case.title),
                ));
            }
        }
    }

    for (node, field) in declared.difference(&labelled) {
        findings.push(Finding::new(
            &corpus,
            None,
            format!(
                "`{node}.{field}` is declared but no unmarked corpus case labels it; label every \
                 field in a `(field labels)` case whose input produces it"
            ),
        ));
    }
    for (node, field) in labelled.difference(&declared) {
        findings.push(Finding::new(
            &node_types,
            None,
            format!(
                "`{node}.{field}` is labelled in the corpus but the grammar no longer declares it"
            ),
        ));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_and_suffixed_rules_do_not_hide_a_case() {
        let cases = parse_cases(
            "====== one\nFirst\n:platform(linux)\n====== one\n\nin\n\n--- note\n\n(a b: (c))\n\
             \n===\nSecond\n===\n\nin\n\n---\n\n(d e: (f))\n",
        )
        .unwrap();
        let titles: Vec<_> = cases.iter().map(|case| case.title.as_str()).collect();
        assert_eq!(titles, ["First", "Second"]);
        assert_eq!(cases[0].markers, [":platform(linux)"]);
        assert_eq!(cases[1].tree.trim(), "(d e: (f))");
    }

    #[test]
    fn only_error_cases_may_omit_the_divider() {
        assert_eq!(
            parse_cases("===\nRejected\n:error\n===\n\nin\n").unwrap()[0].tree,
            ""
        );
        assert!(parse_cases("===\nNo divider\n===\n\nin\n").is_err());
    }

    #[test]
    fn a_stray_rule_is_an_error_not_a_dropped_case() {
        let corpus =
            "===\nOne\n===\n\nin\n\n---\n\n(a)\n===\n\n===\nTwo\n===\n\nin\n\n---\n\n(b)\n";
        assert_eq!(
            parse_cases(corpus).err().as_deref(),
            Some("case `` has no `---` divider")
        );
    }

    #[test]
    fn labels_are_read_per_node_outside_error_recovery() {
        let mut found = BTreeSet::new();
        collect_labels(
            "(binary_expression lhs: (a) \"and\" (UNEXPECTED '\\t') rhs: (b) \
             (ERROR keyword: (identifier)))",
            &mut found,
        )
        .unwrap();
        let found: Vec<_> = found
            .into_iter()
            .map(|(node, field)| format!("{node}.{field}"))
            .collect();
        assert_eq!(found, ["binary_expression.lhs", "binary_expression.rhs"]);
    }
}
