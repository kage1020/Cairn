//! Every Markdown page of the Cairn documentation site, the specification among them, embedded at
//! build time, so a tool can show the pages of the release it was built from without a network.
//!
//! A page is found by the path the site serves it at, and a part of one by what a citation quotes
//! (`spec/lint "Error vs warning"`) or by the `#fragment` a link to it carries.

use std::collections::HashMap;

/// One Markdown page of the documentation site.
pub struct Page {
    path: &'static str,
    title: &'static str,
    source: &'static str,
    body: usize,
}

static PAGES: &[Page] = &include!(concat!(env!("OUT_DIR"), "/pages.rs"));

/// Every page: English before Japanese, each directory's index first, then its chapters by number.
pub fn pages() -> &'static [Page] {
    PAGES
}

/// The page the site serves at `/<path>/`, such as `spec/syntax` or `ja/tutorial`.
pub fn page(path: &str) -> Option<&'static Page> {
    PAGES.iter().find(|page| page.path == path)
}

impl Page {
    /// The path the site serves the page at, without the slashes around it.
    pub fn path(&self) -> &'static str {
        self.path
    }

    /// The front matter's title, as written.
    pub fn title(&self) -> &'static str {
        self.title
    }

    /// The Markdown below the front matter.
    pub fn markdown(&self) -> &'static str {
        &self.source[self.body..]
    }

    /// The page's headings in order, leaving out lines inside fenced code.
    pub fn headings(&self) -> Vec<Heading> {
        let markdown = self.markdown();
        let found: Vec<(usize, &'static str, usize)> = lines(markdown)
            .filter(|line| !line.fenced)
            .filter_map(|line| heading(line.text).map(|(level, text)| (level, text, line.start)))
            .collect();
        let mut slugger = Slugger::default();
        found
            .iter()
            .enumerate()
            .map(|(index, &(level, text, start))| {
                let end = found[index + 1..]
                    .iter()
                    .find(|(next, _, _)| *next <= level)
                    .map_or(markdown.len(), |(_, _, next_start)| *next_start);
                Heading {
                    level,
                    text,
                    slug: slugger.slug(text),
                    markdown: &markdown[start..end],
                }
            })
            .collect()
    }

    /// The terms the page's `- **Term.**` list items define, in order.
    pub fn terms(&self) -> Vec<Term> {
        let markdown = self.markdown();
        let all: Vec<Line> = lines(markdown).collect();
        let mut terms = Vec::new();
        for (index, line) in all.iter().enumerate() {
            if line.fenced {
                continue;
            }
            let item = line.text.trim_start();
            let Some(rest) = item.strip_prefix("- **") else {
                continue;
            };
            let Some(close) = rest.find("**") else {
                continue;
            };
            let indent = line.text.len() - item.len();
            let end = all[index + 1..]
                .iter()
                .enumerate()
                .find(|(offset, next)| {
                    if next.text.trim().is_empty() {
                        all[index + 1 + offset..]
                            .iter()
                            .find(|after| !after.text.trim().is_empty())
                            .is_none_or(|after| indent_of(after.text) <= indent)
                    } else {
                        indent_of(next.text) <= indent
                    }
                })
                .map_or(markdown.len(), |(_, next)| next.start);
            terms.push(Term {
                name: rest[..close].trim_end_matches('.'),
                markdown: &markdown[line.start..end],
            });
        }
        terms
    }

    /// Every title a citation of the page may quote: its own, its headings', its terms'.
    pub fn titles(&self) -> Vec<&'static str> {
        let mut titles = vec![strip_number(self.title)];
        titles.extend(self.headings().iter().map(Heading::title));
        titles.extend(self.terms().iter().map(|term| term.name));
        titles
    }

    /// What a citation of the page quoting `title` names: the whole page, a section, or a term.
    pub fn cited(&self, title: &str) -> Option<&'static str> {
        if strip_number(self.title) == title {
            return Some(self.markdown());
        }
        if let Some(heading) = self.headings().into_iter().find(|h| h.title() == title) {
            return Some(heading.markdown);
        }
        self.terms()
            .into_iter()
            .find(|term| term.name == title)
            .map(|term| term.markdown)
    }

    /// The section a link to `#slug` on the page opens at.
    pub fn anchored(&self, slug: &str) -> Option<&'static str> {
        self.headings()
            .into_iter()
            .find(|heading| heading.slug == slug)
            .map(|heading| heading.markdown)
    }
}

