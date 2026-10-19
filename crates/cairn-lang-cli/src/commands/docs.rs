//! `cairn docs`: the documentation site's pages, from the copy embedded in this binary.

use std::process::ExitCode;

use cairn_lang_core::suggest::{did_you_mean, nearest_match};
use cairn_lang_docs::{Page, Part, pages};

pub(crate) fn run_docs(page: Option<&str>, title: Option<&str>, toc: bool) -> ExitCode {
    let Some(written) = page else {
        list();
        return ExitCode::SUCCESS;
    };
    let (path, fragment) = split_link(written);
    if path.is_empty() {
        eprintln!(
            "error: `{written}` is the site's home page, which this build does not carry; \
             `cairn docs` lists every page it does"
        );
        return ExitCode::from(2);
    }
    let Some(page) = cairn_lang_docs::page(&path) else {
        let near = nearest_match(&path, pages().iter().map(Page::path));
        eprintln!(
            "error: this build carries no page `{path}`{}; `cairn docs` lists every page",
            hint(near)
        );
        return ExitCode::from(2);
    };
    match (fragment, title) {
        (Some(_), Some(_)) => {
            eprintln!(
                "error: both `#fragment` and TITLE name a part of `{}`; give one of the two",
                page.path()
            );
            ExitCode::from(2)
        }
        (Some(_), None) if toc => {
            eprintln!("error: `--toc` lists a whole page's headings; drop the `#fragment`");
            ExitCode::from(2)
        }
        (Some(fragment), None) => print_anchored(page, &fragment),
        (None, Some(title)) => print_cited(page, title),
        (None, None) if toc => {
            print_toc(page);
            ExitCode::SUCCESS
        }
        (None, None) => {
            print_part(page, &Part::Page);
            ExitCode::SUCCESS
        }
    }
}

fn list() {
    let width = pages()
        .iter()
        .map(|page| page.path().len())
        .max()
        .unwrap_or(0);
    for page in pages() {
        outln!("{:<width$}  {}", page.path(), page.title());
    }
}

fn print_anchored(page: &Page, fragment: &str) -> ExitCode {
    if let Some(section) = page.anchored(fragment) {
        outln!("{}", section.trim_end());
        return ExitCode::SUCCESS;
    }
    let anchors: Vec<String> = page
        .headings()
        .iter()
        .map(|heading| format!("#{}", heading.slug))
        .collect();
    let written = format!("#{fragment}");
    let near = nearest_match(&written, anchors.iter().map(String::as_str));
    eprintln!(
        "error: `{}` has no heading whose anchor is `{written}`{}; `cairn docs {} --toc` lists \
         each heading's anchor",
        page.path(),
        hint(near),
        page.path(),
    );
    ExitCode::from(2)
}

fn print_cited(page: &Page, title: &str) -> ExitCode {
    let mut parts = page.cited(title);
    if parts.is_empty() {
        parts = numbered(page, title);
    }
    match parts.as_slice() {
        [part] => {
            print_part(page, part);
            ExitCode::SUCCESS
        }
        [] => {
            let titles = page.titles();
            let near = nearest_match(title, titles.iter().copied());
            eprintln!(
                "error: `{}` has no heading, term or title \"{title}\"{}; `cairn docs {} --toc` \
                 lists its headings",
                page.path(),
                hint(near),
                page.path(),
            );
            ExitCode::from(2)
        }
        parts => {
            eprintln!("{}", ambiguous(page, title, parts));
            ExitCode::from(2)
        }
    }
}

/// The parts `title` names when it is written with the number a citation leaves out.
fn numbered(page: &Page, title: &str) -> Vec<Part> {
    let own = (page.title() == title).then_some(Part::Page);
    let sections = page
        .headings()
        .into_iter()
        .filter(|heading| heading.text == title)
        .map(Part::Section);
    own.into_iter().chain(sections).collect()
}

fn ambiguous(page: &Page, title: &str, parts: &[Part]) -> String {
    let each: Vec<String> = parts
        .iter()
        .map(|part| match part {
            Part::Page => "the page itself".to_owned(),
            Part::Section(heading) => format!("the section `#{}`", heading.slug),
            Part::Term {
                section: Some(slug),
                ..
            } => format!("a term in `#{slug}`"),
            Part::Term { section: None, .. } => "a term above the first heading".to_owned(),
        })
        .collect();
    format!(
        "error: \"{title}\" names {} parts of `{}`: {}; give the `#fragment` of the section you \
         mean instead of TITLE",
        parts.len(),
        page.path(),
        each.join(", "),
    )
}

