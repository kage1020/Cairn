//! The workspace's `unsafe_code` policy, and the test that enforces it.
//!
//! This module doc is the one full statement of the policy. The comment on
//! the lint in the root `Cargo.toml`, the tree-sitter binding's `ffi` module
//! and `development.md` each give the short version and point here, so when
//! what this test checks changes, this is the text that changes with it.
//!
//! The workspace sets the lint to `deny`, not `forbid`, because `forbid`
//! refuses every inner `allow` or `expect` — including the one the generated
//! C parser needs to be reachable from Rust at all. `deny` can be lowered
//! again by any attribute, and Cargo's lint inheritance is opt-in per crate
//! and all-or-nothing: a crate without `[lints] workspace = true` never
//! receives the workspace lints at all. So this test checks, for every crate
//! `cargo metadata` reports as a workspace member — whether it is listed in
//! `members` or joined as a path dependency inside the workspace:
//!
//! 1. The root manifest's `[workspace.lints.rust]` sets `unsafe_code` to
//!    `"deny"`. Lowering it to `allow`, or deleting it, fails here.
//! 2. The crate inherits the workspace lints — `[lints]` then
//!    `workspace = true`, in any TOML spelling — and sets no lint of its own
//!    under `lints`. A `lints.workspace = true` written inside another table
//!    (`[package]`, say) is reported with its line: Cargo ignores it without
//!    a warning.
//! 3. No attribute that can lower a lint level — `allow`, `expect`, `warn`,
//!    or a `cfg_attr` wrapping one — names `unsafe_code` anywhere in the
//!    crate's Rust sources, outside the modules [`ALLOWED`] lists. Today that
//!    is one module: `ffi` in the tree-sitter binding. A crate-root
//!    `#![allow(unsafe_code)]` in that same file fails. Comments, string
//!    literals and identifiers do not count; only attributes do.
//!
//! Out of its reach: a level set outside the sources (`RUSTFLAGS`, a
//! `.cargo/config.toml`), and an attribute a macro assembles from tokens it
//! was handed.
//!
//! It lives in `cairn-lang-core` for want of a workspace-level test target.
//! Run from a packaged crate, whose unpacked directory is a workspace of its
//! own, it has nothing to check and says `skip:` rather than passing quietly.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The modules allowed to lift `unsafe_code`, as `(file, module)` with the
/// file relative to the workspace root.
///
/// An attribute naming the lint is accepted only inside the body of
/// `mod <module> { ... }` in that file. Every entry has to exist and has to
/// lift the lint, so a moved or emptied module fails rather than leaving a
/// stale exemption behind.
const ALLOWED: &[(&str, &str)] = &[("crates/cairn-lang-tree-sitter/bindings/rust/lib.rs", "ffi")];

/// A workspace member crate, as `cargo metadata` reports it.
struct Member {
    name: String,
    /// The directory holding its `Cargo.toml`.
    dir: PathBuf,
}

struct Workspace {
    root: PathBuf,
    members: Vec<Member>,
}

impl Workspace {
    fn relative<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.root).unwrap_or(path)
    }
}

/// The workspace this crate belongs to, read from
/// `cargo metadata --no-deps` — the same member set `cargo test --workspace`
/// builds, path dependencies included — or `None` when the crate is its own
/// workspace (unpacked from a registry), where there is no policy to check.
fn workspace() -> Option<Workspace> {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(&here)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");

    let root = PathBuf::from(
        metadata["workspace_root"]
            .as_str()
            .expect("cargo metadata names the workspace root"),
    );
    if same_dir(&root, &here) {
        eprintln!(
            "skip: {} is its own workspace (a packaged crate), so there is no workspace policy to check",
            here.display()
        );
        return None;
    }

    let ids: HashSet<&str> = metadata["workspace_members"]
        .as_array()
        .expect("cargo metadata lists the workspace members")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let mut members: Vec<Member> = metadata["packages"]
        .as_array()
        .expect("cargo metadata lists the packages")
        .iter()
        .filter(|package| package["id"].as_str().is_some_and(|id| ids.contains(id)))
        .map(|package| Member {
            name: package["name"]
                .as_str()
                .expect("a package has a name")
                .to_owned(),
            dir: Path::new(
                package["manifest_path"]
                    .as_str()
                    .expect("a package has a manifest path"),
            )
            .parent()
            .expect("a manifest sits in a directory")
            .to_path_buf(),
        })
        .collect();
    members.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        members.len(),
        ids.len(),
        "every workspace member id matches a package"
    );
    assert!(
        members.iter().any(|m| same_dir(&m.dir, &here)),
        "cairn-lang-core is not among the workspace members cargo metadata reported"
    );
    Some(Workspace { root, members })
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// One `key = value` line of a TOML manifest, with the key made absolute:
/// the enclosing table header joined to the (possibly dotted) key, so that
/// `[lints]` + `workspace = true` and a top-level `lints.workspace = true`
/// both read as `lints.workspace`.
struct Entry {
    line: usize,
    key: String,
    value: String,
}

