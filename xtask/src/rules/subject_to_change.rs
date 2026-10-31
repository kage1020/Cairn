//! A spec section that specifies syntax the compiler does not build yet opens with a flag, which
//! keeps that syntax Evolving while the rest of its chapter is Stable (`spec/compatibility`
//! "Subject to change"). The flag sets a tier, so each language flags the same sections, and a flag
//! anywhere but a section's first paragraph is one a reader would miss.

use std::path::{Path, PathBuf};

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "subject-to-change",
    summary: "a subject-to-change flag opens its section, and both languages flag the same spec \
              sections",
    check,
};

/// A spec directory, with the words a flag paragraph opens with there.
struct Language {
    dir: &'static str,
    flag: &'static str,
}

const ENGLISH: Language = Language {
    dir: "website/src/content/docs/spec",
    flag: "**Subject to change.**",
};

const JAPANESE: Language = Language {
    dir: "website/src/content/docs/ja/spec",
    flag: "**変更の余地あり。**",
};

/// A heading of a page, and whether a flag opens the section under it.
#[derive(Debug, PartialEq, Eq)]
struct Section {
    line: usize,
    heading: String,
    flagged: bool,
}

/// A page's sections in order, and the lines of the flags that open none of them.
#[derive(Debug, PartialEq, Eq)]
struct Flags {
    sections: Vec<Section>,
    misplaced: Vec<usize>,
}

fn check(repo: &Repo) -> Vec<Finding> {
    let pages: Vec<PathBuf> = repo
        .files_in(Path::new(ENGLISH.dir), &["md"])
        .filter(|path| path.parent() == Some(Path::new(ENGLISH.dir)))
        .cloned()
        .collect();
    if pages.is_empty() {
        return vec![Finding::new(
            ENGLISH.dir,
            None,
            "found no spec page; update ENGLISH and JAPANESE in xtask if the spec moved",
        )];
    }
    let mut findings = Vec::new();
    for english in pages {
        let Some(name) = english.file_name() else {
            continue;
        };
        let japanese = Path::new(JAPANESE.dir).join(name);
        if !repo.files().contains(&japanese) {
            continue;
        }
        let mut read = |path: &Path, language: &Language| match repo.read(path) {
            Ok(text) => Some(flags(&text, language.flag)),
            Err(error) => {
                findings.push(Finding::new(path, None, error));
                None
            }
        };
        let (Some(english_flags), Some(japanese_flags)) =
            (read(&english, &ENGLISH), read(&japanese, &JAPANESE))
        else {
            continue;
        };
        findings.extend(compare(
            (&english, &english_flags),
            (&japanese, &japanese_flags),
        ));
    }
    findings
}

/// Every section of `text` with whether `flag` opens it, fenced code left out.
fn flags(text: &str, flag: &str) -> Flags {
    let mut sections: Vec<Section> = Vec::new();
    let mut misplaced = Vec::new();
    let mut fence: Option<&str> = None;
    let mut opens_paragraph = true;
    let mut first_block = false;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"]
            .into_iter()
            .find(|marker| trimmed.starts_with(marker));
        if let Some(open) = fence {
            if marker == Some(open) {
                fence = None;
                opens_paragraph = true;
            }
            continue;
        }
        if let Some(marker) = marker {
            fence = Some(marker);
            first_block = false;
            continue;
        }
        if line.trim().is_empty() {
            opens_paragraph = true;
            continue;
        }
        if let Some(heading) = heading(line) {
            sections.push(Section {
                line: index + 1,
                heading: heading.to_owned(),
                flagged: false,
            });
            first_block = true;
            opens_paragraph = true;
            continue;
        }
        if opens_paragraph && line.starts_with(flag) {
            match sections.last_mut() {
                Some(section) if first_block => section.flagged = true,
                _ => misplaced.push(index + 1),
            }
        }
        if opens_paragraph {
            first_block = false;
        }
        opens_paragraph = false;
    }
    Flags {
        sections,
        misplaced,
    }
}

/// The text of `line` if it is an ATX heading.
fn heading(line: &str) -> Option<&str> {
    let level = line.chars().take_while(|c| *c == '#').count();
    let text = line[level..].strip_prefix(' ')?;
    (level > 0).then_some(text.trim())
}

/// What differs between an English page's flags and its Japanese page's, and every misplaced flag.
fn compare(
    (english, english_flags): (&Path, &Flags),
    (japanese, japanese_flags): (&Path, &Flags),
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (path, flags) in [(english, english_flags), (japanese, japanese_flags)] {
        for &line in &flags.misplaced {
            findings.push(Finding::new(
                path,
                Some(line),
                "a subject-to-change flag opens its section; make it the first paragraph under the \
                 heading of the section whose syntax it names",
            ));
        }
    }
    if english_flags.sections.len() != japanese_flags.sections.len() {
        findings.push(Finding::new(
            japanese,
            None,
            format!(
                "has {} headings and {} has {}, so their flags cannot be paired; give the two \
                 pages the same headings",
                japanese_flags.sections.len(),
                english.display(),
                english_flags.sections.len(),
            ),
        ));
        return findings;
    }
    let pairs = english_flags.sections.iter().zip(&japanese_flags.sections);
    for (english_section, japanese_section) in pairs {
        let (unflagged, path, flagged, other) =
            match (english_section.flagged, japanese_section.flagged) {
                (true, false) => (japanese_section, japanese, english_section, english),
                (false, true) => (english_section, english, japanese_section, japanese),
                _ => continue,
            };
        findings.push(Finding::new(
            path,
            Some(unflagged.line),
            format!(
                "\"{}\" is not flagged subject to change, but \"{}\" in {} is; flag both \
                 sections or neither",
                unflagged.heading,
                flagged.heading,
                other.display(),
            ),
        ));
    }
    findings
}

#[cfg(test)]
mod tests;
