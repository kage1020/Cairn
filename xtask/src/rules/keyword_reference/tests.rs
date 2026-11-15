use super::*;

static ENGLISH: Page = Page::new(
    "spec/syntax",
    "Syntax",
    "\
## 5.6 Keyword reference

- **`id=`.** The member's name.
- **`door`.** A door.
  - **`side=`.** The wall.
    - **`front`.** The front wall.
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
fn each_code_term_is_read_with_the_term_of_the_innermost_item_holding_it() {
    assert_eq!(
        entries(&ENGLISH),
        BTreeMap::from([
            ((None, "`id=`"), 1),
            ((None, "`door`"), 1),
            ((Some("`door`"), "`side=`"), 1),
            ((Some("`side=`"), "`front`"), 1),
            ((Some("`door`"), "`at=`"), 1),
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
    - **`front`。** 正面の壁です。
  - **`at=`。** 壁のどこかです。
",
    );
    let japanese = entries(&JAPANESE);
    assert_eq!(
        rendered(&repeated("ja.md", &japanese)),
        Vec::<String>::new()
    );
    let found = compare("ja.md", &entries(&ENGLISH), &japanese);
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
  - **`front`。** 正面の壁です。
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
            "ja.md: keyword-reference: has no entry `front` under `side=`, which the English \
             keyword reference has; translate it there",
            "ja.md: keyword-reference: has an entry `window` at the top level the English \
             keyword reference does not; add it there or drop it here",
            "ja.md: keyword-reference: has an entry `front` under `door` the English keyword \
             reference does not; add it there or drop it here",
            "ja.md: keyword-reference: has an entry `at=` under `window` the English keyword \
             reference does not; add it there or drop it here",
        ],
    );
}

#[test]
fn an_entry_a_page_defines_twice_in_one_place_is_reported_at_that_page() {
    static JAPANESE: Page = Page::new(
        "ja/spec/syntax",
        "構文",
        "\
## 5.6 キーワード一覧

- **`id=`。** メンバーの名前です。
- **`door`。** ドアです。
  - **`side=`。** 壁です。
    - **`front`。** 正面の壁です。
  - **`at=`。** 壁のどこかです。
  - **`side=`。** 切り込む壁です。
- **`id=`。** メンバーの名前です。
",
    );
    let japanese = entries(&JAPANESE);
    assert_eq!(
        rendered(&repeated("ja.md", &japanese)),
        [
            "ja.md: keyword-reference: has 2 entries `id=` at the top level; keep one",
            "ja.md: keyword-reference: has 2 entries `side=` under `door`; keep one",
        ],
    );
    let found = compare("ja.md", &entries(&ENGLISH), &japanese);
    assert_eq!(rendered(&found), Vec::<String>::new());
}
