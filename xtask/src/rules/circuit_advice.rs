//! No `Fix:` line advises splitting logic into several `circuit` blocks: `check` refuses a second
//! `circuit` line in a scope as `E_DUPLICATE_CIRCUIT`, so the advice is to split it across scopes.

use std::path::Path;

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "circuit-advice",
    summary: "no diagnostic advises a second `circuit` line in a scope, which `check` refuses",
    check,
};

const REFUSED_ADVICE: &str = "`circuit` block";

const SPLIT_ADVICE: &str =
    "split the logic across several scopes, each with its own `circuit` line";

/// `source` with comment markers dropped and lines joined as the compiler joins a string's `\`
/// continuation, so a phrase a line break splits reads whole; each byte maps to its source line.
fn joined(source: &str) -> (String, Vec<usize>) {
    let mut text = String::with_capacity(source.len());
    let mut lines = Vec::with_capacity(source.len());
    let mut continued = false;
    for (number, line) in source.lines().enumerate() {
        let line = line.trim_start();
        let line = ["//!", "///", "//"]
            .iter()
            .find_map(|marker| line.strip_prefix(marker))
            .map_or(line, str::trim_start);
        if !continued && !text.is_empty() {
            text.push(' ');
            lines.push(number + 1);
        }
        let head = line.strip_suffix('\\');
        let kept = head.unwrap_or(line);
        text.push_str(kept);
        lines.extend(std::iter::repeat_n(number + 1, kept.len()));
        continued = head.is_some();
    }
    (text, lines)
}

fn check(repo: &Repo) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut advised = 0;
    let sources = repo.files_in(Path::new("crates"), &["rs"]).filter(|path| {
        path.components()
            .nth(2)
            .is_some_and(|dir| dir.as_os_str() == "src")
    });
    for path in sources {
        let source = match repo.read(path) {
            Ok(source) => source,
            Err(error) => {
                findings.push(Finding::new(path, None, error));
                continue;
            }
        };
        let (text, lines) = joined(&source);
        advised += text.matches(SPLIT_ADVICE).count();
        for (offset, _) in text.match_indices(REFUSED_ADVICE) {
            findings.push(Finding::new(
                path,
                Some(lines[offset]),
                format!(
                    "advises splitting into several `circuit` blocks, which `check` refuses as \
                     `E_DUPLICATE_CIRCUIT` within one scope; advise \"{SPLIT_ADVICE}\""
                ),
            ));
        }
    }
    if advised == 0 {
        findings.push(Finding::new(
            "crates",
            None,
            format!("found no \"{SPLIT_ADVICE}\" under crates/*/src, so no Fix line was read"),
        ));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phrase_split_by_a_line_break_reads_whole_on_its_first_line() {
        let source = "let a = 1;\n// or split it into `circuit`\n// blocks\nlet b = \"`circuit` \\\n    block\";\n";
        let (text, lines) = joined(source);
        let found: Vec<usize> = text
            .match_indices(REFUSED_ADVICE)
            .map(|(offset, _)| lines[offset])
            .collect();
        assert_eq!(found, [2, 4]);
    }
}
