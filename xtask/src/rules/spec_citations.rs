//! The spec is cited by chapter and section title, never by number, which goes stale when a
//! section moves. Every `spec/<chapter>` names a chapter, and every quoted title after one is a
//! heading of that chapter or a glossary term.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{Finding, Rule};
use crate::repo::{Repo, line_of};

pub const RULE: Rule = Rule {
    name: "spec-citations",
    summary: "spec citations name a real chapter and section title, never a number",
    check,
};

const SPEC_DIR: &str = "website/src/content/docs/spec";

const SCANNED: &[&str] = &[
    "crates",
    "examples",
    ".github",
    ".claude",
    "xtask",
    "CLAUDE.md",
];
const SCANNED_EXTENSIONS: &[&str] = &["rs", "md", "crn"];

/// How far past the chapter a quoted title may reach: one wrapped over two comment lines.
const TITLE_WINDOW: usize = 400;

const SECTION_SIGN: char = '\u{a7}';

fn check(repo: &Repo) -> Vec<Finding> {
    let chapters = spec_titles(repo);
    if chapters.is_empty() {
        return vec![Finding::new(
            SPEC_DIR,
            None,
            "no spec chapters found; update SPEC_DIR in xtask if the spec moved",
        )];
    }

    let mut findings = Vec::new();
    for path in scanned_files(repo) {
        let Ok(text) = repo.read(&path) else {
            continue;
        };
        numbered_citations(&path, &text, &mut findings);
        named_citations(&path, &text, &chapters, &mut findings);
    }
    findings
}

fn scanned_files(repo: &Repo) -> Vec<PathBuf> {
    repo.files()
        .iter()
        .filter(|path| {
            SCANNED.iter().any(|tree| path.starts_with(tree))
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| SCANNED_EXTENSIONS.contains(&extension))
        })
        .cloned()
        .collect()
}

fn numbered_citations(path: &Path, text: &str, findings: &mut Vec<Finding>) {
    let finding = |offset: usize, what: String| {
        Finding::new(
            path,
            Some(line_of(text, offset)),
            format!(
                "{what} cites the spec by number; name the chapter and quote the section title, \
                 as `spec/<chapter>` \"<title>\""
            ),
        )
    };
    for (offset, _) in text.match_indices(SECTION_SIGN) {
        findings.push(finding(offset, format!("`{SECTION_SIGN}`")));
    }
    // ASCII lowering keeps byte offsets valid for `text`.
    let lowered = text.to_ascii_lowercase();
    for (word, also) in [("section ", '.'), ("spec_", '_')] {
        for (offset, _) in lowered.match_indices(word) {
            let rest = &lowered[offset + word.len()..];
            if rest.starts_with(|c: char| c.is_ascii_digit()) {
                let number: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == also)
                    .collect();
                findings.push(finding(offset, format!("\"{word}{number}\"")));
            }
        }
    }
}

fn named_citations(
    path: &Path,
    text: &str,
    chapters: &BTreeMap<String, Vec<String>>,
    findings: &mut Vec<Finding>,
) {
    let mut cite = |offset: usize, stem: &str, quoted: Vec<String>| {
        let line = Some(line_of(text, offset));
        let Some(titles) = chapters.get(stem) else {
            let known: Vec<_> = chapters.keys().map(String::as_str).collect();
            findings.push(Finding::new(
                path,
                line,
                format!(
                    "`spec/{stem}` names no chapter; the chapters are: {}",
                    known.join(", ")
                ),
            ));
            return;
        };
        for title in quoted {
            if !titles.contains(&title) {
                findings.push(Finding::new(
                    path,
                    line,
                    format!(
                        "`spec/{stem}` has no section titled \"{title}\"; nearest: {}",
                        nearest(titles, &title)
                    ),
                ));
            }
        }
    };

    for (offset, _) in text.match_indices("spec/") {
        let rest = &text[offset + "spec/".len()..];
        let stem = chapter_stem(rest);
        if !stem.is_empty() {
            cite(offset, stem, quoted_titles_after(&rest[stem.len()..]));
        }
    }
    for (open, _) in text.match_indices('[') {
        if let Some((stem, quoted)) = markdown_link_citation(text, open) {
            cite(open, stem, quoted);
        }
    }
}

fn chapter_stem(text: &str) -> &str {
    let end = text
        .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        .unwrap_or(text.len());
    &text[..end]
}

/// Chapter file stem → its frontmatter title, its headings without numbers, and glossary terms.
fn spec_titles(repo: &Repo) -> BTreeMap<String, Vec<String>> {
    let mut chapters = BTreeMap::new();
    for path in repo.files_in(Path::new(SPEC_DIR), &["md"]) {
        if path.parent() != Some(Path::new(SPEC_DIR)) {
            continue;
        }
        let (Some(stem), Ok(text)) = (
            path.file_stem().and_then(|stem| stem.to_str()),
            repo.read(path),
        ) else {
            continue;
        };
        let mut titles = Vec::new();
        let mut fenced = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
            } else if fenced {
            } else if let Some(rest) = line.strip_prefix("title:") {
                titles.push(strip_leading_number(rest.trim().trim_matches('"')));
            } else if let Some(rest) = line.strip_prefix("##") {
                titles.push(strip_leading_number(rest.trim_start_matches('#').trim()));
            } else if stem == "glossary"
                && let Some(term) = defined_term(line)
            {
                titles.push(term);
            }
        }
        chapters.insert(stem.to_owned(), titles);
    }
    chapters
}

