//! The crate README's module table must name exactly the modules the crate
//! exports.
//!
//! The README is the front page a crates.io visitor reads, and for a while it
//! read as a skeleton: a Status line saying the crate exposed only
//! `CAIRN_VERSION`, and a table whose third column was headed "Future module"
//! over six rows that had already shipped. Nothing failed when the lexer, the
//! parser, the resolver, and the block-array lowering landed, because prose
//! has no compiler. The table went stale one merged stage at a time and the
//! only thing that could notice was a reader.
//!
//! So the table is held to the one fact it is a table *of*. Adding `pub mod
//! edit` without giving it a row fails here, and so does a row naming a module
//! that was renamed or withdrawn. What the row *says* is still prose and still
//! unchecked — this cannot tell whether "the diagnostic-collecting pipeline"
//! is a fair description of `check`. It holds the inventory, which is the half
//! that went wrong.
//!
//! The parse is deliberately textual rather than a macro over the module tree.
//! A `#[path]`-driven inventory would be generated from the same declarations
//! the README is supposed to be tracking by hand, and the test wants to read
//! the file a contributor edits.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

/// Every `pub mod` declared at the crate root.
///
/// `pub(crate) mod` is excluded: it is not part of the published surface, so
/// the README owes it no row. That is why `prose` does not appear below.
fn exported_modules() -> BTreeSet<String> {
    read(&crate_root().join("src/lib.rs"))
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub mod "))
        .filter_map(|rest| rest.strip_suffix(';'))
        .map(str::to_owned)
        .collect()
}

/// The code spans in the first column of the table under `## Modules`.
///
/// One row may name several modules — `lex`, `parse`, `ast` share a row
/// because they are one stage — so every span in the cell counts, not just
/// the first.
fn modules_named_by_readme() -> BTreeSet<String> {
    let readme = read(&crate_root().join("README.md"));

    let table = readme
        .split_once("\n## Modules\n")
        .expect("README has no `## Modules` section")
        .1
        .split("\n## ")
        .next()
        .expect("split always yields a first element");

    let mut named = BTreeSet::new();
    for row in table.lines() {
        let row = row.trim();
        // Rows only: the header (`| Module |`) and the `|---|` separator
        // carry no code spans, so they fall out on their own.
        let Some(first_cell) = row
            .strip_prefix('|')
            .and_then(|rest| rest.split('|').next())
        else {
            continue;
        };
        for span in first_cell.split('`').skip(1).step_by(2) {
            named.insert(span.trim().to_owned());
        }
    }
    named
}

#[test]
fn the_module_table_names_every_exported_module() {
    let exported = exported_modules();
    let named = modules_named_by_readme();

    assert!(
        !exported.is_empty(),
        "no `pub mod` found in src/lib.rs — the parse above has stopped matching the file",
    );

    let undocumented: Vec<_> = exported.difference(&named).collect();
    assert!(
        undocumented.is_empty(),
        "these modules ship but the README's module table does not name them: {undocumented:?}. \
         Give each one a row, or drop the `pub` if it was not meant to be part of the surface.",
    );
}

#[test]
fn the_module_table_names_nothing_the_crate_does_not_export() {
    let exported = exported_modules();
    let named = modules_named_by_readme();

    let phantom: Vec<_> = named.difference(&exported).collect();
    assert!(
        phantom.is_empty(),
        "the README's module table names modules this crate does not export: {phantom:?}. \
         A renamed module needs its row renamed; a module still ahead of the code belongs in \
         `## Not yet here`, as prose, not as a row in the inventory of what ships.",
    );
}
