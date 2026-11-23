//! The furniture library the compiler ships is the source each language's spec prints: a body
//! builds what `library.crn` says, and a reader learns what the spec's block says.

use std::path::Path;

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "furniture-library",
    summary: "the furniture library core ships is the block each language's spec prints",
    check,
};

const LIBRARY: &str = "crates/cairn-lang-core/src/furniture/library.crn";

const CHAPTERS: &[&str] = &[
    "website/src/content/docs/spec/components-editing-sites.md",
    "website/src/content/docs/ja/spec/components-editing-sites.md",
];

/// The line that opens the library's block.
const OPENS: &str = "theme furniture:";

fn check(repo: &Repo) -> Vec<Finding> {
    let library = match repo.read(Path::new(LIBRARY)) {
        Ok(library) => library,
        Err(error) => return vec![Finding::new(LIBRARY, None, error)],
    };
    let mut findings = Vec::new();
    for chapter in CHAPTERS {
        match repo.read(Path::new(chapter)) {
            Ok(text) => findings.extend(compare(Path::new(chapter), &text, &library)),
            Err(error) => findings.push(Finding::new(*chapter, None, error)),
        }
    }
    findings
}

/// The finding on `chapter` when its fenced block opening with [`OPENS`] is not `library`.
fn compare(chapter: &Path, text: &str, library: &str) -> Option<Finding> {
    let Some((line, block)) = printed(text) else {
        return Some(Finding::new(
            chapter,
            None,
            format!("prints no fenced block opening with `{OPENS}`; print {LIBRARY} there"),
        ));
    };
    let shipped: Vec<&str> = library.lines().collect();
    let first_apart = block
        .iter()
        .zip(&shipped)
        .position(|(printed, shipped)| printed != shipped)
        .or_else(|| (block.len() != shipped.len()).then(|| block.len().min(shipped.len())))?;
    Some(Finding::new(
        chapter,
        Some(line + first_apart),
        format!(
            "the library block parts from {LIBRARY} at its line {}; print the file as it is",
            first_apart + 1,
        ),
    ))
}

/// The line number of the first line inside the block opening with [`OPENS`], and its lines.
fn printed(text: &str) -> Option<(usize, Vec<&str>)> {
    let lines: Vec<&str> = text.lines().collect();
    let opens = lines
        .windows(2)
        .position(|pair| pair[0].starts_with("```") && pair[1] == OPENS)?;
    let body = &lines[opens + 1..];
    let closes = body.iter().position(|line| line.starts_with("```"))?;
    Some((opens + 2, body[..closes].to_vec()))
}

#[cfg(test)]
mod tests;