/// The term a glossary row defines: `- **Fail-loud.** …` defines `Fail-loud`.
fn defined_term(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("- **")?;
    let end = rest.find("**")?;
    Some(rest[..end].trim_end_matches('.').to_owned())
}

/// A heading without its number, such as the `14.5` or `C.3` before it.
fn strip_leading_number(heading: &str) -> String {
    let digits = heading.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    let after = if digits.len() < heading.len() {
        digits
    } else {
        let mut chars = heading.char_indices();
        match (chars.next(), chars.next()) {
            (Some((_, letter)), Some((dot, '.'))) if letter.is_ascii_uppercase() => {
                heading[dot..].trim_start_matches(|c: char| c.is_ascii_digit() || c == '.')
            }
            _ => heading,
        }
    };
    if after.len() == heading.len() || !after.starts_with(char::is_whitespace) {
        return heading.to_owned();
    }
    after.trim_start().to_owned()
}

/// The chapter and titles of a Markdown link citing the spec, given the offset of its `[`.
fn markdown_link_citation(text: &str, open: usize) -> Option<(&str, Vec<String>)> {
    let rest = &text[open + 1..];
    let close = rest.find("](")?;
    let label = &rest[..close];
    if label.contains('\n') {
        return None;
    }
    let href = &rest[close + 2..];
    let href = &href[..href.find(')')?];
    let stem = chapter_stem(label);
    if stem.is_empty() || !href.contains(&format!("/spec/{stem}/")) {
        return None;
    }
    let titles = quoted_titles_after(&label[stem.len()..]);
    (!titles.is_empty()).then_some((stem, titles))
}

/// The quoted titles right after a chapter stem, joined by ` / `, ` and ` or `, `.
fn quoted_titles_after(rest: &str) -> Vec<String> {
    let window = match rest.char_indices().nth(TITLE_WINDOW) {
        Some((end, _)) => &rest[..end],
        None => rest,
    };
    // A citation inside a Rust string literal escapes its quotes.
    let joined = unwrapped(window).replace("\\\"", "\"");

    let mut cursor = joined.strip_prefix(".md").unwrap_or(&joined);
    cursor = cursor.trim_start_matches(['`', ']', ')', ' ']);

    let mut titles = Vec::new();
    while let Some(inner) = cursor.strip_prefix('"') {
        let Some(end) = inner.find('"') else { break };
        titles.push(inner[..end].trim().to_owned());
        let after = &inner[end + 1..];
        let Some(next) = after
            .strip_prefix(" / ")
            .or_else(|| after.strip_prefix(" and "))
            .or_else(|| after.strip_prefix(", "))
        else {
            break;
        };
        cursor = next;
    }
    titles
}

/// `text` joined back into one line across comment markers and string-literal line breaks.
fn unwrapped(text: &str) -> String {
    text.lines()
        .map(|line| {
            let line = line.trim_start();
            let line = line
                .strip_prefix("///")
                .or_else(|| line.strip_prefix("//!"))
                .or_else(|| line.strip_prefix("//"))
                .or_else(|| line.strip_prefix('#'))
                .unwrap_or(line)
                .trim();
            line.strip_suffix('\\').unwrap_or(line).trim_end()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn nearest(titles: &[String], written: &str) -> String {
    let mut ranked: Vec<&String> = titles.iter().collect();
    ranked.sort_by(|a, b| {
        strsim::normalized_levenshtein(b, written)
            .total_cmp(&strsim::normalized_levenshtein(a, written))
    });
    ranked
        .iter()
        .take(3)
        .map(|title| format!("\"{title}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_stripped_only_when_they_are_numbers() {
        assert_eq!(
            strip_leading_number("14.5 Place-and-route"),
            "Place-and-route"
        );
        assert_eq!(
            strip_leading_number("C.3 How a break is communicated"),
            "How a break is communicated"
        );
        assert_eq!(strip_leading_number("3D coordinates"), "3D coordinates");
        assert_eq!(strip_leading_number("Glossary"), "Glossary");
    }

    #[test]
    fn a_title_wrapped_across_comment_lines_reads_whole() {
        assert_eq!(
            quoted_titles_after("` \"Java / Bedrock\n    /// portability\": rest"),
            ["Java / Bedrock portability"]
        );
        assert_eq!(
            quoted_titles_after("` \"Time model\" / \"Connection to the IR and phases\""),
            ["Time model", "Connection to the IR and phases"]
        );
        assert!(quoted_titles_after("` says \"not a title\"").is_empty());
        assert_eq!(quoted_titles_after(r#"` \"Escaped\""#), ["Escaped"]);
    }

    #[test]
    fn a_markdown_citation_needs_the_href_to_agree() {
        let text = "[lint \"Error vs warning\"](https://x/spec/lint/)";
        assert_eq!(
            markdown_link_citation(text, 0),
            Some(("lint", vec!["Error vs warning".to_owned()]))
        );
        assert_eq!(
            markdown_link_citation("[lint \"X\"](https://x/other/)", 0),
            None
        );
    }

    #[test]
    fn every_numbered_form_is_reported() {
        let mut findings = Vec::new();
        let text = format!(
            "a {SECTION_SIGN}3\nsee Section {0}.3\nfn spec_{0}_3() {{}}\n",
            11
        );
        numbered_citations(Path::new("x.rs"), &text, &mut findings);
        let lines: Vec<_> = findings.iter().map(|finding| finding.line).collect();
        assert_eq!(lines, [Some(1), Some(2), Some(3)]);
    }
}
