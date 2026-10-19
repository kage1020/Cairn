use std::path::Path;

use super::*;

fn synthetic(markdown: &'static str) -> Page {
    Page::new("test", "3. A Chapter", markdown)
}

const CHAPTER: &str = "\
Intro.

- **Early.** Before any heading.

## 3.1 First

One.

```md
## 3.9 Not a heading
- **Fenced.** Not a term.
```

### 3.1.1 `code_span` inside

Two.

````md
```
## 3.8 Inside a longer fence
````rust
## 3.7 Still inside
````

## 3.2 Second

- **Term.** Defined
  across two lines.
- **Other** item, not a term.
- **Spread.** Over a blank line,

  into a second paragraph.
- **高さ。** 句点が太字の内側にある。
- **用語**。 句点が太字の外側にある。

Three.
";

#[test]
fn a_section_runs_to_the_next_heading_of_its_level_or_above_and_fences_hold_none() {
    let page = synthetic(CHAPTER);
    let headings: Vec<_> = page
        .headings()
        .into_iter()
        .map(|heading| (heading.level, heading.text, heading.markdown))
        .collect();
    assert_eq!(
        headings,
        [
            (
                2,
                "3.1 First",
                "## 3.1 First\n\nOne.\n\n```md\n## 3.9 Not a heading\n- **Fenced.** Not a term.\n\
                 ```\n\n### 3.1.1 `code_span` inside\n\nTwo.\n\n````md\n```\n\
                 ## 3.8 Inside a longer fence\n````rust\n## 3.7 Still inside\n````\n\n"
            ),
            (
                3,
                "3.1.1 `code_span` inside",
                "### 3.1.1 `code_span` inside\n\nTwo.\n\n````md\n```\n\
                 ## 3.8 Inside a longer fence\n````rust\n## 3.7 Still inside\n````\n\n"
            ),
            (
                2,
                "3.2 Second",
                "## 3.2 Second\n\n- **Term.** Defined\n  across two lines.\n\
                 - **Other** item, not a term.\n- **Spread.** Over a blank line,\n\n  \
                 into a second paragraph.\n- **高さ。** 句点が太字の内側にある。\n\
                 - **用語**。 句点が太字の外側にある。\n\nThree.\n"
            ),
        ]
    );
}

#[test]
fn a_term_ends_with_a_full_stop_and_runs_through_its_continuation_lines() {
    let page = synthetic(CHAPTER);
    let term = |markdown, section: Option<&str>| {
        vec![Part::Term {
            markdown,
            section: section.map(str::to_owned),
        }]
    };
    assert_eq!(
        page.cited("Early"),
        term("- **Early.** Before any heading.\n", None)
    );
    assert_eq!(
        page.cited("Term"),
        term(
            "- **Term.** Defined\n  across two lines.\n",
            Some("32-second")
        )
    );
    assert_eq!(
        page.cited("Spread"),
        term(
            "- **Spread.** Over a blank line,\n\n  into a second paragraph.\n",
            Some("32-second")
        )
    );
    assert_eq!(
        page.cited("高さ"),
        term("- **高さ。** 句点が太字の内側にある。\n", Some("32-second"))
    );
    assert_eq!(
        page.cited("用語"),
        term("- **用語**。 句点が太字の外側にある。\n", Some("32-second"))
    );
    assert_eq!(page.cited("Other"), []);
    assert_eq!(page.cited("Fenced"), []);
}

#[test]
fn a_citation_names_the_page_a_section_or_a_term_by_title_without_its_number() {
    let page = synthetic(CHAPTER);
    assert_eq!(page.cited("A Chapter"), [Part::Page]);
    let cited = page.cited("`code_span` inside");
    let [Part::Section(heading)] = cited.as_slice() else {
        panic!("one section is titled `code_span` inside");
    };
    assert_eq!(heading.slug, "311-code_span-inside");
    assert_eq!(page.cited("3.1 First"), []);
    assert_eq!(page.cited("3. A Chapter"), []);
    assert_eq!(page.cited("Not a heading"), []);
    assert_eq!(
        page.titles(),
        [
            "A Chapter",
            "First",
            "`code_span` inside",
            "Second",
            "Early",
            "Term",
            "Spread",
            "高さ",
            "用語",
        ]
    );
}

#[test]
fn a_title_more_than_one_part_has_names_each_of_them() {
    let page = Page::new(
        "test",
        "1. Same",
        "## 1.1 Same\n\n- **Same.** A term.\n\n## 1.2 Same\n",
    );
    let sections: Vec<_> = page.headings().into_iter().map(Part::Section).collect();
    assert_eq!(
        page.cited("Same"),
        [
            Part::Page,
            sections[0].clone(),
            sections[1].clone(),
            Part::Term {
                markdown: "- **Same.** A term.\n",
                section: Some("11-same".to_owned()),
            },
        ]
    );
    assert_eq!(page.titles(), ["Same"]);
}

