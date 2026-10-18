use super::*;

fn synthetic(markdown: &'static str) -> Page {
    Page {
        path: "test",
        title: "3. A Chapter",
        source: markdown,
        body: 0,
    }
}

const CHAPTER: &str = "\
Intro.

## 3.1 First

One.

```md
## 3.9 Not a heading
```

### 3.1.1 `code_span` inside

Two.

## 3.2 Second

- **Term.** Defined
  across two lines.
- **Other** term.

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
                "## 3.1 First\n\nOne.\n\n```md\n## 3.9 Not a heading\n```\n\n\
                 ### 3.1.1 `code_span` inside\n\nTwo.\n\n"
            ),
            (
                3,
                "3.1.1 `code_span` inside",
                "### 3.1.1 `code_span` inside\n\nTwo.\n\n"
            ),
            (
                2,
                "3.2 Second",
                "## 3.2 Second\n\n- **Term.** Defined\n  across two lines.\n- **Other** term.\n\n\
                 Three.\n"
            ),
        ]
    );
}

#[test]
fn a_term_runs_through_its_continuation_lines() {
    let page = synthetic(CHAPTER);
    let terms: Vec<_> = page
        .terms()
        .into_iter()
        .map(|term| (term.name, term.markdown))
        .collect();
    assert_eq!(
        terms,
        [
            ("Term", "- **Term.** Defined\n  across two lines.\n"),
            ("Other", "- **Other** term.\n"),
        ]
    );
}

#[test]
fn a_citation_names_the_page_a_section_or_a_term_by_title_without_its_number() {
    let page = synthetic(CHAPTER);
    assert_eq!(page.cited("A Chapter"), Some(CHAPTER));
    assert_eq!(
        page.cited("`code_span` inside"),
        Some("### 3.1.1 `code_span` inside\n\nTwo.\n\n")
    );
    assert_eq!(
        page.cited("Term"),
        Some("- **Term.** Defined\n  across two lines.\n")
    );
    assert_eq!(page.cited("3.1 First"), None);
    assert_eq!(page.cited("Not a heading"), None);
    assert_eq!(
        page.titles(),
        [
            "A Chapter",
            "First",
            "`code_span` inside",
            "Second",
            "Term",
            "Other"
        ]
    );
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
         ### Breaking changes 1\n",
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
fn a_page_is_found_at_the_path_the_site_serves_it_at() {
    let syntax = page("spec/syntax").expect("the syntax chapter");
    assert_eq!(syntax.title(), "5. Syntax");
    assert!(syntax.markdown().starts_with("\n## 5.1 Lexical\n"));
    assert_eq!(
        page("spec").map(Page::title),
        Some("Specification 2026.06 (draft)")
    );
    assert_eq!(page("ja/spec/syntax").map(Page::title), Some("5. 構文"));
    assert!(page("spec/syntax.md").is_none());
    assert!(page("playground").is_none(), "an MDX page is not embedded");
}

#[test]
fn the_specification_lists_in_reading_order() {
    let chapters: Vec<_> = pages()
        .iter()
        .map(Page::path)
        .filter(|path| *path == "spec" || path.starts_with("spec/"))
        .collect();
    assert_eq!(
        chapters,
        [
            "spec",
            "spec/overview",
            "spec/principles",
            "spec/architecture",
            "spec/compilation",
            "spec/syntax",
            "spec/blockstate",
            "spec/materials-themes",
            "spec/entities",
            "spec/components-editing-sites",
            "spec/versioning-editions",
            "spec/lint",
            "spec/ecosystem-interop",
            "spec/evaluation",
            "spec/redstone",
            "spec/open-issues",
            "spec/compatibility",
            "spec/glossary",
        ]
    );
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
        for (open, _) in from.markdown().match_indices("](/") {
            let target = &from.markdown()[open + 3..];
            let Some(end) = target.find(')') else {
                continue;
            };
            let Some((path, fragment)) = target[..end].split_once('#') else {
                continue;
            };
            let path = path.trim_end_matches('/');
            let Some(to) = page(path) else {
                unresolved.push(format!("{}: no page `{path}`", from.path()));
                continue;
            };
            if to.anchored(fragment).is_none() {
                unresolved.push(format!("{}: `{path}` has no `#{fragment}`", from.path()));
            }
        }
    }
    assert_eq!(unresolved, Vec::<String>::new());
}