/// The entries of a manifest, in order. Enough TOML for Cargo manifests:
/// comments, table headers, dotted and quoted keys, and values that span
/// lines (their continuation lines are skipped). Not multi-line strings.
fn entries(manifest: &str) -> Vec<Entry> {
    let mut found = Vec::new();
    let mut table = String::new();
    let mut open = 0i32;
    for (n, raw) in manifest.lines().enumerate() {
        let line = strip_toml_comment(raw).trim();
        if open > 0 {
            open += bracket_balance(line);
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            let header = header.trim_start_matches('[');
            let header = header.trim_end().trim_end_matches(']');
            table = normalize_key(header);
            continue;
        }
        let Some(eq) = find_outside_strings(line, '=') else {
            continue;
        };
        let key = normalize_key(&line[..eq]);
        let value = line[eq + 1..].trim().to_owned();
        open = bracket_balance(&value);
        found.push(Entry {
            line: n + 1,
            key: if table.is_empty() {
                key
            } else {
                format!("{table}.{key}")
            },
            value,
        });
    }
    found
}

/// `a . "b" .c` -> `a.b.c`.
fn normalize_key(key: &str) -> String {
    key.split('.')
        .map(|part| part.trim().trim_matches('"').trim_matches('\''))
        .collect::<Vec<_>>()
        .join(".")
}

/// The byte offset of the first `needle` not inside a TOML string.
fn find_outside_strings(line: &str, needle: char) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (at, c) in line.char_indices() {
        match quote {
            Some(q) => {
                if escaped {
                    escaped = false;
                } else if c == '\\' && q == '"' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            }
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == needle => return Some(at),
            None => {}
        }
    }
    None
}

fn strip_toml_comment(line: &str) -> &str {
    find_outside_strings(line, '#').map_or(line, |at| &line[..at])
}

/// Opening minus closing `[`/`{` outside strings.
fn bracket_balance(text: &str) -> i32 {
    let mut balance = 0;
    let mut quote = None;
    let mut escaped = false;
    for c in text.chars() {
        match quote {
            Some(q) => {
                if escaped {
                    escaped = false;
                } else if c == '\\' && q == '"' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '[' | '{' => balance += 1,
                ']' | '}' => balance -= 1,
                _ => {}
            },
        }
    }
    balance
}

/// The level a lint entry's value sets: `"deny"`, or the `level` of
/// `{ level = "deny", priority = 0 }`.
fn lint_level(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('{') {
        let inner = inner.trim_end().trim_end_matches('}');
        return inner.split(',').find_map(|pair| {
            let (key, level) = pair.split_once('=')?;
            (key.trim() == "level").then(|| level.trim().trim_matches('"').to_owned())
        });
    }
    Some(value.trim_matches('"').to_owned())
}

