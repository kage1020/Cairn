//! Lists the site's Markdown pages for the crate to embed, each with its path, title and body.

use std::cmp::Ordering;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Where Starlight reads the pages from, relative to the package root.
const DOCS: &str = "src/content/docs";

/// The site's locales other than the root one, as `locales` in `astro.config.mjs` names them.
const LOCALES: &[&str] = &["ja"];

struct Found {
    path: String,
    file: PathBuf,
    title: String,
    body: usize,
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("set by Cargo"));
    println!("cargo::rerun-if-changed={DOCS}");
    let docs = root.join(DOCS);
    let mut found = Vec::new();
    walk(&docs, &docs, &mut found);
    found.sort_by(reading_order);

    let mut table = String::from("[\n");
    for page in &found {
        writeln!(
            table,
            "    Page {{ path: {:?}, title: {:?}, source: include_str!({:?}), body: {} }},",
            page.path,
            page.title,
            page.file.display().to_string(),
            page.body,
        )
        .expect("writing to a String cannot fail");
    }
    table.push(']');
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("set by Cargo")).join("pages.rs");
    fs::write(&out, table)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", out.display()));
}

fn walk(docs: &Path, dir: &Path, found: &mut Vec<Found>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()));
    for entry in entries {
        let file = entry
            .unwrap_or_else(|error| panic!("cannot read an entry of {}: {error}", dir.display()))
            .path();
        if file.is_dir() {
            walk(docs, &file, found);
            continue;
        }
        let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        // Starlight leaves out a file whose name starts with `_`.
        if name.starts_with('_') || file.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", file.display()));
        let (title, body) = front_matter(&source)
            .unwrap_or_else(|| panic!("{} has no front matter with a `title:`", file.display()));
        found.push(Found {
            path: site_path(docs, &file),
            file,
            title,
            body,
        });
    }
}

/// The path the site serves `file` at, without the slashes around it.
fn site_path(docs: &Path, file: &Path) -> String {
    let relative = file
        .strip_prefix(docs)
        .expect("walked from docs")
        .with_extension("");
    let mut parts: Vec<&str> = relative
        .components()
        .map(|part| part.as_os_str().to_str().expect("a UTF-8 file name"))
        .collect();
    if parts.last() == Some(&"index") {
        parts.pop();
    }
    parts.join("/")
}

/// The front matter's `title:` and the byte offset its closing `---` line ends at.
fn front_matter(source: &str) -> Option<(String, usize)> {
    let mut lines = source.split_inclusive('\n');
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut offset = source.find('\n')? + 1;
    let mut title = None;
    for line in lines {
        offset += line.len();
        let line = line.trim_end();
        if line == "---" {
            return Some((title?, offset));
        }
        if let Some(value) = line.strip_prefix("title:") {
            title = Some(unquote(value.trim()));
        }
    }
    None
}

/// A YAML scalar written plain, single-quoted or double-quoted.
fn unquote(value: &str) -> String {
    if let Some(inner) = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        inner.replace("\\\"", "\"").replace("\\\\", "\\")
    } else if let Some(inner) = value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        inner.replace("''", "'")
    } else {
        value.to_owned()
    }
}

/// English before each translation, a directory's index first, numbered chapters by number.
fn reading_order(a: &Found, b: &Found) -> Ordering {
    sort_key(a).cmp(&sort_key(b))
}

fn sort_key(page: &Found) -> (usize, &str, u8, u32, &str) {
    let path = page.path.as_str();
    let locale = LOCALES
        .iter()
        .position(|locale| path == *locale || path.starts_with(&format!("{locale}/")))
        .map_or(0, |index| index + 1);
    let is_index = page.file.file_stem().and_then(|stem| stem.to_str()) == Some("index");
    let dir = if is_index {
        path
    } else {
        path.rsplit_once('/').map_or("", |(dir, _)| dir)
    };
    let number = page
        .title
        .split_once(". ")
        .and_then(|(number, _)| number.parse::<u32>().ok());
    let rank = match (is_index, number) {
        (true, _) => 0,
        (false, Some(_)) => 1,
        (false, None) => 2,
    };
    (locale, dir, rank, number.unwrap_or(0), path)
}
