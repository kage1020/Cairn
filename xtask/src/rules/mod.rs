//! The rules `cargo xtask lint` runs, and the finding each one reports.

mod circuit_advice;
mod comment_length;
mod diagnostic_catalog;
mod editor_keywords;
mod grammar_fields;
mod manifest;
mod module_cycles;
mod readme;
mod session_refs;
mod site_parity;
mod spec_citations;
mod subject_to_change;
mod tier_one_tokens;
mod unsafe_code;
mod versions;

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
    subject_to_change::RULE,
    site_parity::RULE,
    session_refs::RULE,
    readme::MODULES,
    readme::PUBLIC_API,
    readme::SUBCOMMANDS,
    readme::SYNTH_STAGES,
    diagnostic_catalog::RULE,
    tier_one_tokens::RULE,
    grammar_fields::RULE,
    editor_keywords::RULE,
    comment_length::RULE,
    module_cycles::RULE,
    circuit_advice::RULE,
];

/// A fault a rule found, at a file and, when it has one, a line.
pub struct Finding {
    path: PathBuf,
    line: Option<usize>,
    message: String,
}

impl Finding {
    pub fn new(path: impl Into<PathBuf>, line: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            line,
            message: message.into(),
        }
    }

    /// The finding as `rule` reports it, as a line of text.
    pub fn render(&self, rule: &str) -> String {
        let line = self.line.map(|line| format!(":{line}")).unwrap_or_default();
        format!("{}{line}: {rule}: {}", self.path.display(), self.message)
    }

    /// The finding as a GitHub Actions workflow command, which annotates the file in the PR.
    pub fn annotation(&self, rule: &str) -> String {
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
            "::error file={}{line},title={rule}::{message}",
            self.path.display(),
        )
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
        let finding = Finding::new("a/b.rs", Some(3), "one\ntwo 100%");
        assert_eq!(
            finding.annotation("rule"),
            "::error file=a/b.rs,line=3,title=rule::one%0Atwo 100%25"
        );
        assert_eq!(finding.render("rule"), "a/b.rs:3: rule: one\ntwo 100%");
    }
}
