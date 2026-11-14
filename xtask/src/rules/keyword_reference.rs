//! The Japanese keyword reference holds the entries the English one does, each nested under the
//! same keyword. The language server quotes the English page and a test holds that one to core's
//! keyword table, so nothing else notices a Japanese entry missing, extra or misplaced.

use std::collections::BTreeSet;

use cairn_lang_docs::{Page, Part};

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "keyword-reference",
    summary: "spec/syntax's keyword reference lists the same entries, nested alike, in each \
              language",
    check,
};

/// The English page and the Japanese one, each with the file it is built from.
const PAGES: [(&str, &str); 2] = [
    ("spec/syntax", "website/src/content/docs/spec/syntax.md"),
    (
        "ja/spec/syntax",
        "website/src/content/docs/ja/spec/syntax.md",
    ),
];

/// An entry: the code term it nests in, `None` at the top level, and its own name.
type Entry = (Option<&'static str>, &'static str);

fn check(_repo: &Repo) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut read = Vec::new();
    for (path, file) in PAGES {
        match cairn_lang_docs::page(path) {
            Some(page) => read.push(entries(page)),
            None => findings.push(Finding::new(file, None, format!("embeds no page `{path}`"))),
        }
    }
    if let [english, japanese] = read.as_slice() {
        findings.extend(compare(PAGES[1].1, english, japanese));
    }
    findings
}

/// Every term of `page` whose name is code, each with the code term it nests in.
fn entries(page: &Page) -> BTreeSet<Entry> {
    let terms: Vec<(&'static str, &'static str)> = page
        .titles()
        .into_iter()
        .filter(|title| title.len() > 1 && title.starts_with('`') && title.ends_with('`'))
        .flat_map(|title| {
            page.cited(title)
                .into_iter()
                .filter_map(move |part| match part {
                    Part::Term { markdown, .. } => Some((title, markdown)),
                    Part::Page | Part::Section(_) => None,
                })
        })
        .collect();
    terms
        .iter()
        .map(|&(name, markdown)| {
            let owner = terms
                .iter()
                .filter(|(_, outer)| outer.len() > markdown.len() && nests_in(markdown, outer))
                .min_by_key(|(_, outer)| outer.len())
                .map(|&(owner, _)| owner);
            (owner, name)
        })
        .collect()
}

/// Whether `inner` lies inside `outer`; both slice one page's Markdown, so that is nesting.
fn nests_in(inner: &str, outer: &str) -> bool {
    let inner = inner.as_bytes().as_ptr_range();
    let outer = outer.as_bytes().as_ptr_range();
    outer.start <= inner.start && inner.end <= outer.end
}

/// A finding at `file`, the Japanese page, for each entry one language has and the other lacks.
fn compare(file: &str, english: &BTreeSet<Entry>, japanese: &BTreeSet<Entry>) -> Vec<Finding> {
    let place = |owner: Option<&str>| match owner {
        Some(owner) => format!("under {owner}"),
        None => "at the top level".to_owned(),
    };
    let missing = english.difference(japanese).map(|&(owner, name)| {
        format!(
            "has no entry {name} {}, which the English keyword reference has; translate it \
             there",
            place(owner)
        )
    });
    let extra = japanese.difference(english).map(|&(owner, name)| {
        format!(
            "has an entry {name} {} the English keyword reference does not; add it there or \
             drop it here",
            place(owner)
        )
    });
    missing
        .chain(extra)
        .map(|message| Finding::new(file, None, message))
        .collect()
}

#[cfg(test)]
mod tests;
