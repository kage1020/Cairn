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
//! 1. No `§`, no "section 11.3", and no `spec_11_3` buried in an identifier
//!    survives outside the spec itself.
//! 2. Every `spec/<chapter>` names a chapter that exists.
//! 3. Every quoted title that follows one is a real heading in that chapter.
//!
//! Renumbering the spec now changes nothing here. *Retitling* a section fails
//! this test naming the file and line, which is the right trade: a title change
//! is a change of meaning, and the comment that cited it deserves a re-read.
//!
//! Two things stay unchecked. Whether the citation is *apt* — nothing here can
//! tell that "Error vs warning" is the section that actually settles the
//! question the comment is asking; this holds the reference, not the argument.
//! And anything outside [`SCANNED`]: `website/` is excluded because that is
//! where the numbers are defined, and `editors/` and the root READMEs are
//! simply not read.
//!
//! It lives in `cairn-lang-core` for want of a workspace-level test target; it
//! reads the trees in [`SCANNED`], not this crate. A packaged crate, unpacked
//! into a registry directory that holds none of them, has nothing to check and
//! both tests return early. Inside the workspace they never skip: a spec
//! directory that has moved fails rather than passing quietly.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The directory two levels up from `crates/cairn-lang-core`, which is the
/// repository root when this runs from a checkout.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two directories below the repository root")
        .to_path_buf()
}

