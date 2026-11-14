use super::*;

static ENGLISH: Page = Page::new(
    "spec/syntax",
    "Syntax",
    "\
## 5.6 Keyword reference

- **`id=`.** The member's name.
- **`door`.** A door.
  - **`side=`.** The wall.
  - **`at=`.** Where along it.
- **Not code.** A term the rule leaves out.
",
);

fn rendered(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|finding| finding.render(RULE.name))
        .collect()
}

#[test]
fn each_code_term_is_read_with_the_code_term_it_nests_in() {
    assert_eq!(
        entries(&ENGLISH),
        BTreeSet::from([
            (None, "`id=`"),
            (None, "`door`"),
            (Some("`door`"), "`side=`"),
            (Some("`door`"), "`at=`"),
        ]),
    );
}

#[test]
fn a_japanese_page_that_nests_its_entries_alike_has_no_finding() {
    static JAPANESE: Page = Page::new(
        "ja/spec/syntax",
        "構文",
        "\
## 5.6 キーワード一覧

- **`id=`。** メンバーの名前です。
- **`door`。** ドアです。
  - **`side=`。** 壁です。
  - **`at=`。** 壁のどこかです。
",
    );
    let found = compare("ja.md", &entries(&ENGLISH), &entries(&JAPANESE));
    assert_eq!(rendered(&found), Vec::<String>::new());
}

#[test]
fn an_entry_missing_extra_or_under_another_keyword_is_reported_at_the_japanese_page() {
    static JAPANESE: Page = Page::new(
        "ja/spec/syntax",
        "構文",
        "\
## 5.6 キーワード一覧

- **`door`。** ドアです。
  - **`side=`。** 壁です。
- **`window`。** 窓です。
  - **`at=`。** 壁のどこかです。
",
    );
    let found = compare("ja.md", &entries(&ENGLISH), &entries(&JAPANESE));
    assert_eq!(
        rendered(&found),
        [
            "ja.md: keyword-reference: has no entry `id=` at the top level, which the English \
             keyword reference has; translate it there",
            "ja.md: keyword-reference: has no entry `at=` under `door`, which the English \
             keyword reference has; translate it there",
            "ja.md: keyword-reference: has an entry `window` at the top level the English \
             keyword reference does not; add it there or drop it here",
            "ja.md: keyword-reference: has an entry `at=` under `window` the English keyword \
             reference does not; add it there or drop it here",
        ],
    );
}
