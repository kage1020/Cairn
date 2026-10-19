//! `cairn docs` end to end: which page or part an argument names, and how it is printed.

use cairn_lang_docs::{Heading, Page, Part, page, pages};

use crate::common::{Run, cairn};

fn docs(args: &[&str]) -> Run {
    Run::from(cairn("docs", args))
}

fn embedded(path: &str) -> &'static Page {
    page(path).unwrap_or_else(|| panic!("this build embeds `{path}`"))
}

/// The heading `title` cites on the page at `path`.
fn section(path: &str, title: &str) -> Heading {
    match embedded(path).cited(title).as_slice() {
        [Part::Section(heading)] => heading.clone(),
        parts => panic!("`{path}` \"{title}\" names {parts:?}, not one section"),
    }
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
    let expected = principles.document();
    for args in [
        &["spec/principles"][..],
        &["/spec/principles/"],
        &["https://cairn.kage1020.com/spec/principles/"],
        &["https://cairn.kage1020.com/spec/principles/index.md?from=llms.txt"],
        &["spec/principles.md"],
        &["spec/principles", principles.title()],
        &["spec/principles", principles.titles()[0]],
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
fn a_quoted_title_and_a_link_fragment_print_the_section_they_name() {
    let heading = section("spec/lint", "Error vs warning");
    let expected = format!("{}\n", heading.markdown.trim_end());
    let link = format!("/spec/lint/#{}", heading.slug);
    let url = format!(
        "https://cairn.kage1020.com/spec/lint/index.md#{}",
        heading.slug
    );
    for args in [
        &["/spec/lint/", "Error vs warning"][..],
        &["/spec/lint/", heading.text],
        &[link.as_str()],
        &[url.as_str()],
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
    let heading = section("ja/spec/syntax", "ヘッダ");
    let escaped: String = heading
        .slug
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || byte == b'-' {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    assert_ne!(escaped, heading.slug);
    let run = docs(&[&format!("/ja/spec/syntax/#{escaped}")]);
    assert_eq!(
        (run.code, run.stderr.as_str(), run.stdout.as_str()),
        (
            Some(0),
            "",
            format!("{}\n", heading.markdown.trim_end()).as_str()
        ),
    );
}

#[test]
fn the_toc_lists_each_heading_as_a_link_the_command_takes() {
    let syntax = embedded("spec/syntax");
    assert!(syntax.headings().iter().any(|heading| heading.level > 2));
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
    let run = docs(&["/spec/syntax/", "--toc"]);
    assert_eq!(
        (run.code, run.stderr.as_str(), run.stdout.as_str()),
        (Some(0), "", expected.as_str()),
    );
}

#[test]
fn what_names_nothing_is_refused_with_the_nearest_candidate() {
    let headers = section("spec/syntax", "Headers").slug;
    let typo = &headers[..headers.len() - 1];
    let with_typo = format!("/spec/syntax/#{typo}");
    let with_fragment = format!("/spec/syntax/#{headers}");
    for (args, stderr) in [
        (
            &["tutorail"][..],
            "error: this build carries no page `tutorail` (did you mean `tutorial`?); `cairn docs` \
             lists every page\n"
                .to_owned(),
        ),
        (
            &["https://cairn.kage1020.com/"],
            "error: `https://cairn.kage1020.com/` is the site's home page, which this build does \
             not carry; `cairn docs` lists every page it does\n"
                .to_owned(),
        ),
        (
            &["/spec/syntax/", "Headerz"],
            "error: `spec/syntax` has no heading, term or title \"Headerz\" (did you mean \
             `Headers`?); `cairn docs spec/syntax --toc` lists its headings\n"
                .to_owned(),
        ),
        (
            &[with_typo.as_str()],
            format!(
                "error: `spec/syntax` has no heading whose anchor is `#{typo}` (did you mean \
                 `#{headers}`?); `cairn docs spec/syntax --toc` lists each heading's anchor\n"
            ),
        ),
        (
            &[with_fragment.as_str(), "Headers"],
            "error: both `#fragment` and TITLE name a part of `spec/syntax`; give one of the two\n"
                .to_owned(),
        ),
        (
            &[with_fragment.as_str(), "--toc"],
            "error: `--toc` lists a whole page's headings; drop the `#fragment`\n".to_owned(),
        ),
        (
            &["/spec/syntax/", "Headers", "--toc"],
            "error: the argument '[TITLE]' cannot be used with '--toc'\n\n\
             Usage: cairn docs <PAGE> <TITLE>\n\n\
             For more information, try '--help'.\n"
                .to_owned(),
        ),
    ] {
        let run = docs(args);
        assert_eq!(
            (run.code, run.stdout.as_str(), run.stderr.as_str()),
            (Some(2), "", stderr.as_str()),
            "`cairn docs {}`",
            args.join(" "),
        );
    }
}
