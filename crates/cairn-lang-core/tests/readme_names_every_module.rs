//! The crate README's module table names exactly the crate's `pub mod`
//! declarations.
//!
//! Holding the table to that is the point: a stage can land without anything
//! failing, because prose has no compiler, and the table went stale one merged
//! stage at a time with only a reader able to notice.
//!
//! `pub(crate) mod` is excluded — not published surface, so the README owes it
//! no row, which is why `prose` has none. What a row *says* stays unchecked:
//! nothing here can tell whether "the diagnostic-collecting pipeline" is still
//! a fair description of `check`. This holds the inventory, not the prose.
//!
//! The parse is textual rather than a macro over the module tree. A generated
//! inventory would be derived from the same declarations the README is
//! supposed to be tracking by hand, and the test wants to read the file a
//! contributor edits.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

/// The module name declared on one line of source, if it declares one.
///
/// Anything this cannot read is a panic rather than a `None`. A declaration
/// silently dropped here leaves its module out of `exported`, and a module not
/// in `exported` can never be reported as missing from the README — the
/// failure this file exists to catch would pass. `pub mod lock; // lockfile
/// schema` was enough to do it.
///
/// So the search is for `pub mod ` anywhere in the line's code, not at its
/// start: a `#[cfg(…)] pub mod foo;` on one line counts, as does the `pub mod
/// foo {` form. Trailing comments are cut first, which also drops `//!` and
/// `///` lines whole. `pub(crate) mod` does not contain `pub mod ` and so
/// falls out here rather than by a separate rule.
fn module_declared_on(line: &str) -> Option<String> {
    let code = line.split("//").next().unwrap_or(line);
    let rest = code.split_once("pub mod ")?.1;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    assert!(
        !name.is_empty(),
        "cannot read a module name out of a `pub mod` declaration: {line:?}. \
         This parse must never drop a declaration — a module missing from the set below \
         cannot be reported as missing from the README.",
    );
    Some(name)
}

/// Every `pub mod` declared at the crate root.
fn exported_modules() -> BTreeSet<String> {
    read(&crate_root().join("src/lib.rs"))
        .lines()
        .filter_map(module_declared_on)
        .collect()
}

/// The code spans in the first column of the table under `## Modules`.
///
/// One row may name several modules — `lex`, `parse`, `ast` share a row
/// because they are one stage — so every span in the cell counts, not just the
/// first. That reserves the first cell for module names: a span there naming
/// anything else is read as a module and reported as one.
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

/// The declaration forms that used to fall out of the parse silently.
///
/// Each one is a real shape `lib.rs` could take tomorrow. A regression here
/// does not announce itself in the two tests above — it makes them pass — so
/// the parse is exercised directly.
#[test]
fn a_declaration_is_never_dropped_silently() {
    for (line, expected) in [
        ("pub mod lock;", Some("lock")),
        ("pub mod lock; // lockfile schema", Some("lock")),
        ("#[cfg(feature = \"x\")] pub mod lock;", Some("lock")),
        ("pub mod lock {", Some("lock")),
        ("pub mod block_array;", Some("block_array")),
        // Not part of the published surface, and not a declaration at all.
        ("pub(crate) mod prose;", None),
        ("mod prose;", None),
        ("//! pub mod lock; in a doc comment", None),
        ("use cairn_lang_core::lock;", None),
    ] {
        assert_eq!(
            module_declared_on(line).as_deref(),
            expected,
            "misread {line:?}",
        );
    }
}
