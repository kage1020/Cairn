//! `cairn docs`: the documentation site's pages, from the copy embedded in this binary.

use std::process::ExitCode;

use cairn_lang_core::suggest::{did_you_mean, nearest_match};
use cairn_lang_docs::{Page, pages};

pub(crate) fn run_docs(page: Option<&str>, title: Option<&str>, toc: bool) -> ExitCode {
    let Some(written) = page else {
        list();
        return ExitCode::SUCCESS;
    };
    let (path, fragment) = split_link(written);
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
                "error: both `#fragment` and TITLE name a part of `{path}`; give one of the two"
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
            outln!("# {}\n\n{}", page.title(), page.markdown().trim());
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
    let cited = page.cited(title).or_else(|| {
        page.headings()
            .into_iter()
            .find(|heading| heading.text == title)
            .map(|heading| heading.markdown)
    });
    if let Some(part) = cited {
        outln!("{}", part.trim_end());
        return ExitCode::SUCCESS;
    }
    let titles = page.titles();
    let near = nearest_match(title, titles.iter().copied());
    eprintln!(
        "error: `{}` has no heading, term or title \"{title}\"{}; `cairn docs {} --toc` lists its \
         headings",
        page.path(),
        hint(near),
        page.path(),
    );
    ExitCode::from(2)
}

fn print_toc(page: &Page) {
    let headings = page.headings();
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
            split("/spec/lint/#113-error-vs-warning"),
            ("spec/lint".into(), Some("113-error-vs-warning".into()))
        );
        assert_eq!(
            split("/ja/spec/syntax#53-ヘッダ"),
            ("ja/spec/syntax".into(), Some("53-ヘッダ".into()))
        );
        assert_eq!(
            split("https://cairn.kage1020.com/ja/spec/syntax/#53-%E3%83%98%E3%83%83%E3%83%80"),
            ("ja/spec/syntax".into(), Some("53-ヘッダ".into()))
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
}