#[test]
fn the_workspace_denies_unsafe_code() {
    const KEY: &str = "workspace.lints.rust.unsafe_code";
    let Some(ws) = workspace() else {
        return;
    };
    let path = ws.root.join("Cargo.toml");
    let manifest =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let levels: Vec<(usize, Option<String>)> = entries(&manifest)
        .into_iter()
        .filter(|entry| entry.key == KEY || entry.key == format!("{KEY}.level"))
        .map(|entry| (entry.line, lint_level(&entry.value)))
        .collect();
    match levels.as_slice() {
        [(_, Some(level))] if level == "deny" => {}
        [] => panic!(
            "Cargo.toml: `[workspace.lints.rust]` does not set `unsafe_code`; it has to say \
             `unsafe_code = \"deny\"` — every other check in this file assumes it"
        ),
        found => panic!(
            "Cargo.toml: `[workspace.lints.rust]` has to set `unsafe_code = \"deny\"`, found {}",
            found
                .iter()
                .map(|(line, level)| format!(
                    "`{}` on line {line}",
                    level.as_deref().unwrap_or("?")
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[test]
fn every_member_inherits_the_workspace_lints() {
    let Some(ws) = workspace() else {
        return;
    };
    let mut problems = Vec::new();
    for member in &ws.members {
        let path = member.dir.join("Cargo.toml");
        let rel = ws.relative(&path).display().to_string();
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let mut inherits = false;
        for entry in entries(&text) {
            let compact: String = entry.value.split_whitespace().collect();
            if entry.key == "lints.workspace" {
                inherits |= entry.value == "true";
            } else if entry.key == "lints" {
                inherits |= compact == "{workspace=true}";
            } else if entry.key.starts_with("lints.") {
                problems.push(format!(
                    "{rel}:{}: `{}` sets a lint of the crate's own; a crate takes its lints \
                     from the workspace or not at all",
                    entry.line, entry.key
                ));
            } else if entry.key.ends_with(".lints.workspace") {
                problems.push(format!(
                    "{rel}:{}: `{}` is not `lints.workspace`, and Cargo ignores it without a \
                     warning; write `[lints]` then `workspace = true` at the top level",
                    entry.line, entry.key
                ));
            }
        }
        if !inherits {
            problems.push(format!(
                "{rel}: crate `{}` never sets `lints.workspace = true`, so it receives none of \
                 `[workspace.lints]`; add `[lints]` then `workspace = true`",
                member.name
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "every workspace member has to inherit [workspace.lints]:\n  {}",
        problems.join("\n  ")
    );
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `source` with every comment, string literal and char literal replaced by
/// spaces, newlines kept so that offsets still map to the same lines. Block
/// comments nest, as they do in Rust.
fn code_only(source: &str) -> Vec<char> {
    let src: Vec<char> = source.chars().collect();
    let mut out = src.clone();
    let blank = |out: &mut Vec<char>, from: usize, to: usize| {
        for c in &mut out[from..to.min(src.len())] {
            if *c != '\n' {
                *c = ' ';
            }
        }
    };
    let at = |i: usize| src.get(i).copied();
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        let starts_word = i == 0 || !is_ident(src[i - 1]);
        if c == '/' && at(i + 1) == Some('/') {
            let end = (i..src.len())
                .find(|&j| src[j] == '\n')
                .unwrap_or(src.len());
            blank(&mut out, i, end);
            i = end;
        } else if c == '/' && at(i + 1) == Some('*') {
            let mut depth = 0;
            let mut j = i;
            while j < src.len() {
                if src[j] == '/' && at(j + 1) == Some('*') {
                    depth += 1;
                    j += 2;
                } else if src[j] == '*' && at(j + 1) == Some('/') {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            blank(&mut out, i, j);
            i = j;
        } else if c == '"' {
            let mut j = i + 1;
            while j < src.len() && src[j] != '"' {
                j += if src[j] == '\\' { 2 } else { 1 };
            }
            blank(&mut out, i, j + 1);
            i = j + 1;
        } else if starts_word
            && matches!(c, 'r' | 'b' | 'c')
            && raw_string_hashes(&src, i).is_some()
        {
            let (open, hashes) = raw_string_hashes(&src, i).expect("checked above");
            let mut j = open + 1;
            while j < src.len() {
                if src[j] == '"' && (1..=hashes).all(|k| at(j + k) == Some('#')) {
                    break;
                }
                j += 1;
            }
            let end = j + 1 + hashes;
            blank(&mut out, i, end);
            i = end;
        } else if c == '\'' {
            // A char literal (`'x'`, `'\n'`, `'\u{..}'`) or a lifetime (`'a`).
            let end = if at(i + 1) == Some('\\') {
                (i + 2..src.len()).find(|&j| src[j] == '\'').map(|j| j + 1)
            } else if at(i + 2) == Some('\'') {
                Some(i + 3)
            } else {
                None
            };
            if let Some(end) = end {
                blank(&mut out, i, end);
                i = end;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

/// For a raw string starting at `i` (`r"`, `r#"`, `br##"`, `cr"`): the
/// offset of its opening quote and its number of `#`.
fn raw_string_hashes(src: &[char], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if matches!(src.get(j), Some('b' | 'c')) {
        j += 1;
    }
    if src.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let hashes = src[j..].iter().take_while(|&&c| c == '#').count();
    (src.get(j + hashes) == Some(&'"')).then_some((j + hashes, hashes))
}

/// An attribute that can lower a lint level and names `unsafe_code`.
struct Lift {
    /// Char offset of its `#`.
    at: usize,
    line: usize,
    text: String,
}

/// Every `#[...]` / `#![...]` in `code` whose path is `allow`, `expect`,
/// `warn` or `cfg_attr` and whose body names `unsafe_code` as a whole word.
/// Attributes that span lines are read whole.
fn lifts(code: &[char]) -> Vec<Lift> {
    const LOWERING: [&str; 4] = ["allow", "expect", "warn", "cfg_attr"];
    let skip_ws = |mut j: usize| {
        while code.get(j).is_some_and(|c| c.is_whitespace()) {
            j += 1;
        }
        j
    };
    let mut found = Vec::new();
    for (at, _) in code.iter().enumerate().filter(|(_, c)| **c == '#') {
        let mut j = skip_ws(at + 1);
        if code.get(j) == Some(&'!') {
            j = skip_ws(j + 1);
        }
        if code.get(j) != Some(&'[') {
            continue;
        }
        let Some(close) = matching(code, j, '[', ']') else {
            continue;
        };
        let body: String = code[j + 1..close].iter().collect();
        let path: String = body
            .trim_start()
            .chars()
            .take_while(|&c| is_ident(c))
            .collect();
        if LOWERING.contains(&path.as_str()) && names_word(&body, "unsafe_code") {
            found.push(Lift {
                at,
                line: code[..at].iter().filter(|&&c| c == '\n').count() + 1,
                text: code[at..=close]
                    .iter()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            });
        }
    }
    found
}

fn names_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(is_ident)
            && !text[at + word.len()..].chars().next().is_some_and(is_ident)
    })
}

/// The offset of the delimiter closing the one at `open`.
fn matching(code: &[char], open: usize, left: char, right: char) -> Option<usize> {
    let mut depth = 0;
    for (j, &c) in code.iter().enumerate().skip(open) {
        if c == left {
            depth += 1;
        } else if c == right {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
    }
    None
}

/// The char range strictly inside the braces of `mod <name> { ... }`.
fn module_body(code: &[char], name: &str) -> Option<(usize, usize)> {
    let text: String = code.iter().collect();
    let chars_before = |byte: usize| text[..byte].chars().count();
    text.match_indices("mod").find_map(|(at, _)| {
        if text[..at].chars().next_back().is_some_and(is_ident) {
            return None;
        }
        let rest = text[at + 3..].trim_start();
        let after_name = rest.strip_prefix(name)?;
        if after_name.chars().next().is_some_and(is_ident) || rest.len() == text[at + 3..].len() {
            return None;
        }
        let brace = after_name.trim_start();
        if !brace.starts_with('{') {
            return None;
        }
        let open = chars_before(text.len() - brace.len());
        let close = matching(code, open, '{', '}')?;
        Some((open, close))
    })
}

/// The [`ALLOWED`] entry for `rel`, with its module's body range in `code`.
fn allowed_module(
    rel: &Path,
    code: &[char],
) -> Option<(&'static str, &'static str, (usize, usize))> {
    let &(path, module) = ALLOWED.iter().find(|(path, _)| rel == Path::new(path))?;
    let Some(body) = module_body(code, module) else {
        panic!("{path} has no `mod {module} {{ ... }}`; if it moved, move ALLOWED with it");
    };
    Some((path, module, body))
}

#[test]
fn unsafe_code_is_lifted_only_by_the_tree_sitter_ffi() {
    let Some(ws) = workspace() else {
        return;
    };
    let mut files = Vec::new();
    for member in &ws.members {
        rust_files(&member.dir, &mut files);
    }
    files.sort();
    files.dedup();

    let mut unused: Vec<(&str, &str)> = ALLOWED.to_vec();
    let mut hits = Vec::new();
    for file in &files {
        let rel = ws.relative(file);
        let text =
            fs::read_to_string(file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let code = code_only(&text);
        let allowed_body = allowed_module(rel, &code);
        for lift in lifts(&code) {
            match allowed_body {
                Some((path, module, (open, close))) if lift.at > open && lift.at < close => {
                    unused.retain(|entry| *entry != (path, module));
                }
                _ => hits.push(format!("{}:{}: {}", rel.display(), lift.line, lift.text)),
            }
        }
    }
    assert!(
        unused.is_empty(),
        "these ALLOWED entries were not found, or no longer lift `unsafe_code`; move or \
         remove them: {unused:?}"
    );
    assert!(
        hits.is_empty(),
        "only the modules in ALLOWED ({ALLOWED:?}) may lift `unsafe_code`:\n  {}",
        hits.join("\n  ")
    );
}