#[test]
fn an_anchor_is_the_slug_the_site_gives_the_heading() {
    let page = synthetic(
        "## 10.4 Fail-loud and minimum-version inference\n\
         ## 6.2 `intent_state` and `resolved_state`\n\
         ## \"did you mean\"\n\
         ## [Linked](/spec/lint/) label\n\
         ### 順序付けは DataVersion で、エディションごとに\n\
         ### Breaking changes\n\
         ### Breaking changes\n\
         ### Breaking changes 1\n\
         ### A\n\
         ### A 1\n\
         ### A\n",
    );
    let slugs: Vec<_> = page
        .headings()
        .into_iter()
        .map(|heading| heading.slug)
        .collect();
    assert_eq!(
        slugs,
        [
            "104-fail-loud-and-minimum-version-inference",
            "62-intent_state-and-resolved_state",
            "did-you-mean",
            "linked-label",
            "順序付けは-dataversion-でエディションごとに",
            "breaking-changes",
            "breaking-changes-1",
            "breaking-changes-1-1",
            "a",
            "a-1",
            "a-2",
        ]
    );
    assert_eq!(
        page.anchored("breaking-changes-1"),
        Some("### Breaking changes\n")
    );
    assert_eq!(page.anchored("missing"), None);
}

#[test]
fn a_number_is_stripped_only_when_it_is_one() {
    assert_eq!(strip_number("14.5 Place-and-route"), "Place-and-route");
    assert_eq!(
        strip_number("C.3 How a break is communicated"),
        "How a break is communicated"
    );
    assert_eq!(strip_number("3D coordinates"), "3D coordinates");
    assert_eq!(strip_number("Glossary"), "Glossary");
}

#[test]
fn a_closing_hash_sequence_is_not_part_of_the_heading() {
    assert_eq!(heading("## Title ##"), Some((2, "Title")));
    assert_eq!(heading("## C# #"), Some((2, "C#")));
    assert_eq!(heading("#hashtag"), None);
    assert_eq!(heading("    ## indented code"), None);
}

#[test]
fn a_page_is_found_by_its_url_or_its_file() {
    for written in [
        "spec/syntax",
        "spec/syntax.md",
        "spec/syntax/index",
        "spec/syntax/index.md",
    ] {
        assert_eq!(
            page(written).map(Page::path),
            Some("spec/syntax"),
            "{written}"
        );
    }
    for written in ["spec", "spec/index", "spec/index.md"] {
        assert_eq!(page(written).map(Page::path), Some("spec"), "{written}");
    }
    assert_eq!(
        page("ja/spec/syntax/index.md").map(Page::path),
        Some("ja/spec/syntax")
    );
    assert!(page("index").is_none(), "the home page is MDX");
    assert!(page("playground").is_none(), "an MDX page is not embedded");
}

#[test]
fn a_page_holds_its_file_below_the_front_matter() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/content/docs");
    for (path, file) in [
        ("spec", "spec/index.md"),
        ("spec/syntax", "spec/syntax.md"),
        ("ja/tutorial", "ja/tutorial.md"),
    ] {
        let source = std::fs::read_to_string(docs.join(file)).expect("the page's file");
        let page = page(path).expect("an embedded page");
        let front = source
            .strip_suffix(page.markdown())
            .expect("the file ends with the page's Markdown");
        assert!(
            front.starts_with("---\n") && front.ends_with("\n---\n"),
            "{file}"
        );
        assert!(front.contains(page.title()), "{file}");
    }
}

#[test]
fn the_specification_lists_its_index_first_then_its_chapters_by_number() {
    let chapter = |page: &Page| {
        page.title()
            .split_once(". ")
            .and_then(|(number, _)| number.parse::<u32>().ok())
    };
    let spec: Vec<_> = pages()
        .iter()
        .filter(|page| page.path() == "spec" || page.path().starts_with("spec/"))
        .map(|page| {
            (
                page.path() != "spec",
                chapter(page).is_none(),
                chapter(page),
            )
        })
        .collect();
    assert!(
        spec.iter()
            .filter(|(_, _, number)| number.is_some())
            .count()
            > 1
    );
    assert!(spec.is_sorted(), "{spec:?}");
    assert_eq!(spec[0], (false, true, None));

    let first_japanese = pages()
        .iter()
        .position(|page| page.path().starts_with("ja/"))
        .expect("a Japanese page");
    assert!(
        pages()[first_japanese..]
            .iter()
            .all(|page| page.path().starts_with("ja/")),
        "every English page lists before the first Japanese one"
    );
}

/// The site's build refuses a link whose `#fragment` names no heading, so a fragment this crate
/// cannot find is a slug it computes differently from the site.
#[test]
fn every_fragment_a_page_links_to_is_an_anchor_of_that_page() {
    let mut unresolved = Vec::new();
    for from in pages() {
        for (open, _) in from.markdown().match_indices("](") {
            let target = &from.markdown()[open + 2..];
            let Some(end) = target.find(')') else {
                continue;
            };
            let Some((path, fragment)) = target[..end].split_once('#') else {
                continue;
            };
            let to = if path.is_empty() {
                from
            } else if let Some(path) = path.strip_prefix('/') {
                let path = path.trim_end_matches('/');
                let Some(to) = page(path) else {
                    unresolved.push(format!("{}: no page `{path}`", from.path()));
                    continue;
                };
                to
            } else {
                continue;
            };
            if to.anchored(fragment).is_none() {
                unresolved.push(format!(
                    "{}: `{}` has no `#{fragment}`",
                    from.path(),
                    to.path()
                ));
            }
        }
    }
    assert_eq!(unresolved, Vec::<String>::new());
}
