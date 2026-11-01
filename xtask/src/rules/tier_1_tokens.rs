//! Each language's tier-1 table lists exactly the tokens each built-in `materials` catalog
//! declares: a catalog token without a row turns Stable unannounced, and a row a catalog lacks
//! names a token that edition cannot resolve.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use super::{Finding, Rule};
use crate::markdown::{first_cell_spans, section};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "tier-1-tokens",
    summary: "the spec's tier-1 table lists exactly the tokens each built-in materials catalog \
              declares, in each language",
    check,
};

/// Each edition's built-in catalog.
const CATALOGS: &[&str] = &[
    "crates/cairn-lang-formats/registry-data/java/materials.json",
    "crates/cairn-lang-formats/registry-data/bedrock/materials.json",
];

/// The materials chapter in each language, with the title of its tier-1 section.
const TABLES: &[(&str, &str)] = &[
    (
        "website/src/content/docs/spec/materials-themes.md",
        "Tier-1 tokens",
    ),
    (
        "website/src/content/docs/ja/spec/materials-themes.md",
        "tier-1 のトークン",
    ),
];

fn check(repo: &Repo) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut catalogs = Vec::new();
    for path in CATALOGS {
        let path = Path::new(path);
        match repo.read(path).and_then(|text| catalog_tokens(&text)) {
            Ok(tokens) => catalogs.push((path, tokens)),
            Err(error) => findings.push(Finding::new(path, None, error)),
        }
    }
    for (path, title) in TABLES {
        let path = Path::new(path);
        match repo.read(path) {
            Ok(text) => {
                let rows = table_tokens(path, &text, title, &mut findings);
                findings.extend(compare(path, &rows, &catalogs));
            }
            Err(error) => findings.push(Finding::new(path, None, error)),
        }
    }
    findings
}

/// The tokens a `materials.json` declares.
fn catalog_tokens(text: &str) -> Result<BTreeSet<String>, String> {
    let catalog: Value =
        serde_json::from_str(text).map_err(|error| format!("is not valid JSON: {error}"))?;
    let entries = catalog
        .get("entries")
        .and_then(Value::as_array)
        .ok_or("has no `entries` array")?;
    entries
        .iter()
        .map(|entry| {
            entry
                .get("token")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("has an entry with no `token` string: {entry}"))
        })
        .collect()
}

/// Whether `line` is a table row.
fn is_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// Each token the first cells of the first table in the `### {title}` section name, `@` dropped,
/// with its line, and a finding for each first cell or span that does not read as tokens.
fn table_tokens(
    path: &Path,
    text: &str,
    title: &str,
    findings: &mut Vec<Finding>,
) -> Vec<(usize, String)> {
    let Some(lines) = section(text, 3, |heading| heading == title) else {
        findings.push(Finding::new(
            path,
            None,
            format!("has no `### {title}` section"),
        ));
        return Vec::new();
    };
    let rows = lines
        .into_iter()
        .skip_while(|(_, line)| !is_row(line))
        .take_while(|(_, line)| is_row(line));
    let mut tokens = Vec::new();
    let mut unread = false;
    for (line, row) in rows {
        let spans = match first_cell_spans(row) {
            Ok(spans) => spans.into_iter().flatten(),
            Err(error) => {
                findings.push(Finding::new(path, Some(line), error));
                unread = true;
                continue;
            }
        };
        for span in spans {
            if let Some(token) = span.strip_prefix('@') {
                tokens.push((line, token.to_owned()));
            } else {
                findings.push(Finding::new(
                    path,
                    Some(line),
                    format!(
                        "the tier-1 table lists `{span}`, which is not written as a token; \
                         spell it `@{span}`"
                    ),
                ));
                unread = true;
            }
        }
    }
    if tokens.is_empty() && !unread {
        findings.push(Finding::new(
            path,
            None,
            format!("the first table of `### {title}` names no token in its first column"),
        ));
    }
    tokens
}

/// What the table at `path` lists that a catalog does not declare, and the reverse.
fn compare(
    path: &Path,
    rows: &[(usize, String)],
    catalogs: &[(&Path, BTreeSet<String>)],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (catalog, tokens) in catalogs {
        for (line, token) in rows {
            if !tokens.contains(token) {
                findings.push(Finding::new(
                    path,
                    Some(*line),
                    format!(
                        "the tier-1 table lists `@{token}`, which {} does not declare; add it to \
                         every catalog or drop the row's entry",
                        catalog.display()
                    ),
                ));
            }
        }
        for token in tokens {
            if !rows.iter().any(|(_, listed)| listed == token) {
                findings.push(Finding::new(
                    path,
                    None,
                    format!(
                        "{catalog} declares `@{token}`, which the tier-1 table does not list; add \
                         a row for it and declare it in every edition's catalog, or drop it from \
                         {catalog}",
                        catalog = catalog.display()
                    ),
                ));
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests;
