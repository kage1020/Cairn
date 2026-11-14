//! A published library crate's Rust API is Internal (`spec/compatibility` "Internal"), yet docs.rs
//! and crates.io show it like any other. Its crate doc and its README each open with
//! [`STATEMENT`], word for word, so a reader of either page learns it carries no promise, and one
//! rule holds every copy and the link they share.

use std::path::Path;

use syn::spanned::Spanned;
use syn::{Expr, Lit, Meta};

use super::{Finding, Rule};
use crate::markdown::heading;
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "internal-tier",
    summary: "each published library crate opens its crate doc and README with the statement \
              that its Rust API is Internal",
    check,
};

/// The sentence each published library crate's crate doc and README open with.
const STATEMENT: &str = "This crate's Rust API is in the \
                         [Internal tier](https://cairn.kage1020.com/spec/compatibility/#internal) \
                         and carries no promise: any release may change it.";

/// The site [`STATEMENT`] links into, which serves the pages `cairn-lang-docs` embeds.
const SITE: &str = "https://cairn.kage1020.com/";

/// The directory of the site's page sources.
const PAGES: &str = "website/src/content/docs";

/// The two pages a crate opens with [`STATEMENT`].
#[derive(Clone, Copy)]
enum Place {
    /// The library's crate root, whose `//!` doc docs.rs shows first.
    CrateDoc,
    /// The README, which crates.io shows.
    Readme,
}

fn check(repo: &Repo) -> Vec<Finding> {
    let members = match repo.members() {
        Ok(members) => members,
        Err(error) => return vec![Finding::new("Cargo.toml", None, error)],
    };
    let mut findings: Vec<Finding> = link_finding().into_iter().collect();
    let mut stating = 0;
    for member in members.iter().filter(|member| member.published) {
        let Some(library) = &member.library else {
            continue;
        };
        stating += 1;
        findings.extend(read_opening(repo, library, Place::CrateDoc));
        match &member.readme {
            Some(readme) => findings.extend(read_opening(repo, readme, Place::Readme)),
            None => findings.push(Finding::new(
                member.dir.join("Cargo.toml"),
                None,
                format!(
                    "publishes a library with no README for crates.io to show; add a README.md \
                     whose first paragraph under its title is, word for word: {STATEMENT}"
                ),
            )),
        }
    }
    if stating == 0 {
        findings.push(Finding::new(
            "Cargo.toml",
            None,
            "found no published workspace crate with a library; update this rule if the \
             workspace changed",
        ));
    }
    findings
}

/// The finding for the file at `path` unless it opens with [`STATEMENT`] as `place` requires.
fn read_opening(repo: &Repo, path: &Path, place: Place) -> Option<Finding> {
    match repo.read(path) {
        Ok(text) => opening_finding(path, &text, place),
        Err(error) => Some(Finding::new(path, None, error)),
    }
}

/// The finding for `text`, read from `path`, unless it opens with [`STATEMENT`] as `place`
/// requires.
fn opening_finding(path: &Path, text: &str, place: Place) -> Option<Finding> {
    let (line, opening) = match place {
        Place::CrateDoc => match crate_doc_opening(text) {
            Ok(found) => found,
            Err(error) => {
                let line = error.span().start().line;
                return Some(Finding::new(path, Some(line), error.to_string()));
            }
        },
        Place::Readme => readme_opening(text),
    };
    if opening.as_deref() == Some(STATEMENT) {
        return None;
    }
    let fix = match place {
        Place::CrateDoc => {
            "the crate doc does not open with the Internal-tier statement; make this its first \
             paragraph, word for word, over as many `//!` lines as it takes"
        }
        Place::Readme => {
            "the README does not open with the Internal-tier statement; make this its first \
             paragraph under the title, word for word"
        }
    };
    Some(Finding::new(
        path,
        Some(line),
        format!("{fix}: {STATEMENT}"),
    ))
}

/// The first paragraph of a crate root's `//!` doc, with the line it starts on, or line 1 and
/// `None` when the doc has none.
///
/// # Errors
///
/// The parse error, when `source` is not Rust.
fn crate_doc_opening(source: &str) -> Result<(usize, Option<String>), syn::Error> {
    let file = syn::parse_file(source)?;
    let mut lines = Vec::new();
    for attribute in &file.attrs {
        if let Meta::NameValue(doc) = &attribute.meta
            && doc.path.is_ident("doc")
            && let Expr::Lit(literal) = &doc.value
            && let Lit::Str(text) = &literal.lit
        {
            let start = attribute.span().start().line;
            lines.extend((start..).zip(text.value().split('\n').map(str::to_owned)));
        }
    }
    let paragraph = first_paragraph(lines.iter().map(|(line, text)| (*line, text.as_str())));
    Ok(paragraph.map_or((1, None), |(line, text)| (line, Some(text))))
}

/// The first paragraph under a README's level-1 title, with the line it starts on, or the title's
/// line and `None` when nothing follows it.
fn readme_opening(text: &str) -> (usize, Option<String>) {
    let mut lines = (1..)
        .zip(text.lines())
        .skip_while(|(_, line)| line.trim().is_empty())
        .peekable();
    let mut fallback = 1;
    if let Some(&(line, title)) = lines.peek()
        && heading(title).is_some_and(|(level, _)| level == 1)
    {
        fallback = line;
        lines.next();
    }
    first_paragraph(lines).map_or((fallback, None), |(line, text)| (line, Some(text)))
}

/// The first run of non-blank `lines`, each trimmed and joined by a space, with the line it starts
/// on.
fn first_paragraph<'a>(
    lines: impl IntoIterator<Item = (usize, &'a str)>,
) -> Option<(usize, String)> {
    let mut lines = lines
        .into_iter()
        .skip_while(|(_, text)| text.trim().is_empty());
    let (start, first) = lines.next()?;
    let rest = lines.map_while(|(_, text)| Some(text.trim()).filter(|text| !text.is_empty()));
    let words: Vec<&str> = std::iter::once(first.trim()).chain(rest).collect();
    Some((start, words.join(" ")))
}

/// The page path and `#fragment` of the first link in `statement`, when it links into [`SITE`].
fn link(statement: &str) -> Option<(&str, &str)> {
    let (_, target) = statement.split_once("](")?;
    let (url, _) = target.split_once(')')?;
    url.strip_prefix(SITE)?.split_once("/#")
}

/// A finding unless [`STATEMENT`] links to a section of a page `cairn-lang-docs` embeds.
fn link_finding() -> Option<Finding> {
    let (page, fragment) = link(STATEMENT).expect("STATEMENT links into SITE");
    if cairn_lang_docs::page(page).is_some_and(|found| found.anchored(fragment).is_some()) {
        return None;
    }
    Some(Finding::new(
        Path::new(PAGES).join(format!("{page}.md")),
        None,
        format!(
            "has no section at `#{fragment}`, where the Internal-tier statement every published \
             library crate opens with links; give the tier definition that heading back, or \
             change the link in xtask's `STATEMENT` and in every crate: {STATEMENT}"
        ),
    ))
}

#[cfg(test)]
mod tests;
