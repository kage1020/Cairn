//! Source, tests, examples, and crate READMEs cite the spec by *name*, never by
//! section number.
//!
//! A section number is a coordinate, not a fact. `§14.5` is only true while
//! "Place-and-route" happens to be the fifth section of the fourteenth chapter;
//! insert a chapter, split a section, and several hundred comments become
//! quietly wrong with nothing to catch it. The cost lands on whoever edits the
//! spec next, as a full audit of the source tree — which is exactly the tax
//! CONTRIBUTING's "No session-local references" rule already refuses to pay for
//! issue numbers and milestone coordinates. The fix is the same one: replace
//! the coordinate with the fact it stood in for.
//!
//! So a citation names the chapter file and, when it means one section, that
//! section's title:
//!
//! ```text
//! /// Stable per `spec/lint` "Error vs warning": errors are things ...
//! ```
//!
//! This test holds three lines:
//!
//! 1. No `§` and no "section 11.3" survives outside the spec itself.
//! 2. Every `spec/<chapter>` names a chapter that exists.
//! 3. Every quoted title that follows one is a real heading in that chapter.
//!
//! Renumbering the spec now changes nothing here. *Retitling* a section fails
//! this test naming the file and line, which is the right trade: a title change
//! is a change of meaning, and the comment that cited it deserves a re-read.
//!
//! What stays unchecked is whether the citation is *apt* — nothing here can
//! tell that "Error vs warning" is the section that actually settles the
//! question the comment is asking. This holds the reference, not the argument.
//!
//! It lives in `cairn-lang-core` for want of a workspace-level test target; it
//! reads the whole repository, not this crate. When the spec directory is
//! absent — a packaged crate, published without the website — there is nothing
//! to check against and the test reports that it skipped rather than failing.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The repository root, two levels up from `crates/cairn-lang-core`.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two directories below the repository root")
        .to_path_buf()
}

/// Where the English spec chapters live. The Japanese mirror under `ja/` is a
/// translation of the same numbered headings, so it is spec too, not a citation
/// of one.
const SPEC_DIR: &str = "website/src/content/docs/spec";

/// Trees worth reading. The spec and its translation are excluded by not being
/// listed: they are where the numbers are *defined*.
const SCANNED: [&str; 3] = ["crates", "examples", ".github"];

/// Extensions that carry prose we are holding to the rule.
const SCANNED_EXTENSIONS: [&str; 3] = ["rs", "md", "crn"];

/// Build output and vendored packages, which are neither ours nor prose.
const SKIPPED_DIRECTORIES: [&str; 3] = ["target", "node_modules", "snapshots"];

/// `CONTRIBUTING` documents this rule and has to quote the pattern it bans, and
/// `CHANGELOG` is one of the surfaces CONTRIBUTING already exempts because
/// release vocabulary is what it is for.
fn is_exempt(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    name.starts_with("CONTRIBUTING")
        || name.starts_with("CHANGELOG")
        || name == "spec_references_name_sections.rs"
}

/// Every prose file under the scanned trees.
fn scanned_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for tree in SCANNED {
        collect(&root.join(tree), &mut found);
    }
    found.sort();
    found
}

fn collect(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        if path.is_dir() {
            if !SKIPPED_DIRECTORIES.contains(&name) && !name.starts_with('.') {
                collect(&path, found);
            }
            continue;
        }
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if SCANNED_EXTENSIONS.contains(&extension) && !is_exempt(&path) {
            found.push(path);
        }
    }
}

/// Chapter file stem to the set of titles it declares: its own, from the
/// frontmatter, and one per numbered heading, each with the leading number
/// stripped.
///
/// `title: "14. Redstone (logic circuits)"` and `## 14.5 Place-and-route` give
/// `redstone` the titles `Redstone (logic circuits)` and `Place-and-route`.
fn spec_titles(root: &Path) -> BTreeMap<String, Vec<String>> {
    let mut chapters = BTreeMap::new();
    let Ok(entries) = fs::read_dir(root.join(SPEC_DIR)) else {
        return chapters;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a `.md` file has a stem")
            .to_owned();
        let text = fs::read_to_string(&path).expect("spec chapter is readable");

        let mut titles = Vec::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("title:") {
                titles.push(strip_leading_number(rest.trim().trim_matches('"')));
            } else if let Some(rest) = line.strip_prefix("##") {
                titles.push(strip_leading_number(rest.trim_start_matches('#').trim()));
            } else if let Some(term) = defined_term(line) {
                titles.push(term);
            }
        }
        chapters.insert(stem, titles);
    }
    chapters
}

/// The term a glossary row defines, from `- **Fail-loud.** Silent substitution
/// …`.
///
/// The glossary is a definition list rather than a chapter of headings, so its
/// entries are names to cite too — and a term is a better anchor than the
/// section that happens to hold it, being the thing the comment actually means.
/// The trailing full stop belongs to the sentence, not the term.
fn defined_term(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("- **")?;
    let end = rest.find("**")?;
    Some(rest[..end].trim_end_matches('.').to_owned())
}

