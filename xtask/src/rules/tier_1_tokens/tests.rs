use super::*;

fn rendered(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|finding| finding.render(RULE.name))
        .collect()
}

/// The tokens and findings `table_tokens` reads from `text`'s `### Tier-1 tokens` section.
fn read(text: &str) -> (Vec<(usize, String)>, Vec<String>) {
    let mut findings = Vec::new();
    let tokens = table_tokens(Path::new("spec.md"), text, "Tier-1 tokens", &mut findings);
    (tokens, rendered(&findings))
}

fn rows(rows: &[(usize, &str)]) -> Vec<(usize, String)> {
    rows.iter()
        .map(|&(line, token)| (line, token.to_owned()))
        .collect()
}

#[test]
fn the_tier_1_table_is_the_first_table_of_its_own_section() {
    let chapter = "\
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

#### Examples

| `@floor.example` | A second table. |

## 7.3 Next
";
    assert_eq!(
        read(chapter),
        (rows(&[(11, "floor.a"), (11, "floor.b")]), vec![])
    );
}

#[test]
fn a_row_cell_not_read_as_tokens_is_reported_at_its_line_and_the_other_rows_still_read() {
    let chapter = "\
### Tier-1 tokens

| Tokens | For |
|---|---|
| `@floor.a`, `floor.b` | No `@` on the second. |
| `@floor.c | An unpaired backtick. |
| `@@floor.d` | Two `@`. |
| `@floor.e` | After the rows above. |
";
    assert_eq!(
        read(chapter),
        (
            rows(&[(5, "floor.a"), (7, "@floor.d"), (8, "floor.e")]),
            vec![
                "spec.md:5: tier-1-tokens: the tier-1 table lists `floor.b`, which is not \
                 written as a token; spell it `@floor.b`"
                    .to_owned(),
                "spec.md:6: tier-1-tokens: unpaired backtick in a table row's first cell: \
                 \"| `@floor.c | An unpaired backtick. |\""
                    .to_owned(),
            ]
        )
    );
}

#[test]
fn a_missing_section_or_a_table_with_no_token_is_reported() {
    assert_eq!(
        read("### Other\n\n| `@floor.a` | Floors |\n"),
        (
            vec![],
            vec!["spec.md: tier-1-tokens: has no `### Tier-1 tokens` section".to_owned()]
        )
    );
    assert_eq!(
        read("### Tier-1 tokens\n\n| Tokens | For |\n|---|---|\n"),
        (
            vec![],
            vec![
                "spec.md: tier-1-tokens: the first table of `### Tier-1 tokens` names no token \
                 in its first column"
                    .to_owned()
            ]
        )
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
fn each_catalog_is_compared_with_the_table_on_its_own() {
    let rows = rows(&[(11, "floor.a"), (11, "floor.b")]);
    let java = BTreeSet::from(["floor.a".to_owned(), "floor.d".to_owned()]);
    let bedrock = BTreeSet::from(["floor.a".to_owned(), "floor.c".to_owned()]);
    let catalogs = [
        (Path::new("java.json"), java),
        (Path::new("bedrock.json"), bedrock),
    ];
    assert_eq!(
        rendered(&compare(Path::new("spec.md"), &rows, &catalogs)),
        [
            "spec.md:11: tier-1-tokens: the tier-1 table lists `@floor.b`, which java.json does \
             not declare; add it to every catalog or drop the row's entry",
            "spec.md: tier-1-tokens: java.json declares `@floor.d`, which the tier-1 table does \
             not list; add a row for it and declare it in every edition's catalog, or drop it \
             from java.json",
            "spec.md:11: tier-1-tokens: the tier-1 table lists `@floor.b`, which bedrock.json \
             does not declare; add it to every catalog or drop the row's entry",
            "spec.md: tier-1-tokens: bedrock.json declares `@floor.c`, which the tier-1 table \
             does not list; add a row for it and declare it in every edition's catalog, or drop \
             it from bedrock.json",
        ]
    );
}
