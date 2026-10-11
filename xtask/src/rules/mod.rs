//! The rules `cargo xtask lint` runs, and the finding each one reports.

mod comment_length;
mod diagnostic_catalog;
mod editor_keywords;
mod grammar_fields;
mod manifest;
mod readme;
mod session_refs;
mod spec_citations;
mod unsafe_code;
mod versions;

use std::fmt;
use std::path::PathBuf;

use crate::repo::Repo;

pub struct Rule {
    pub name: &'static str,
    pub summary: &'static str,
    pub check: fn(&Repo) -> Vec<Finding>,
}

pub const RULES: &[Rule] = &[
    manifest::RULE,
    unsafe_code::RULE,
    versions::RULE,
    spec_citations::RULE,
    session_refs::RULE,
    readme::MODULES,
    readme::PUBLIC_API,
    readme::SUBCOMMANDS,
    diagnostic_catalog::RULE,
    grammar_fields::RULE,
    editor_keywords::RULE,
    comment_length::RULE,
];

pub struct Finding {
    rule: &'static str,
    path: PathBuf,
    line: Option<usize>,
    message: String,
}

impl Finding {
    pub fn new(
        rule: &'static str,
        path: impl Into<PathBuf>,
        line: Option<usize>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule,
            path: path.into(),
            line,
            message: message.into(),
        }
    }

    /// The finding as a GitHub Actions workflow command, which annotates the file in the PR.
    pub fn annotation(&self) -> String {
        let line = self
            .line
            .map(|line| format!(",line={line}"))
            .unwrap_or_default();
        let message = self
            .message
            .replace('%', "%25")
            .replace('\r', "%0D")
            .replace('\n', "%0A");
        format!(
            "::error file={}{line},title={}::{message}",
            self.path.display(),
            self.rule
        )
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())?;
        if let Some(line) = self.line {
            write!(f, ":{line}")?;
        }
        write!(f, ": {}: {}", self.rule, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_names_are_unique() {
        let mut names: Vec<_> = RULES.iter().map(|rule| rule.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), RULES.len());
    }

    #[test]
    fn an_annotation_keeps_a_multi_line_message_on_one_line() {
        let finding = Finding::new("rule", "a/b.rs", Some(3), "one\ntwo 100%");
        assert_eq!(
            finding.annotation(),
            "::error file=a/b.rs,line=3,title=rule::one%0Atwo 100%25"
        );
        assert_eq!(finding.to_string(), "a/b.rs:3: rule: one\ntwo 100%");
    }
}
