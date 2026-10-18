//! `cairn docs` end to end: which page or part an argument names, and how it is printed.

use cairn_lang_docs::{Page, page, pages};

use crate::common::{Run, cairn};

fn docs(args: &[&str]) -> Run {
    Run::from(cairn("docs", args))
}

fn embedded(path: &str) -> &'static Page {
    page(path).unwrap_or_else(|| panic!("this build embeds `{path}`"))
}

#[test]
fn without_a_page_every_page_is_listed_beside_its_title() {
    let run = docs(&[]);
    assert_eq!((run.code, run.stderr.as_str()), (Some(0), ""));
    let width = pages().iter().map(|page| page.path().len()).max().unwrap();
    let expected = pages()
        .iter()
        .map(|page| format!("{:<width$}  {}\n", page.path(), page.title()))
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(run.stdout, expected);
}

#[test]
fn a_page_prints_as_markdown_under_its_title() {
    let principles = embedded("spec/principles");
    let expected = format!(
        "# 2. Design Principles\n\n{}\n",
        principles.markdown().trim()
    );
    for written in [
        "spec/principles",
        "/spec/principles/",
        "https://cairn.kage1020.com/spec/principles/",
    ] {
        let run = docs(&[written]);
        assert_eq!(
            (run.code, run.stderr.as_str(), run.stdout.as_str()),
            (Some(0), "", expected.as_str()),
            "`cairn docs {written}`",
        );
    }
}

#[test]
fn a_quoted_title_and_a_link_fragment_print_the_section_they_name() {
    let section = embedded("spec/lint")
        .cited("Error vs warning")
        .expect("spec/lint cites \"Error vs warning\"");
    let expected = format!("{}\n", section.trim_end());
    for args in [
        &["/spec/lint/", "Error vs warning"][..],
        &["/spec/lint/", "11.3 Error vs warning"],
        &["/spec/lint/#113-error-vs-warning"],
    ] {
        let run = docs(args);
        assert_eq!(
            (run.code, run.stderr.as_str(), run.stdout.as_str()),
            (Some(0), "", expected.as_str()),
            "`cairn docs {}`",
            args.join(" "),
        );
    }
}

#[test]
fn a_percent_escaped_fragment_names_the_heading_it_spells() {
    let run = docs(&["/ja/spec/syntax/#53-%E3%83%98%E3%83%83%E3%83%80"]);
    assert_eq!((run.code, run.stderr.as_str()), (Some(0), ""));
    assert_eq!(run.stdout.lines().next(), Some("## 5.3 ヘッダ"));
}

#[test]
fn the_toc_lists_each_heading_as_a_link_the_command_takes() {
    let syntax = embedded("spec/syntax");
    let expected = syntax
        .headings()
        .iter()
        .map(|heading| {
            format!(
                "{}- [{}](/spec/syntax/#{})\n",
                "  ".repeat(heading.level - 2),
                heading.text,
                heading.slug,
            )
        })
        .collect::<Vec<_>>()
        .concat();
    assert!(expected.contains("\n  - [5.1.1 Literals and separators]("));
    let run = docs(&["/spec/syntax/", "--toc"]);
    assert_eq!(
        (run.code, run.stderr.as_str(), run.stdout.as_str()),
        (Some(0), "", expected.as_str()),
    );
}

#[test]
fn what_names_nothing_is_refused_with_the_nearest_candidate() {
    for (args, stderr) in [
        (
            &["tutorail"][..],
            "error: this build carries no page `tutorail` (did you mean `tutorial`?); `cairn docs` \
             lists every page\n",
        ),
        (
            &["/spec/syntax/", "Headerz"],
            "error: `spec/syntax` has no heading, term or title \"Headerz\" (did you mean \
             `Headers`?); `cairn docs spec/syntax --toc` lists its headings\n",
        ),
        (
            &["/spec/syntax/#53-header"],
            "error: `spec/syntax` has no heading whose anchor is `#53-header` (did you mean \
             `#53-headers`?); `cairn docs spec/syntax --toc` lists each heading's anchor\n",
        ),
        (
            &["/spec/syntax/#53-headers", "Headers"],
            "error: both `#fragment` and TITLE name a part of `spec/syntax`; give one of the two\n",
        ),
        (
            &["/spec/syntax/#53-headers", "--toc"],
            "error: `--toc` lists a whole page's headings; drop the `#fragment`\n",
        ),
    ] {
        let run = docs(args);
        assert_eq!(
            (run.code, run.stdout.as_str(), run.stderr.as_str()),
            (Some(2), "", stderr),
            "`cairn docs {}`",
            args.join(" "),
        );
    }
}
