//! A spec section that specifies syntax the compiler does not build yet opens with a flag, which
//! keeps that syntax Evolving when its chapter turns Stable (`spec/compatibility` "Subject to
//! change"). The flag sets a tier, so each language flags the same sections. A flag anywhere but a
//! section's first paragraph is one a reader would miss, and one written another way is one this
//! rule would.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{Finding, Rule};
use crate::markdown::{heading, lines, section};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "subject-to-change",
    summary: "a subject-to-change flag is written as `spec/compatibility` defines it and opens its \
              section, and both languages flag the same spec sections",
    check,
};

/// A spec directory, the words its flag opens with, and the flag as a paragraph starts with it.
struct Language {
    dir: &'static str,
    words: &'static str,
    flag: &'static str,
}

const ENGLISH: Language = Language {
    dir: "website/src/content/docs/spec",
    words: "Subject to change",
    flag: "**Subject to change.**",
};

const JAPANESE: Language = Language {
    dir: "website/src/content/docs/ja/spec",
    words: "変更の余地あり",
    flag: "**変更の余地あり。**",
};

/// The page of each spec directory whose `####` section titled with the flag's words defines it.
const DEFINITION: &str = "compatibility.md";

/// A heading of a page, and whether a flag opens the section under it.
#[derive(Debug, PartialEq, Eq)]
struct Section {
    line: usize,
    heading: String,
    flagged: bool,
}

/// A page's sections in order, and the lines of the blocks that look like a flag but are not one.
#[derive(Debug, PartialEq, Eq)]
struct Flags {
    sections: Vec<Section>,
    /// Flags that are not the first paragraph under a heading.
    misplaced: Vec<usize>,
    /// Blocks that open with the flag's words but are not written as the flag.
    malformed: Vec<usize>,
}

/// A spec page and the flags read from it.
struct Page<'a> {
    path: &'a Path,
    language: &'static Language,
    flags: Flags,
}

/// An English spec page and its Japanese twin, either of which may be missing.
type Pair<'a> = (Option<&'a Path>, Option<&'a Path>);

fn check(repo: &Repo) -> Vec<Finding> {
    let (pairs, mut findings) = pair(repo.files());
    for (english, japanese) in pairs {
        let english = english.and_then(|path| read(repo, path, &ENGLISH, &mut findings));
        let japanese = japanese.and_then(|path| read(repo, path, &JAPANESE, &mut findings));
        findings.extend(report(english.as_ref(), japanese.as_ref()));
    }
    for language in [&ENGLISH, &JAPANESE] {
        findings.extend(definition(repo, language));
    }
    findings
}

/// The page at `path` with its flags, or `None` with a finding when it cannot be read.
fn read<'a>(
    repo: &Repo,
    path: &'a Path,
    language: &'static Language,
    findings: &mut Vec<Finding>,
) -> Option<Page<'a>> {
    match repo.read(path) {
        Ok(text) => Some(Page {
            path,
            language,
            flags: flags(&text, language),
        }),
        Err(error) => {
            findings.push(Finding::new(path, None, error));
            None
        }
    }
}

/// Whether `path` is a page directly inside `language`'s spec directory.
fn in_spec(path: &Path, language: &Language) -> bool {
    path.parent() == Some(Path::new(language.dir)) && path.extension() == Some(OsStr::new("md"))
}

/// The spec pages among `files`, each English page with its Japanese twin, then each Japanese page
/// with none, and a finding for a language with no page.
fn pair(files: &[PathBuf]) -> (Vec<Pair<'_>>, Vec<Finding>) {
    let pages = |language: &Language| -> Vec<&Path> {
        files
            .iter()
            .map(PathBuf::as_path)
            .filter(|path| in_spec(path, language))
            .collect()
    };
    let english = pages(&ENGLISH);
    let japanese = pages(&JAPANESE);
    let findings = [(&ENGLISH, &english), (&JAPANESE, &japanese)]
        .into_iter()
        .filter(|(_, pages)| pages.is_empty())
        .map(|(language, _)| {
            Finding::new(
                language.dir,
                None,
                "found no spec page; update this rule's directory if the spec moved",
            )
        })
        .collect();
    let mut pairs: Vec<Pair<'_>> = english
        .iter()
        .map(|&page| (Some(page), twin(page, &japanese)))
        .collect();
    pairs.extend(
        japanese
            .iter()
            .filter(|&&page| twin(page, &english).is_none())
            .map(|&page| (None, Some(page))),
    );
    (pairs, findings)
}

/// The page among `others` with `page`'s file name.
fn twin<'a>(page: &Path, others: &[&'a Path]) -> Option<&'a Path> {
    others
        .iter()
        .copied()
        .find(|other| other.file_name() == page.file_name())
}

