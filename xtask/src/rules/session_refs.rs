//! No milestone, issue or pull-request coordinates outside the files whose purpose they are: code,
//! docs and examples have to read on their own later.

use regex::Regex;

use super::{Finding, Rule};
use crate::repo::{Repo, line_of};

pub const RULE: Rule = Rule {
    name: "session-refs",
    summary: "no milestone, issue or PR coordinates outside the changelog, roadmap and compatibility",
    check,
};

/// The `[ ]` classes keep this file from matching its own pattern.
const PATTERN: &str = concat!(
    r"\bM[1-6]\b|M[0-9]-PR[0-9]+|pre-M[0-9]|\bPR[0-9]+\b|\blater[ ]PR\b|\bfuture[ ]PR\b",
    r"|\(#[0-9]+\)|(?i:\b(?:PR|issue|closes|fixes|refs)[ ]#[0-9]+)|/(?:pull|issues)/[0-9]+",
);

/// Formats that hold no prose, so a file in one need not read as UTF-8.
const BINARY: &[&str] = &[
    "gz",
    "ico",
    "jpeg",
    "jpg",
    "mcstructure",
    "nbt",
    "png",
    "wasm",
    "webp",
    "woff",
    "woff2",
];

fn exempt(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.starts_with("CHANGELOG")
        || path.starts_with("changelog.d/")
        || name.starts_with("CONTRIBUTING")
        || name == "compatibility.md"
        || name == "roadmap.md"
        // The template asks for the issue a PR closes; the tests feed the changelog references.
        || path == ".github/pull_request_template.md"
        || path == ".github/scripts/test_changelog.py"
        // Hashes in lockfiles are not prose.
        || std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lock"))
        || name == "pnpm-lock.yaml"
}

fn check(repo: &Repo) -> Vec<Finding> {
    let pattern = Regex::new(PATTERN).expect("valid regex");
    let mut findings = Vec::new();
    let mut scanned = 0;
    for path in repo.files() {
        let Some(name) = path.to_str() else {
            findings.push(Finding::new(path, None, "the path is not UTF-8"));
            continue;
        };
        let binary = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| BINARY.contains(&extension.to_ascii_lowercase().as_str()));
        if exempt(name) || binary {
            continue;
        }
        scanned += 1;
        let text = match repo.read(path) {
            Ok(text) => text,
            Err(error) => {
                findings.push(Finding::new(path, None, error));
                continue;
            }
        };
        for found in pattern.find_iter(&text) {
            findings.push(Finding::new(
                path,
                Some(line_of(&text, found.start())),
                format!(
                    "`{}` is a session-local coordinate; write the fact it stands for",
                    found.as_str()
                ),
            ));
        }
    }
    if scanned == 0 {
        findings.push(Finding::new(".", None, "scanned no files"));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_match_and_ordinary_words_do_not() {
        let pattern = Regex::new(PATTERN).unwrap();
        // Assembled at run time so this file does not match its own rule.
        let hits = [
            format!("M{}", 3),
            format!("M{}-PR{}", 3, 4),
            format!("pre-M{}", 2),
            format!("PR{}", 12),
            format!("a later {}", "PR"),
            format!("a future {}", "PR"),
            format!("fixed (#{})", 475),
            format!("PR #{}", 12),
            format!("closes #{}", 3),
            format!("/pull/{}", 455),
            format!("/issues/{}", 12),
        ];
        for hit in hits {
            assert!(pattern.is_match(&hit), "{hit}");
        }
        for miss in ["M7", "M10", "PROMPT", "cell #0", "spare PR", "(#[0-9]+)"] {
            assert!(!pattern.is_match(miss), "{miss}");
        }
    }

    #[test]
    fn the_changelog_and_roadmap_are_exempt() {
        assert!(exempt("CHANGELOG.md"));
        assert!(exempt("changelog.d/x.fixed.md"));
        assert!(exempt("website/src/content/docs/ja/roadmap.md"));
        assert!(exempt(".github/pull_request_template.md"));
        assert!(!exempt("crates/cairn-lang-core/src/lib.rs"));
    }
}