/// `14.5 Place-and-route` without its `14.5`. A heading with no leading number
/// — `Glossary`, `Compatibility Tiers` — is returned unchanged.
fn strip_leading_number(heading: &str) -> String {
    let after_number = heading.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    if after_number.len() == heading.len() {
        return heading.to_owned();
    }
    after_number.trim_start().to_owned()
}

/// The 1-based line a byte offset falls on, for an error a reader can jump to.
fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count() + 1
}

#[test]
fn no_file_cites_the_spec_by_section_number() {
    let root = repo_root();
    let mut offences = Vec::new();

    for path in scanned_files(&root) {
        let text = fs::read_to_string(&path).expect("scanned file is readable");
        let display = path.strip_prefix(&root).unwrap_or(&path).display();

        for (offset, _) in text.match_indices('§') {
            offences.push(format!("{display}:{}: `§`", line_of(&text, offset)));
        }

        let lowered = text.to_lowercase();
        for (offset, _) in lowered.match_indices("section ") {
            let rest = &lowered[offset + "section ".len()..];
            if rest.starts_with(|c: char| c.is_ascii_digit()) {
                let number: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                offences.push(format!(
                    "{display}:{}: \"section {number}\"",
                    line_of(&text, offset)
                ));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "the spec is cited by section number in {} place(s). A number is a coordinate: \
         renumbering the spec silently invalidates it. Name the chapter and the section title \
         instead — `spec/lint` \"Error vs warning\" — so the citation says what it meant.\n{}",
        offences.len(),
        offences.join("\n")
    );
}

#[test]
fn every_spec_citation_names_a_chapter_and_a_real_section() {
    let root = repo_root();
    let chapters = spec_titles(&root);
    if chapters.is_empty() {
        eprintln!(
            "skipped: no spec chapters under {SPEC_DIR}, so there is nothing to check citations \
             against. This is the packaged-crate case, where the website is not shipped."
        );
        return;
    }

    let mut offences = Vec::new();

    for path in scanned_files(&root) {
        let text = fs::read_to_string(&path).expect("scanned file is readable");
        let display = path.strip_prefix(&root).unwrap_or(&path).display();

        for (offset, _) in text.match_indices("spec/") {
            let rest = &text[offset + "spec/".len()..];
            let stem: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                .collect();
            if stem.is_empty() {
                continue;
            }
            let line = line_of(&text, offset);

            let Some(titles) = chapters.get(&stem) else {
                offences.push(format!(
                    "{display}:{line}: `spec/{stem}` names no chapter. The chapters are: {}",
                    chapters.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
                continue;
            };

            let Some(quoted) = quoted_title_after(&rest[stem.len()..]) else {
                continue;
            };
            if !titles.iter().any(|title| title == &quoted) {
                offences.push(format!(
                    "{display}:{line}: `spec/{stem}` has no section titled \"{quoted}\". Nearest: \
                     {}",
                    nearest(titles, &quoted)
                ));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "{} spec citation(s) name something the spec does not have. Either the citation is \
         stale, or a section was retitled and the comments that leaned on it need re-reading.\n{}",
        offences.len(),
        offences.join("\n")
    );
}

/// The title in `` `spec/lint` "Error vs warning" ``, given everything after
/// `lint`.
///
/// Only the punctuation that can sit between the chapter and its title is
/// stepped over — a closing backtick, the `.md` some citations still spell, a
/// markdown link's `]`, and spaces. Anything else means the quote that follows
/// belongs to a different sentence, so there is no title here to check. A
/// section title containing a double quote cannot be written in this form;
/// name the chapter and describe the section in prose instead.
///
/// The title may be wrapped across lines, because comments wrap at 100 columns
/// and several titles are most of a sentence. Reading one therefore has to put
/// the line back together, which [`unwrapped`] does.
fn quoted_title_after(rest: &str) -> Option<String> {
    let rest = rest.strip_prefix(".md").unwrap_or(rest);
    let rest = rest.trim_start_matches(['`', ']', ')', ' ']);
    let inner = rest.strip_prefix('"')?;
    let end = inner.find('"')?;
    Some(unwrapped(&inner[..end]))
}

/// A citation that a line break ran through, joined back into one line.
///
/// A wrapped comment resumes with its marker — `///`, `//!`, `//`, `#` in a
/// `.crn` example, nothing at all in a README — so the continuation is dropped
/// down to the prose, and every run of whitespace becomes one space. What comes
/// back compares equal to the heading it was copied from.
fn unwrapped(title: &str) -> String {
    title
        .lines()
        .map(|line| {
            let line = line.trim_start();
            let line = line
                .strip_prefix("///")
                .or_else(|| line.strip_prefix("//!"))
                .or_else(|| line.strip_prefix("//"))
                .or_else(|| line.strip_prefix('#'))
                .unwrap_or(line);
            line.trim()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The handful of titles closest to what was written, for an error that says
/// what the citation probably meant instead of listing a chapter's every
/// heading.
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
