//! A built documentation site serves what `cairn-lang-docs` embeds: a page's `index.md` at each
//! embedded path and at no other, holding what `cairn docs` prints for that page, and an `id` on
//! each heading equal to the slug the crate computes for it. The crate reads the page sources
//! without a Markdown parser, so this is where a construct it reads differently from Starlight
//! surfaces. Only a build has the site to compare, so the rule checks nothing without `--site`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cairn_lang_docs::{Page, pages};

use super::{Finding, Rule};
use crate::repo::Repo;

pub const RULE: Rule = Rule {
    name: "site-parity",
    summary: "with --site, the built site serves the pages and anchors cairn-lang-docs embeds",
    check,
};

fn check(repo: &Repo) -> Vec<Finding> {
    let Some(site) = &repo.site else {
        return Vec::new();
    };
    let mut served = BTreeSet::new();
    if let Err(error) = served_pages(site, site, &mut served) {
        return vec![Finding::new(site, None, error)];
    }
    if served.is_empty() {
        return vec![Finding::new(
            site,
            None,
            "serves no `index.md`; run `pnpm run build` in `website/` and pass its `dist/`",
        )];
    }

    let mut findings = Vec::new();
    for path in &served {
        if !pages().iter().any(|page| page.path() == path) {
            findings.push(Finding::new(
                markdown_file(site, path),
                None,
                format!(
                    "the site serves `/{path}/index.md`, which cairn-lang-docs does not embed; \
                     make `website/rust/build.rs` read that page's file the way Starlight does"
                ),
            ));
        }
    }
    for page in pages() {
        if served.contains(page.path()) {
            same_markdown(site, page, &mut findings);
            same_anchors(site, page, &mut findings);
        } else {
            findings.push(Finding::new(
                site,
                None,
                format!(
                    "cairn-lang-docs embeds `{}`, which the site does not serve at `/{}/index.md`; \
                     make `website/rust/build.rs` leave out what Starlight leaves out",
                    page.path(),
                    page.path(),
                ),
            ));
        }
    }
    findings
}

/// Adds the path of each directory under `dir` that holds an `index.md`, relative to `site`.
fn served_pages(site: &Path, dir: &Path, served: &mut BTreeSet<String>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            served_pages(site, &path, served)?;
        } else if path.file_name().is_some_and(|name| name == "index.md") {
            let relative = dir.strip_prefix(site).expect("walked from the site");
            let parts: Vec<_> = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect();
            served.insert(parts.join("/"));
        }
    }
    Ok(())
}

fn markdown_file(site: &Path, path: &str) -> PathBuf {
    site.join(path).join("index.md")
}

fn same_markdown(site: &Path, page: &Page, findings: &mut Vec<Finding>) {
    let file = markdown_file(site, page.path());
    let served = match fs::read_to_string(&file) {
        Ok(served) => served,
        Err(error) => {
            findings.push(Finding::new(
                &file,
                None,
                format!("cannot read it: {error}"),
            ));
            return;
        }
    };
    let embedded = page.document();
    if served == embedded {
        return;
    }
    let (line, site_line, crate_line) = first_difference(&served, &embedded);
    findings.push(Finding::new(
        &file,
        Some(line),
        format!(
            "the site serves {site_line} where `cairn docs {}` prints {crate_line}; make \
             `website/rust/build.rs` read the front matter the way Starlight does",
            page.path(),
        ),
    ));
}

fn same_anchors(site: &Path, page: &Page, findings: &mut Vec<Finding>) {
    let file = site.join(page.path()).join("index.html");
    let html = match fs::read_to_string(&file) {
        Ok(html) => html,
        Err(error) => {
            findings.push(Finding::new(
                &file,
                None,
                format!("cannot read it: {error}"),
            ));
            return;
        }
    };
    let served = heading_ids(&html);
    let headings = page.headings();
    let computed: Vec<&str> = headings
        .iter()
        .map(|heading| heading.slug.as_str())
        .collect();
    if served == computed {
        return;
    }
    let index = served
        .iter()
        .zip(&computed)
        .position(|(served, computed)| served != computed)
        .unwrap_or(served.len().min(computed.len()));
    let quoted = |anchors: &[&str]| {
        anchors
            .get(index)
            .map_or("no heading".to_owned(), |anchor| format!("`#{anchor}`"))
    };
    findings.push(Finding::new(
        &file,
        None,
        format!(
            "heading {} of `{}` is {} on the site and {} in cairn-lang-docs; make \
             `website/rust/lib.rs` find headings and slug them the way Astro does",
            index + 1,
            page.path(),
            quoted(&served),
            quoted(&computed),
        ),
    ));
}

/// The 1-based line two texts first differ at, and that line of each, quoted.
fn first_difference(site: &str, embedded: &str) -> (usize, String, String) {
    let site_lines: Vec<&str> = site.lines().collect();
    let embedded_lines: Vec<&str> = embedded.lines().collect();
    let index = site_lines
        .iter()
        .zip(&embedded_lines)
        .position(|(site, embedded)| site != embedded)
        .unwrap_or(site_lines.len().min(embedded_lines.len()));
    let quoted = |lines: &[&str]| {
        lines
            .get(index)
            .map_or("the end of the page".to_owned(), |line| format!("`{line}`"))
    };
    (index + 1, quoted(&site_lines), quoted(&embedded_lines))
}

/// The `id` of each heading in a page Starlight rendered, its own title and outline left out.
fn heading_ids(html: &str) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut rest = html;
    while let Some(open) = rest.find("<h") {
        rest = &rest[open + 2..];
        if !rest.starts_with(['1', '2', '3', '4', '5', '6']) {
            continue;
        }
        let Some(end) = rest.find('>') else {
            break;
        };
        let id = rest[1..end]
            .split_once(" id=\"")
            .and_then(|(_, value)| value.split_once('"'))
            .map(|(id, _)| id);
        if let Some(id) = id.filter(|id| *id != "_top" && !id.starts_with("starlight__")) {
            ids.push(id);
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ids_of_a_pages_own_headings_are_read_in_order() {
        let html = "<header><h1 id=\"_top\">T</h1></header><hr>\
            <h2 id=\"starlight__on-this-page\">On this page</h2>\
            <div class=\"sl-heading-wrapper level-h2\"><h2 id=\"one\">One</h2></div>\
            <h3 class=\"x\" id=\"two\">Two</h3><h4>No id</h4><h5 data-id=\"no\" id=\"three\">";
        assert_eq!(heading_ids(html), ["one", "two", "three"]);
    }

    #[test]
    fn a_difference_is_reported_at_its_first_line() {
        assert_eq!(
            first_difference("# T\n\nsame\nsite\n", "# T\n\nsame\ncrate\n"),
            (4, "`site`".to_owned(), "`crate`".to_owned())
        );
        assert_eq!(
            first_difference("# T\n", "# T\n\nmore\n"),
            (2, "the end of the page".to_owned(), "``".to_owned())
        );
    }
}
