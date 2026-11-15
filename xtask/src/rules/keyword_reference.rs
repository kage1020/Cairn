//! The Japanese keyword reference holds the entries the English one does, each nested under the
//! same keyword, and each page holds an entry once. Nothing but this rule compares the Japanese
//! entries to the English ones.

use std::collections::BTreeMap;

use cairn_lang_docs::{Page, Part};

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "keyword-reference",
    summary: "spec/syntax's keyword reference lists the same entries, nested alike, once each, in \
              each language",
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

/// An entry: the term it nests in, `None` at the top level, and its own name.
type Entry = (Option<&'static str>, &'static str);

fn check(_repo: &Repo) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut read = Vec::new();
    for (path, file) in PAGES {
        match cairn_lang_docs::page(path) {
            Some(page) => {
                let counted = entries(page);
                findings.extend(repeated(file, &counted));
                read.push(counted);
            }
            None => findings.push(Finding::new(file, None, format!("embeds no page `{path}`"))),
        }
    }
    if let [english, japanese] = read.as_slice() {
        findings.extend(compare(PAGES[1].1, english, japanese));
    }
    findings
}

/// Every term of `page` whose name is code, with the term it nests in, and how many times the
/// page defines it there.
fn entries(page: &Page) -> BTreeMap<Entry, usize> {
    let mut counted = BTreeMap::new();
    for title in page
        .titles()
        .into_iter()
        .filter(|title| title.len() > 1 && title.starts_with('`') && title.ends_with('`'))
    {
        for part in page.cited(title) {
            if let Part::Term { within, .. } = part {
                *counted.entry((within, title)).or_default() += 1;
            }
        }
    }
    counted
}

fn place(owner: Option<&str>) -> String {
    match owner {
        Some(owner) => format!("under {owner}"),
        None => "at the top level".to_owned(),
    }
}

/// A finding at `file` for each entry its page defines more than once in the same place.
fn repeated(file: &str, entries: &BTreeMap<Entry, usize>) -> Vec<Finding> {
    entries
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(&(owner, name), count)| {
            Finding::new(
                file,
                None,
                format!("has {count} entries {name} {}; keep one", place(owner)),
            )
        })
        .collect()
}

/// A finding at `file`, the Japanese page, for each entry one language has and the other lacks.
fn compare(
    file: &str,
    english: &BTreeMap<Entry, usize>,
    japanese: &BTreeMap<Entry, usize>,
) -> Vec<Finding> {
    let missing = english
        .keys()
        .filter(|entry| !japanese.contains_key(*entry))
        .map(|&(owner, name)| {
            format!(
                "has no entry {name} {}, which the English keyword reference has; translate it \
                 there",
                place(owner)
            )
        });
    let extra = japanese
        .keys()
        .filter(|entry| !english.contains_key(*entry))
        .map(|&(owner, name)| {
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
