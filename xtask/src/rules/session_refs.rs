//! No milestone or pull-request coordinates outside the files whose purpose they are.
//!
//! Code, docs and examples have to read on their own later, so a milestone id, a pull request
//! number or a pointer to a pull request yet to come is replaced by the fact it stood for. The
//! changelog, the roadmap, the compatibility table and CONTRIBUTING (which states this rule)
//! are exempt.

use regex::Regex;

use super::{Finding, Rule};
use crate::repo::{Repo, line_of};

pub const RULE: Rule = Rule {
    name: "session-refs",
    summary: "no milestone or PR coordinates outside the changelog, roadmap and compatibility",
    check,
};

/// The `[ ]` classes keep this file from matching its own pattern.
const PATTERN: &str =
    r"\bM[1-6]\b|M[0-9]-PR[0-9]+|pre-M[0-9]|\bPR[0-9]+\b|\blater[ ]PR\b|\bfuture[ ]PR\b";

fn exempt(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.starts_with("CHANGELOG")
        || path.starts_with("changelog.d/")
        || name.starts_with("CONTRIBUTING")
        || name == "compatibility.md"
        || name == "roadmap.md"
        // Hashes in lockfiles are not prose.
        || std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lock"))
        || name == "pnpm-lock.yaml"
}

fn check(repo: &Repo) -> Vec<Finding> {
    let pattern = Regex::new(PATTERN).expect("valid regex");
    let mut findings = Vec::new();
    for path in repo.files() {
        let Some(name) = path.to_str() else {
            continue;
        };
        if exempt(name) {
            continue;
        }
        // Binary files do not read as UTF-8 and hold no prose.
        let Ok(text) = repo.read(path) else {
            continue;
        };
        for found in pattern.find_iter(&text) {
            findings.push(Finding::new(
                RULE.name,
                path,
                Some(line_of(&text, found.start())),
                format!(
                    "`{}` is a session-local coordinate; write the fact it stands for",
                    found.as_str()
                ),
            ));
        }
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
        ];
        for hit in hits {
            assert!(pattern.is_match(&hit), "{hit}");
        }
        for miss in ["M7", "M10", "PROMPT", "cell #0", "spare PR"] {
            assert!(!pattern.is_match(miss), "{miss}");
        }
    }

    #[test]
    fn the_changelog_and_roadmap_are_exempt() {
        assert!(exempt("CHANGELOG.md"));
        assert!(exempt("changelog.d/x.fixed.md"));
        assert!(exempt("website/src/content/docs/ja/roadmap.md"));
        assert!(!exempt("crates/cairn-lang-core/src/lib.rs"));
    }
}