/// Whether `root` is the Cairn workspace rather than the registry directory a
/// packaged crate was unpacked into.
///
/// This is what tells a missing spec directory apart from a missing
/// repository. Published crates carry their own `Cargo.toml` but not the
/// workspace one, and `cargo package` rewrites the manifest it ships, so the
/// `[workspace]` table is the marker that survives exactly one of the two
/// cases.
fn is_workspace_root(root: &Path) -> bool {
    fs::read_to_string(root.join("Cargo.toml")).is_ok_and(|manifest| {
        manifest
            .lines()
            .any(|line| line.trim_end() == "[workspace]")
    })
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
const SKIPPED_DIRECTORIES: [&str; 2] = ["target", "node_modules"];

/// This file, which has to quote the patterns it bans in order to name them.
///
/// Read from [`file!`] rather than written out, so that moving this test — the
/// module doc above says a workspace-level target is where it belongs — does
/// not turn it into its own first offence.
fn is_exempt(path: &Path) -> bool {
    fn name(path: &Path) -> Option<&str> {
        path.file_name().and_then(|n| n.to_str())
    }
    name(path).is_some() && name(path) == name(Path::new(file!()))
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
/// frontmatter, one per numbered heading at any depth, each with the leading
/// number stripped, and — in the glossary alone — one per defined term.
///
/// `title: "14. Redstone (logic circuits)"` and `## 14.5 Place-and-route` give
/// `redstone` the titles `Redstone (logic circuits)` and `Place-and-route`.
///
/// Fenced blocks are skipped. `spec/compatibility` illustrates a release note
/// with a ```` ```text ```` block whose sample headings sit at column zero;
/// collecting those would make `` `spec/compatibility` "Breaking changes" ``
/// pass against a section that does not exist.
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
        let mut fenced = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            if let Some(rest) = line.strip_prefix("title:") {
                titles.push(strip_leading_number(rest.trim().trim_matches('"')));
            } else if let Some(rest) = line.strip_prefix("##") {
                titles.push(strip_leading_number(rest.trim_start_matches('#').trim()));
            } else if stem == "glossary"
                && let Some(term) = defined_term(line)
            {
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
///
/// Only the glossary, because a bold lead-in is ordinary prose elsewhere: six
/// other chapters carry twenty-three of them, and reading those as sections
/// would let `` `spec/compilation` "Stair orientation" `` pass against a
/// paragraph rather than a heading.
fn defined_term(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("- **")?;
    let end = rest.find("**")?;
    Some(rest[..end].trim_end_matches('.').to_owned())
}

/// `14.5 Place-and-route` without its `14.5`, and `C.3 How a break is
/// communicated` without its `C.3` — the appendix numbers with a letter, and a
/// citation that had to carry the `C.3` would be the coordinate this whole
/// convention exists to delete.
///
/// A heading with no leading number — `Glossary`, `Compatibility Tiers` — is
/// returned unchanged, and so is one that merely opens with a digit:
/// `3D coordinates` is a title, not a numbered `3` followed by `D coordinates`.
/// Requiring whitespace after the run is what tells those apart.
fn strip_leading_number(heading: &str) -> String {
    let digits = heading.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    let after = if digits.len() < heading.len() {
        digits
    } else {
        // `C.1 …`: one uppercase letter, then the numbering proper.
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

/// The 1-based line a byte offset falls on, for an error a reader can jump to.
fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count() + 1
}

#[test]
fn no_file_cites_the_spec_by_section_number() {
    let root = repo_root();
    if !is_workspace_root(&root) {
        return;
    }
    let files = scanned_files(&root);
    assert!(
        !files.is_empty(),
        "no prose files under {SCANNED:?} in {}, so this test would pass without reading \
         anything. Either a tree was renamed or the scan is broken.",
        root.display()
    );
    let mut offences = Vec::new();

    for path in files {
        let text = fs::read_to_string(&path).expect("scanned file is readable");
        let display = path.strip_prefix(&root).unwrap_or(&path).display();

        for (offset, _) in text.match_indices('§') {
            offences.push(format!("{display}:{}: `§`", line_of(&text, offset)));
        }

        // ASCII-only, so offsets into `lowered` index `text` too: every
        // pattern below is ASCII, and `to_lowercase` is not length-preserving
        // for every character (`İ` grows, `ẞ` shrinks), which would drift the
        // reported line and can split `text[..offset]` off a char boundary.
        let lowered = text.to_ascii_lowercase();
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

        // A coordinate hides just as well inside an identifier, where neither
        // of the two spellings above is there to catch it: a test named
        // `every_code_is_classified_against_spec_11_3` goes stale on exactly
        // the same edit, and nothing renames it.
        for (offset, _) in lowered.match_indices("spec_") {
            let rest = &lowered[offset + "spec_".len()..];
            if rest.starts_with(|c: char| c.is_ascii_digit()) {
                let number: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '_')
                    .collect();
                offences.push(format!(
                    "{display}:{}: `spec_{number}` in a name",
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
    if !is_workspace_root(&root) {
        return;
    }
    let chapters = spec_titles(&root);
    assert!(
        !chapters.is_empty(),
        "no spec chapters under {SPEC_DIR}, so every citation below would go unchecked. The \
         spec has moved or been renamed; point {SPEC_DIR} at where it went."
    );

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

            check(
                &mut offences,
                &display,
                line,
                &stem,
                titles,
                quoted_titles_after(&rest[stem.len()..]),
            );
        }

        // The READMEs cite in markdown, which puts the title in the link text
        // and the chapter in the href — behind it, where the scan above has
        // already gone past. Reading the link whole is what makes a retitle
        // fail on a README too.
        for (open, _) in text.match_indices('[') {
            let Some((stem, quoted)) = markdown_link_citation(&text, open) else {
                continue;
            };
            let line = line_of(&text, open);
            let Some(titles) = chapters.get(&stem) else {
                offences.push(format!(
                    "{display}:{line}: `spec/{stem}` names no chapter. The chapters are: {}",
                    chapters.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
                continue;
            };
            check(&mut offences, &display, line, &stem, titles, quoted);
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

/// Record every cited title the chapter does not have.
fn check(
    offences: &mut Vec<String>,
    display: &std::path::Display<'_>,
    line: usize,
    stem: &str,
    titles: &[String],
    quoted: Vec<String>,
) {
    for written in quoted {
        if !titles.contains(&written) {
            offences.push(format!(
                "{display}:{line}: `spec/{stem}` has no section titled \"{written}\". Nearest: {}",
                nearest(titles, &written)
            ));
        }
    }
}

/// The chapter and titles of a markdown link that cites the spec —
/// `[lint "Error vs warning"](https://cairn.kage1020.com/spec/lint/)` — given
/// the offset of its `[`.
///
/// `None` unless the link text opens with a chapter stem that the href then
/// confirms, which is what tells a citation apart from any other link whose
/// text happens to start with a lowercase word.
fn markdown_link_citation(text: &str, open: usize) -> Option<(String, Vec<String>)> {
    let rest = &text[open + 1..];
    let close = rest.find("](")?;
    let label = &rest[..close];
    if label.contains('\n') {
        return None;
    }
    let href = &rest[close + 2..];
    let href = &href[..href.find(')')?];

    let stem: String = label
        .chars()
        .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .collect();
    if stem.is_empty() || !href.contains(&format!("/spec/{stem}/")) {
        return None;
    }
    let titles = quoted_titles_after(&label[stem.len()..]);
    if titles.is_empty() {
        return None;
    }
    Some((stem, titles))
}

/// How far past the chapter a title may start. Long enough for the longest
/// heading in the spec to survive two wrapped lines and their comment markers,
/// short enough that a quote three sentences later is out of reach.
const TITLE_WINDOW: usize = 400;

/// The titles in `` `spec/lint` "Error vs warning" ``, given everything after
/// `lint`.
///
/// Only the punctuation that can sit between the chapter and its title is
/// stepped over — a closing backtick, the `.md` some citations still spell, a
/// markdown link's `]`, and spaces. Anything else means the quote that follows
/// belongs to a different sentence, so there is no title here to check.
///
/// The gap may be a line break, because comments wrap at 100 columns and
/// several titles are most of a sentence: a chapter can end one line and its
/// title open the next, and a title can be split across two. Reading one
/// therefore has to put the lines back together, which [`unwrapped`] does.
///
/// A citation inside a Rust string literal spells its quotes `\"`, so those are
/// folded back to plain quotes first; six user-facing diagnostics cite the spec
/// that way and would otherwise go unread. A section title containing a double
/// quote cannot be written in this form at all; name the chapter and describe
/// the section in prose instead.
///
/// More than one title, because a citation can mean two sections of the same
/// chapter — `"Time model" / "Connection to the IR and phases"` — and checking
/// only the first would let the second rot.
fn quoted_titles_after(rest: &str) -> Vec<String> {
    let window = match rest.char_indices().nth(TITLE_WINDOW) {
        Some((end, _)) => &rest[..end],
        None => rest,
    };
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

/// A citation that a line break ran through, joined back into one line.
///
/// A wrapped comment resumes with its marker — `///`, `//!`, `//`, `#` in a
/// `.crn` example, nothing at all in a README — so the continuation is dropped
/// down to the prose, and every run of whitespace becomes one space. A string
/// literal wraps with a trailing `\`, which swallows the newline and the next
/// line's indent and is no part of the string, so it goes too. What comes back
/// compares equal to the heading it was copied from.
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
            let line = line.trim();
            line.strip_suffix('\\').unwrap_or(line).trim_end()
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