fn print_part(page: &Page, part: &Part) {
    let text = match part {
        Part::Page => page.document(),
        Part::Section(heading) => heading.markdown.to_owned(),
        Part::Term { markdown, .. } => (*markdown).to_owned(),
    };
    outln!("{}", text.trim_end());
}

fn print_toc(page: &Page) {
    let headings = page.headings();
    if headings.is_empty() {
        eprintln!(
            "note: `{}` has no headings; `cairn docs {}` prints it whole",
            page.path(),
            page.path()
        );
    }
    let top = headings
        .iter()
        .map(|heading| heading.level)
        .min()
        .unwrap_or(0);
    for heading in &headings {
        outln!(
            "{}- [{}](/{}/#{})",
            "  ".repeat(heading.level - top),
            heading.text,
            page.path(),
            heading.slug,
        );
    }
}

fn hint(near: Option<&str>) -> String {
    near.map(|near| format!(" ({})", did_you_mean(near)))
        .unwrap_or_default()
}

/// A page path and fragment from a path, a site link, or a URL of the site.
fn split_link(written: &str) -> (String, Option<String>) {
    let local = ["https://", "http://"]
        .iter()
        .find_map(|scheme| written.strip_prefix(scheme))
        .map_or(written, |rest| {
            rest.find('/').map_or("", |slash| &rest[slash..])
        });
    let (path, fragment) = match local.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (local, None),
    };
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    (
        percent_decoded(path.trim_matches('/')),
        fragment
            .filter(|fragment| !fragment.is_empty())
            .map(percent_decoded),
    )
}

/// `text` with each `%XX` escape decoded, or unchanged when the escapes are not UTF-8.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .filter(|hex| hex.iter().all(u8::is_ascii_hexdigit));
        if let Some(hex) = escape {
            let hex = std::str::from_utf8(hex).expect("ASCII hex digits");
            decoded.push(u8::from_str_radix(hex, 16).expect("two hex digits"));
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|_| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_link_or_url_names_the_page_and_fragment() {
        let split = |written| split_link(written);
        assert_eq!(split("ja/tutorial"), ("ja/tutorial".into(), None));
        assert_eq!(
            split("/spec/lint/#error-vs-warning"),
            ("spec/lint".into(), Some("error-vs-warning".into()))
        );
        assert_eq!(
            split("/ja/spec/syntax#ヘッダ"),
            ("ja/spec/syntax".into(), Some("ヘッダ".into()))
        );
        assert_eq!(
            split("https://cairn.kage1020.com/ja/spec/syntax/#%E3%83%98%E3%83%83%E3%83%80"),
            ("ja/spec/syntax".into(), Some("ヘッダ".into()))
        );
        assert_eq!(
            split("https://cairn.kage1020.com/spec/syntax/index.md"),
            ("spec/syntax/index.md".into(), None)
        );
        assert_eq!(
            split("/spec/syntax/?x=1#headers"),
            ("spec/syntax".into(), Some("headers".into()))
        );
        assert_eq!(split("https://cairn.kage1020.com"), (String::new(), None));
        assert_eq!(split("tutorial/#"), ("tutorial".into(), None));
    }

    #[test]
    fn an_escape_that_is_not_utf8_leaves_the_text_as_written() {
        assert_eq!(percent_decoded("100%"), "100%");
        assert_eq!(percent_decoded("%zz"), "%zz");
        assert_eq!(percent_decoded("%FF"), "%FF");
        assert_eq!(percent_decoded("a%2Fb"), "a/b");
    }

    #[test]
    fn an_ambiguous_title_lists_each_part_it_names() {
        let page = Page::new(
            "test",
            "1. Same",
            "- **Same.** Above.\n\n## 1.1 Same\n\n- **Same.** Below.\n",
        );
        assert_eq!(
            ambiguous(&page, "Same", &page.cited("Same")),
            "error: \"Same\" names 4 parts of `test`: the page itself, the section \
             `#11-same`, a term above the first heading, a term in `#11-same`; give the \
             `#fragment` of the section you mean instead of TITLE"
        );
    }
}
