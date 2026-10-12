//! A comment is a few lines of why. One that needs more is defending the code or retelling it.

use std::path::Path;

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "comment-length",
    summary: "a comment block keeps to its budget: `//` 3 lines, `///` 4, `//!` 10",
    check,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Line,
    Item,
    Module,
}

impl Kind {
    /// The kind of a trimmed comment line and the text after its marker.
    fn of(trimmed: &str) -> Option<(Self, &str)> {
        if let Some(text) = trimmed.strip_prefix("//!") {
            Some((Self::Module, text))
        } else if let Some(text) = trimmed.strip_prefix("///")
            && !text.starts_with('/')
        {
            Some((Self::Item, text))
        } else {
            trimmed.strip_prefix("//").map(|text| (Self::Line, text))
        }
    }

    const fn budget(self) -> usize {
        match self {
            Self::Line => 3,
            Self::Item => 4,
            Self::Module => 10,
        }
    }

    const fn marker(self) -> &'static str {
        match self {
            Self::Line => "//",
            Self::Item => "///",
            Self::Module => "//!",
        }
    }
}

/// Blank lines and rustdoc's `# Errors` style headings carry no prose.
fn is_prose(kind: Kind, text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && (kind == Kind::Line || !text.starts_with("# "))
}

/// Each comment block over budget: its first line (1-based), kind and prose line count.
fn over_budget(source: &str) -> Vec<(usize, Kind, usize)> {
    let mut found = Vec::new();
    let mut open: Option<(usize, Kind, usize)> = None;
    for (index, line) in source.lines().enumerate() {
        let comment = Kind::of(line.trim_start());
        match (open, comment) {
            (Some((start, kind, prose)), Some((next, text))) if next == kind => {
                open = Some((start, kind, prose + usize::from(is_prose(kind, text))));
            }
            _ => {
                if let Some(block) = open.take()
                    && block.2 > block.1.budget()
                {
                    found.push(block);
                }
                open = comment
                    .map(|(kind, text)| (index + 1, kind, usize::from(is_prose(kind, text))));
            }
        }
    }
    if let Some(block) = open
        && block.2 > block.1.budget()
    {
        found.push(block);
    }
    found
}

fn check(repo: &Repo) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut scanned = 0;
    for path in repo.files_in(Path::new(""), &["rs"]) {
        scanned += 1;
        let source = match repo.read(path) {
            Ok(source) => source,
            Err(error) => {
                findings.push(Finding::new(path, None, error));
                continue;
            }
        };
        for (line, kind, prose) in over_budget(&source) {
            findings.push(Finding::new(
                path,
                Some(line),
                format!(
                    "this `{}` comment runs {prose} lines, over its budget of {}; keep the why \
                     the code cannot say and drop the rest",
                    kind.marker(),
                    kind.budget(),
                ),
            ));
        }
    }
    if scanned == 0 {
        findings.push(Finding::new(".", None, "scanned no Rust files"));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_at_its_budget_passes_and_one_line_more_does_not() {
        let source = "// a\n// b\n// c\nfn f() {}\n// a\n// b\n// c\n// d\n";
        assert_eq!(over_budget(source), [(5, Kind::Line, 4)]);
    }

    #[test]
    fn blank_lines_and_headings_are_not_prose() {
        let source = "/// a\n///\n/// # Errors\n///\n/// b\n/// c\n/// d\nfn f() {}\n";
        assert_eq!(over_budget(source), []);
    }

    #[test]
    fn a_change_of_marker_starts_a_new_block() {
        let source = "//! a\n//! b\n/// a\n/// b\n/// c\n/// d\n// a\n// b\n// c\n";
        assert_eq!(over_budget(source), []);
    }

    #[test]
    fn four_slashes_are_a_plain_comment() {
        let source = "//// a\n//// b\n//// c\n//// d\n";
        assert_eq!(over_budget(source), [(1, Kind::Line, 4)]);
    }

    #[test]
    fn a_trailing_comment_is_not_a_block() {
        let source = "let a = 1; // a\nlet b = 2; // b\nlet c = 3; // c\nlet d = 4; // d\n";
        assert_eq!(over_budget(source), []);
    }
}
