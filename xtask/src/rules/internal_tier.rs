//! A published library crate's Rust API is Internal (`spec/compatibility` "Internal"), yet docs.rs
//! and crates.io show it like any other. Its crate doc and its README each open with
//! [`STATEMENT`], word for word, so a reader of either page learns it carries no promise, and one
//! rule holds every copy and the link they share. Both pages are read the way rustdoc and
//! crates.io render their Markdown.

use std::ops::Range;
use std::path::Path;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Attribute, Expr, Lit, Meta, Token};

use super::{Finding, Rule};
use crate::repo::{Member, Repo, line_of};

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

/// Why the opening of a crate root's doc cannot be read.
enum Unreadable {
    /// The source does not parse as Rust.
    Parse(syn::Error),
    /// The `doc` attribute on this line, whose text only the compiler can produce, comes before
    /// the first paragraph has ended.
    Opaque(usize),
}

/// What one of a crate root's inner attributes adds to its doc.
enum Fragment {
    /// Doc text, each line with the source line it stands on.
    Text(Vec<(usize, String)>),
    /// Doc text the rule cannot read, such as `include_str!` or a `cfg_attr`.
    Opaque,
    /// Nothing.
    None,
}

fn check(repo: &Repo) -> Vec<Finding> {
    let members = match repo.members() {
        Ok(members) => members,
        Err(error) => return vec![Finding::new("Cargo.toml", None, error)],
    };
    link_finding(STATEMENT)
        .into_iter()
        .chain(findings(&members, |path| repo.read(path)))
        .collect()
}

