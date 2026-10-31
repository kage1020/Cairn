//! The tier-1 vocabulary is the abstract tokens the built-in `materials` catalogs declare, and the
//! spec lists it because that list is what turns Stable (`spec/materials-themes` "Tier-1 tokens").
//! A token a catalog gains without a row would be Stable by a definition nobody reads, and a row
//! for a token a catalog lacks is a promise no target keeps.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use super::{Finding, Rule};
use crate::markdown::{first_cell_spans, section};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "tier-one-tokens",
    summary: "the spec's tier-1 table lists exactly the tokens each built-in materials catalog \
              declares, in each language",
    check,
};

/// Each edition's built-in catalog.
const CATALOGS: &[&str] = &[
    "crates/cairn-lang-formats/registry-data/java/materials.json",
    "crates/cairn-lang-formats/registry-data/bedrock/materials.json",
];

/// The spec directory in each language, with the title of the tier-1 section. The chapter file is
/// joined on apart, since a path spelled whole would read as a citation to `spec-citations`.
const TABLES: &[(&str, &str)] = &[
    ("website/src/content/docs/spec", "Tier-1 tokens"),
    ("website/src/content/docs/ja/spec", "tier-1 のトークン"),
];

const MATERIALS_CHAPTER: &str = "materials-themes.md";

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
    for (dir, title) in TABLES {
        let path = Path::new(dir).join(MATERIALS_CHAPTER);
        let rows = match repo.read(&path).and_then(|text| table_tokens(&text, title)) {
            Ok(rows) => rows,
            Err(error) => {
                findings.push(Finding::new(&path, None, error));
                continue;
            }
        };
        findings.extend(compare(&path, &rows, &catalogs));
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

/// Each token the first cells of the `### {title}` section's table name, `@` dropped, with its line.
fn table_tokens(text: &str, title: &str) -> Result<Vec<(usize, String)>, String> {
    let lines = section(text, 3, |heading| heading == title)
        .ok_or_else(|| format!("has no `### {title}` section"))?;
    let mut tokens = Vec::new();
    for (line, row) in lines {
        let Some(spans) = first_cell_spans(row)? else {
            continue;
        };
        tokens.extend(
            spans
                .into_iter()
                .map(|span| (line, span.trim_start_matches('@').to_owned())),
        );
    }
    if tokens.is_empty() {
        return Err(format!(
            "the `### {title}` section's table names no token in its first column"
        ));
    }
    Ok(tokens)
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
                        "{} declares `{token}`, which the tier-1 table does not list; list it, \
                         and in every edition's catalog",
                        catalog.display()
                    ),
                ));
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(findings: &[Finding]) -> Vec<String> {
        findings
            .iter()
            .map(|finding| finding.render(RULE.name))
            .collect()
    }

    const CHAPTER: &str = "\
## 7.2 Canonical vocabulary

| Kind | Example |
|---|---|
| `@not.listed` | A table above the section. |

### Tier-1 tokens

| Tokens | For |
|---|---|
| `@floor.a`, `@floor.b` | Floors |

```text
| `@fenced.row` | Not a row. |
```

## 7.3 Next
";

    #[test]
    fn the_tier_one_table_is_read_from_its_own_section_with_each_rows_line() {
        assert_eq!(
            table_tokens(CHAPTER, "Tier-1 tokens"),
            Ok(vec![(11, "floor.a".to_owned()), (11, "floor.b".to_owned())])
        );
        assert_eq!(
            table_tokens(CHAPTER, "Missing"),
            Err("has no `### Missing` section".to_owned())
        );
    }

    #[test]
    fn a_catalogs_tokens_are_read_from_its_entries() {
        let catalog = r#"{ "entries": [ { "token": "floor.a", "block": "oak_planks" } ] }"#;
        assert_eq!(
            catalog_tokens(catalog),
            Ok(BTreeSet::from(["floor.a".to_owned()]))
        );
        assert_eq!(
            catalog_tokens(r#"{ "entries": [ { "block": "oak_planks" } ] }"#),
            Err(r#"has an entry with no `token` string: {"block":"oak_planks"}"#.to_owned())
        );
    }

    #[test]
    fn a_token_on_one_side_only_is_named_against_the_catalog_that_differs() {
        let rows = [(11, "floor.a".to_owned()), (11, "floor.b".to_owned())];
        let java = BTreeSet::from(["floor.a".to_owned(), "floor.b".to_owned()]);
        let bedrock = BTreeSet::from(["floor.a".to_owned(), "floor.c".to_owned()]);
        let catalogs = [
            (Path::new("java.json"), java),
            (Path::new("bedrock.json"), bedrock),
        ];
        assert_eq!(
            rendered(&compare(Path::new("spec.md"), &rows, &catalogs)),
            [
                "spec.md:11: tier-one-tokens: the tier-1 table lists `@floor.b`, which \
                 bedrock.json does not declare; add it to every catalog or drop the row's entry",
                "spec.md: tier-one-tokens: bedrock.json declares `floor.c`, which the tier-1 \
                 table does not list; list it, and in every edition's catalog",
            ]
        );
    }
}
