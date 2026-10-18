//! The spec is cited by chapter and section title, never by number, which goes stale when a
//! section moves. Every `spec/<chapter>` names a chapter, and every quoted title after one is
//! something `cairn docs` prints for that chapter: a heading, a defined term, or its own title.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cairn_lang_docs::Page;

use super::{Finding, Rule};
use crate::repo::{Repo, line_of};

pub const RULE: Rule = Rule {
    name: "spec-citations",
    summary: "spec citations name a real chapter and section title, never a number",
    check,
};

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
    let chapters = spec_chapters();
    if chapters.is_empty() {
        return vec![Finding::new(
            "website",
            None,
            "cairn-lang-docs embeds no `spec/` page; update `spec_chapters` if the spec moved",
        )];
    }

    let scanned = scanned_files(repo);
    if scanned.is_empty() {
        return vec![Finding::new(
            ".",
            None,
            "scanned no files; update SCANNED in xtask if the trees moved",
        )];
    }
    let mut findings = Vec::new();
    for path in scanned {
        let text = match repo.read(&path) {
            Ok(text) => text,
            Err(error) => {
                findings.push(Finding::new(&path, None, error));
                continue;
            }
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
    chapters: &BTreeMap<&str, &Page>,
    findings: &mut Vec<Finding>,
) {
    let mut cite = |offset: usize, stem: &str, quoted: Vec<String>| {
        let line = Some(line_of(text, offset));
        let Some(chapter) = chapters.get(stem) else {
            let known: Vec<_> = chapters.keys().copied().collect();
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
            if chapter.cited(&title).is_none() {
                findings.push(Finding::new(
                    path,
                    line,
                    format!(
                        "`spec/{stem}` has no section titled \"{title}\"; nearest: {}",
                        nearest(&chapter.titles(), &title)
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

/// Chapter stem → its page, the index under the stem its file has.
fn spec_chapters() -> BTreeMap<&'static str, &'static Page> {
    cairn_lang_docs::pages()
        .iter()
        .filter_map(|page| match page.path() {
            "spec" => Some(("index", page)),
            path => path
                .strip_prefix("spec/")
                .filter(|stem| !stem.contains('/'))
                .map(|stem| (stem, page)),
        })
        .collect()
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
    // A quote right after the chapter closes the string literal the chapter is written in.
    if window
        .strip_prefix(".md")
        .unwrap_or(window)
        .starts_with('"')
    {
        return Vec::new();
    }
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

fn nearest(titles: &[&str], written: &str) -> String {
    let mut ranked: Vec<&str> = titles.to_vec();
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
        assert!(quoted_titles_after("\".into(), \"not a title\"").is_empty());
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