/// Every section of `text` with whether `language`'s flag opens it, fenced code left out.
fn flags(text: &str, language: &Language) -> Flags {
    let mut sections: Vec<Section> = Vec::new();
    let mut misplaced = Vec::new();
    let mut malformed = Vec::new();
    let mut opens_paragraph = true;
    let mut first_block = false;
    for (index, line) in lines(text).enumerate() {
        if line.fenced {
            first_block = false;
            opens_paragraph = true;
            continue;
        }
        if line.text.trim().is_empty() {
            opens_paragraph = true;
            continue;
        }
        if let Some((_, title)) = heading(line.text) {
            sections.push(Section {
                line: index + 1,
                heading: title.to_owned(),
                flagged: false,
            });
            first_block = true;
            opens_paragraph = true;
            continue;
        }
        let (stripped, container) = unmarked(line.text);
        if opens_paragraph || container {
            if opens_paragraph && line.text.starts_with(language.flag) {
                match sections.last_mut() {
                    Some(section) if first_block => section.flagged = true,
                    _ => misplaced.push(index + 1),
                }
            } else if stripped.starts_with(language.words) {
                malformed.push(index + 1);
            }
            first_block = false;
        }
        opens_paragraph = false;
    }
    Flags {
        sections,
        misplaced,
        malformed,
    }
}

/// `line` without the indentation, block quote and list markers and emphasis it opens with, and
/// whether a block quote or list marker opened it.
fn unmarked(line: &str) -> (&str, bool) {
    let mut rest = line;
    let mut container = false;
    loop {
        let trimmed = rest.trim_start();
        let item = trimmed
            .strip_prefix(['-', '*', '+'])
            .or_else(|| {
                let digits = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
                (digits.len() < trimmed.len())
                    .then(|| digits.strip_prefix(['.', ')']))
                    .flatten()
            })
            .filter(|after| after.starts_with([' ', '\t']));
        match trimmed.strip_prefix('>').or(item) {
            Some(after) => {
                rest = after;
                container = true;
            }
            None => return (trimmed.trim_start_matches(['*', '_']), container),
        }
    }
}

/// What is wrong on a page pair: each page's stray flags, then the sections only one page flags.
fn report(english: Option<&Page>, japanese: Option<&Page>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for page in [english, japanese].into_iter().flatten() {
        for &line in &page.flags.misplaced {
            findings.push(Finding::new(
                page.path,
                Some(line),
                "this subject-to-change flag is not the first paragraph under a heading; move it \
                 directly under the heading of the section whose syntax it names",
            ));
        }
        for &line in &page.flags.malformed {
            findings.push(Finding::new(
                page.path,
                Some(line),
                format!(
                    "this block opens with \"{}\" but is not written as the flag; start an \
                     unindented paragraph with `{}`, or reword it",
                    page.language.words, page.language.flag,
                ),
            ));
        }
    }
    if let (Some(english), Some(japanese)) = (english, japanese) {
        findings.extend(compare(english, japanese));
    }
    findings
}

/// The sections only one page of a pair flags, once either page flags any.
fn compare(english: &Page, japanese: &Page) -> Vec<Finding> {
    let flagged = |page: &Page| page.flags.sections.iter().any(|section| section.flagged);
    if !flagged(english) && !flagged(japanese) {
        return Vec::new();
    }
    let (english_sections, japanese_sections) = (&english.flags.sections, &japanese.flags.sections);
    if english_sections.len() != japanese_sections.len() {
        return vec![Finding::new(
            japanese.path,
            None,
            format!(
                "has {} headings and {} has {}, so their flags cannot be paired; give the two \
                 pages the same headings",
                japanese_sections.len(),
                english.path.display(),
                english_sections.len(),
            ),
        )];
    }
    let mut findings = Vec::new();
    for (english_section, japanese_section) in english_sections.iter().zip(japanese_sections) {
        let (unflagged, page, flagged, other) =
            match (english_section.flagged, japanese_section.flagged) {
                (true, false) => (japanese_section, japanese, english_section, english),
                (false, true) => (english_section, english, japanese_section, japanese),
                _ => continue,
            };
        findings.push(Finding::new(
            page.path,
            Some(unflagged.line),
            format!(
                "\"{}\" is not flagged subject to change, but \"{}\" in {} is; flag both \
                 sections or neither",
                unflagged.heading,
                flagged.heading,
                other.path.display(),
            ),
        ));
    }
    findings
}

/// A finding unless `language`'s compatibility page defines the flag this rule looks for.
fn definition(repo: &Repo, language: &Language) -> Option<Finding> {
    let Some(path) = repo
        .files()
        .iter()
        .find(|path| in_spec(path, language) && path.file_name() == Some(OsStr::new(DEFINITION)))
    else {
        return Some(Finding::new(
            language.dir,
            None,
            format!("found no {DEFINITION}, which defines the subject-to-change flag"),
        ));
    };
    match repo.read(path) {
        Ok(text) if defines(&text, language) => None,
        Ok(_) => Some(Finding::new(
            path,
            None,
            format!(
                "has no `#### {}` section showing `{}`, the flag this rule looks for; update the \
                 rule to the flag the page defines",
                language.words, language.flag,
            ),
        )),
        Err(error) => Some(Finding::new(path, None, error)),
    }
}

/// Whether `text` has a `####` section titled with `language`'s words that shows its flag.
fn defines(text: &str, language: &Language) -> bool {
    section(text, 4, |title| title == language.words)
        .is_some_and(|lines| lines.iter().any(|(_, line)| line.contains(language.flag)))
}

#[cfg(test)]
mod tests;