/// The findings for the published library crates among `members`, whose files `read` returns.
fn findings(members: &[Member], read: impl Fn(&Path) -> Result<String, String>) -> Vec<Finding> {
    let opening = |path: &Path, place| match read(path) {
        Ok(text) => opening_finding(path, &text, place),
        Err(error) => Some(Finding::new(path, None, error)),
    };
    let mut findings = Vec::new();
    let mut stating = 0;
    for member in members.iter().filter(|member| member.published) {
        let Some(library) = &member.library else {
            continue;
        };
        stating += 1;
        findings.extend(opening(library, Place::CrateDoc));
        match &member.readme {
            Some(readme) => findings.extend(opening(readme, Place::Readme)),
            None => findings.push(Finding::new(
                member.dir.join("Cargo.toml"),
                None,
                format!(
                    "publishes a library with no README for crates.io to show; remove \
                     `readme = false` if the manifest sets it, and add a README.md beside it \
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

/// The finding for `text`, read from `path`, unless it opens with [`STATEMENT`] as `place`
/// requires.
fn opening_finding(path: &Path, text: &str, place: Place) -> Option<Finding> {
    let (line, opening) = match place {
        Place::CrateDoc => match crate_doc_opening(text) {
            Ok(found) => found,
            Err(Unreadable::Parse(error)) => {
                let line = error.span().start().line;
                return Some(Finding::new(path, Some(line), error.to_string()));
            }
            Err(Unreadable::Opaque(line)) => {
                return Some(Finding::new(
                    path,
                    Some(line),
                    format!(
                        "this `doc` attribute's text is not a string literal, so the rule cannot \
                         tell what the crate doc opens with; put the Internal-tier statement \
                         before it, word for word, as a `//!` paragraph of its own: {STATEMENT}"
                    ),
                ));
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

/// The first paragraph of a crate root's doc, with the line it starts on, or the line where one
/// belongs and `None`.
///
/// # Errors
///
/// [`Unreadable`] when `source` is not Rust, or an opaque `doc` attribute precedes the paragraph.
fn crate_doc_opening(source: &str) -> Result<(usize, Option<String>), Unreadable> {
    let file = syn::parse_file(source).map_err(Unreadable::Parse)?;
    let mut lines = Vec::new();
    for attribute in &file.attrs {
        match fragment(attribute, source) {
            Fragment::Text(text) => lines.extend(text),
            Fragment::Opaque => {
                let line = attribute.span().start().line;
                return settled_opening(&lines).ok_or(Unreadable::Opaque(line));
            }
            Fragment::None => {}
        }
    }
    Ok(doc_opening(&lines).0)
}

/// What `attribute`, an inner attribute of `source`, adds to the crate doc.
fn fragment(attribute: &Attribute, source: &str) -> Fragment {
    match &attribute.meta {
        Meta::NameValue(doc) if doc.path.is_ident("doc") => {
            let Expr::Lit(literal) = &doc.value else {
                return Fragment::Opaque;
            };
            let Lit::Str(text) = &literal.lit else {
                return Fragment::Opaque;
            };
            let start = attribute.span().start();
            let block = source
                .lines()
                .nth(start.line - 1)
                .is_some_and(|line| line.chars().skip(start.column).take(3).eq("/*!".chars()));
            let lines = (start.line..)
                .zip(text.value().split('\n').map(str::to_owned))
                .collect();
            Fragment::Text(if block { block_doc(lines) } else { lines })
        }
        Meta::List(list) if list.path.is_ident("cfg_attr") => {
            let carries_doc = list
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .is_ok_and(|metas| {
                    metas.iter().skip(1).any(
                        |meta| matches!(meta, Meta::NameValue(doc) if doc.path.is_ident("doc")),
                    )
                });
            if carries_doc {
                Fragment::Opaque
            } else {
                Fragment::None
            }
        }
        _ => Fragment::None,
    }
}

/// The lines of a `/*! */` doc as rustdoc reads them: a first and last line of nothing but `*`
/// dropped, and the `*` every line opens with taken off with the space before it.
fn block_doc(mut lines: Vec<(usize, String)>) -> Vec<(usize, String)> {
    let starry = |line: &str| line.trim().chars().all(|c| c == '*');
    if lines.first().is_some_and(|(_, line)| starry(line)) {
        lines.remove(0);
    }
    if lines.last().is_some_and(|(_, line)| starry(line)) {
        lines.pop();
    }
    if lines
        .iter()
        .all(|(_, line)| line.trim_start().starts_with('*'))
    {
        for (_, line) in &mut lines {
            *line = line.trim_start()[1..].to_owned();
        }
    }
    lines
}

/// The opening of the doc made of `lines`, as [`crate_doc_opening`] returns it, and whether doc
/// text after `lines` could still change it.
fn doc_opening(lines: &[(usize, String)]) -> ((usize, Option<String>), bool) {
    let indent = lines
        .iter()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(_, line)| line.len() - line.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);
    let mut markdown = String::new();
    for (_, line) in lines {
        markdown.push_str(line.get(indent..).unwrap_or_default());
        markdown.push('\n');
    }
    let source_line = |offset| {
        lines
            .get(line_of(&markdown, offset) - 1)
            .map_or(1, |(line, _)| *line)
    };
    match first_paragraph(&markdown, false) {
        Ok(range) => {
            let open = line_of(&markdown, range.end - 1) == lines.len();
            let opening = (source_line(range.start), Some(joined(&markdown[range])));
            (opening, open)
        }
        Err(offset) => {
            let open = lines.iter().all(|(_, line)| line.trim().is_empty());
            ((source_line(offset), None), open)
        }
    }
}

/// The opening of the doc made of `lines`, unless doc text after them could still change it.
fn settled_opening(lines: &[(usize, String)]) -> Option<(usize, Option<String>)> {
    let (opening, open) = doc_opening(lines);
    (!open).then_some(opening)
}

/// The first paragraph under a README's level-1 title, or from the top when it has none, with the
/// line it starts on; else the line of the block standing there, or of the title, or line 1, and
/// `None`.
fn readme_opening(text: &str) -> (usize, Option<String>) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match first_paragraph(text, true) {
        Ok(range) => (line_of(text, range.start), Some(joined(&text[range]))),
        Err(offset) => (line_of(text, offset), None),
    }
}

/// The byte range of the Markdown `text`'s first block, after a level-1 title when `titled` and
/// it opens with one, when that block is a paragraph.
///
/// # Errors
///
/// The offset of the block standing there instead, else of the title, else 0.
fn first_paragraph(text: &str, titled: bool) -> Result<Range<usize>, usize> {
    let mut depth = 0usize;
    let mut title = None;
    for (event, range) in Parser::new(text).into_offset_iter() {
        if depth > 0 {
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => depth -= 1,
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::Paragraph) => return Ok(range),
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) if titled && title.is_none() => {
                title = Some(range.start);
                depth = 1;
            }
            _ => return Err(range.start),
        }
    }
    Err(title.unwrap_or(0))
}

/// The lines of `paragraph`, each trimmed, joined by a space.
fn joined(paragraph: &str) -> String {
    paragraph
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The page path and the fragment, without its `#`, of the first link in `statement`, when it
/// links into [`SITE`].
fn link(statement: &str) -> Option<(&str, &str)> {
    let (_, target) = statement.split_once("](")?;
    let (url, _) = target.split_once(')')?;
    url.strip_prefix(SITE)?.split_once("/#")
}

/// A finding unless `statement` links to a section of a page `cairn-lang-docs` embeds.
///
/// # Panics
///
/// When `statement` has no link into [`SITE`].
fn link_finding(statement: &str) -> Option<Finding> {
    let (page, fragment) = link(statement).expect("the statement links into SITE");
    if cairn_lang_docs::page(page).is_some_and(|found| found.anchored(fragment).is_some()) {
        return None;
    }
    Some(Finding::new(
        Path::new(PAGES).join(format!("{page}.md")),
        None,
        format!(
            "has no section at `#{fragment}`, where the Internal-tier statement every published \
             library crate opens with links; give the tier definition that heading back, or \
             change the link in xtask's `STATEMENT` and in every crate: {statement}"
        ),
    ))
}

#[cfg(test)]
mod tests;