/// A heading of a page, with the section it opens.
pub struct Heading {
    /// The number of `#` it is written with.
    pub level: usize,
    /// Its text as written, section number and inline code included.
    pub text: &'static str,
    /// The `#fragment` the site gives it.
    pub slug: String,
    /// The heading's line through the line before the next heading of its level or above.
    pub markdown: &'static str,
}

impl Heading {
    /// The text without a leading section number such as `14.5` or `C.3`, as a citation quotes it.
    pub fn title(&self) -> &'static str {
        strip_number(self.text)
    }
}

/// A term a page defines with a `- **Term.**` list item.
pub struct Term {
    /// The bold text without its closing full stop.
    pub name: &'static str,
    /// The list item, its continuation lines included.
    pub markdown: &'static str,
}

struct Line {
    start: usize,
    text: &'static str,
    /// Inside fenced code, the fence lines themselves included.
    fenced: bool,
}

fn lines(markdown: &'static str) -> impl Iterator<Item = Line> {
    let mut start = 0;
    let mut open: Option<(char, usize)> = None;
    markdown.split_inclusive('\n').map(move |raw| {
        let text = raw.trim_end_matches(['\n', '\r']);
        let line_start = start;
        start += raw.len();
        let marker = fence(text);
        let fenced = match (open, marker) {
            (None, Some(marker)) => {
                open = Some(marker);
                true
            }
            (Some((char, count)), Some((closing, length)))
                if closing == char
                    && length >= count
                    && text.trim().trim_start_matches(char).is_empty() =>
            {
                open = None;
                true
            }
            (open, _) => open.is_some(),
        };
        Line {
            start: line_start,
            text,
            fenced,
        }
    })
}

/// The marker and length of a code fence line.
fn fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let length = trimmed.chars().take_while(|c| *c == marker).count();
    (length >= 3).then_some((marker, length))
}

/// The level and text of an ATX heading line.
fn heading(line: &str) -> Option<(usize, &str)> {
    let line = line.trim_end();
    let unindented = line.trim_start_matches(' ');
    if line.len() - unindented.len() > 3 {
        return None;
    }
    let level = unindented.bytes().take_while(|byte| *byte == b'#').count();
    let rest = &unindented[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let text = rest.trim();
    let unclosed = text.trim_end_matches('#');
    if unclosed.len() < text.len() && (unclosed.is_empty() || unclosed.ends_with([' ', '\t'])) {
        return Some((level, unclosed.trim_end()));
    }
    Some((level, text))
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// `heading` without a leading `14.5` or `C.3` and the space after it.
fn strip_number(heading: &str) -> &str {
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
        return heading;
    }
    after.trim_start()
}

/// GitHub's slugger, which Astro gives each page a fresh one of.
#[derive(Default)]
struct Slugger {
    seen: HashMap<String, usize>,
}

impl Slugger {
    fn slug(&mut self, heading: &str) -> String {
        let base = slug(&plain_text(heading));
        let mut slug = base.clone();
        while self.seen.contains_key(&slug) {
            let count = self.seen.get_mut(&base).expect("the base is seen first");
            *count += 1;
            slug = format!("{base}-{count}");
        }
        self.seen.insert(slug.clone(), 0);
        slug
    }
}

/// The text a heading renders as: code spans without their backticks, links as their label.
fn plain_text(heading: &str) -> String {
    let mut text = String::with_capacity(heading.len());
    let mut rest = heading;
    while let Some(open) = rest.find(['`', '[']) {
        text.push_str(&rest[..open]);
        let tail = &rest[open..];
        if let Some(code) = tail.strip_prefix('`') {
            let Some(close) = code.find('`') else {
                text.push_str(tail);
                return text;
            };
            text.push_str(&code[..close]);
            rest = &code[close + 1..];
        } else if let Some((label, after)) = link(tail) {
            text.push_str(label);
            rest = after;
        } else {
            text.push('[');
            rest = &tail[1..];
        }
    }
    text.push_str(rest);
    text
}

/// The label of an inline link at the start of `text`, and what follows the link.
fn link(text: &str) -> Option<(&str, &str)> {
    let inner = text.strip_prefix('[')?;
    let close = inner.find("](")?;
    let target = &inner[close + 2..];
    let end = target.find(')')?;
    Some((&inner[..close], &target[end + 1..]))
}

fn slug(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ' '))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

#[cfg(test)]
mod tests;
