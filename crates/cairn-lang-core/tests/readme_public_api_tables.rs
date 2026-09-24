//! A crate README's `## Public API` table names exactly the items that
//! crate's root re-exports.
//!
//! The table is an inventory, and an inventory nobody checks drifts. This one
//! had: `cairn-lang-formats` published a table of 17 rows against 36
//! re-exports, so the front page crates.io shows answered "what can I call"
//! with less than half of the answer, and had done for as long as it took
//! nineteen `pub use` lines to land without one.
//!
//! **What `## Public API` means.** The crate root's `pub use` re-exports, and
//! nothing else. The wider reading — every `pub` item inside every `pub mod` —
//! is a different and much larger set: `cairn-lang-formats`' `registry` is a
//! directory module whose own `mod.rs` re-exports thirty-odd names, none of
//! which the crate root lifts. Under the rule taken here an item reachable
//! only as `registry::AliasCatalog` or `portability::PortabilityEntries` is
//! deliberately off the table, which is the whole reason the rule is written
//! down in both READMEs rather than left to be inferred from a row count.
//!
//! **What a row names.** One full `module::item` path per code span. A row may
//! hold several spans when the items share one sentence — `data_version::JavaTarget`
//! beside `data_version::resolve_java_target` — and every span is a path this
//! test resolves, so the module prefix is checked too: move an item between
//! modules and the row goes stale here rather than in a reader's head.
//! `cairn-lang-formats` used to write one of those pairs as
//! `portability::portability_for_java` / `_for_bedrock`, a shorthand that is
//! not a symbol; normalizing it away was part of the change that added this
//! test, so the parse never has to know about it.
//!
//! What a row *says* stays unchecked, as in `readme_names_every_module.rs`:
//! nothing here can tell whether a Role column still describes the item.
//!
//! It lives in `cairn-lang-core` for want of a workspace-level test target,
//! following `spec_references_name_sections.rs`; it reads other crates, not
//! this one. A packaged crate, unpacked into a registry directory holding no
//! sibling crates, has nothing to check and every test returns early.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The crates whose README carries a `## Public API` table.
///
/// Spelled out rather than discovered, and checked in both directions by
/// [`exactly_these_crates_carry_a_public_api_table`]. A discovered list would
/// make deleting the heading a way to switch the guard off silently, which is
/// the failure this file exists to stop.
///
/// `cairn-lang-core` and `cairn-lang-cli` are absent because their inventories
/// are a module table and a subcommand table, each with a test of its own.
/// `cairn-lang-lsp`, `cairn-lang-redstone` and `cairn-lang-wasm` are absent
/// because they publish no such table yet; adding one means adding it here, and
/// the test below says so by name.
const TABLED_CRATES: &[&str] = &["cairn-lang-formats", "cairn-lang-nbt"];

/// The directory two levels up from `crates/cairn-lang-core`, which is the
/// repository root when this runs from a checkout.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two directories below the repository root")
        .to_path_buf()
}

/// Whether `root` is the Cairn workspace rather than the registry directory a
/// packaged crate was unpacked into.
///
/// Published crates carry their own `Cargo.toml` but not the workspace one, and
/// `cargo package` rewrites the manifest it ships, so the `[workspace]` table is
/// the marker that survives exactly one of the two cases.
fn is_workspace_root(root: &Path) -> bool {
    fs::read_to_string(root.join("Cargo.toml")).is_ok_and(|manifest| {
        manifest
            .lines()
            .any(|line| line.trim_end() == "[workspace]")
    })
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

/// Every item the crate root re-exports, each as the full `module::item` path
/// the `pub use` spells it with.
///
/// Anything this cannot read is a panic rather than a quiet skip. A re-export
/// dropped here is one that can never be reported as missing from the README —
/// the failure this file exists to catch would pass — so every shape the parse
/// does not understand has to stop the test instead.
fn reexported_items(source: &str) -> BTreeSet<String> {
    assert!(
        !source.contains("/*"),
        "this crate root carries a block comment, which the line-wise comment strip below does \
         not understand. A `pub use` inside one would be read as real code, and one outside a \
         comment that this strip mangles would be read as nothing at all. Teach the parse block \
         comments, or keep the file on `//`.",
    );
    let code = source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");

    // A leading newline so a `pub use` on the file's first line is found by the
    // same split as every other one.
    let code = format!("\n{code}");
    let mut items = BTreeSet::new();
    for tail in code.split("\npub use ").skip(1) {
        let (statement, _) = tail
            .split_once(';')
            .unwrap_or_else(|| panic!("a crate-root `pub use` is never closed by a `;`: {tail:?}"));
        expand_reexport_into(statement, &mut items);
    }
    items
}

/// Expand one `pub use` statement — everything between `pub use ` and its `;` —
/// into the paths it publishes.
///
/// Whitespace is flattened first, so the braced list rustfmt wraps over four
/// lines reads the same as the one it keeps on one.
fn expand_reexport_into(statement: &str, items: &mut BTreeSet<String>) {
    let flat = statement.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        !flat.contains('*'),
        "a glob re-export cannot be enumerated, so the README table cannot be held to it: \
         `pub use {flat};`. Name the items, or teach this test what the glob covers.",
    );
    assert!(
        !flat.contains(" as "),
        "a renaming re-export publishes a name this parse does not read: `pub use {flat};`. \
         The table would have to name the new spelling; teach the parse the `as` form first.",
    );

    let Some((prefix, rest)) = flat.split_once('{') else {
        items.insert(strip_local_prefix(&flat).to_owned());
        return;
    };

    let inner = rest
        .trim()
        .strip_suffix('}')
        .unwrap_or_else(|| panic!("a braced re-export list is never closed: `pub use {flat};`"));
    assert!(
        !inner.contains('{'),
        "nested braces in a re-export list are not read: `pub use {flat};`. Flatten the \
         statement, or teach the parse to recurse — a path lost in here is a path the README \
         can omit for free.",
    );

    let prefix = prefix.trim().strip_suffix("::").unwrap_or_else(|| {
        panic!("a braced re-export list has no `::` before its `{{`: `pub use {flat};`")
    });
    let prefix = strip_local_prefix(prefix);
    for item in inner.split(',') {
        let item = item.trim();
        // The trailing comma rustfmt leaves on a wrapped list.
        if item.is_empty() {
            continue;
        }
        items.insert(format!("{prefix}::{item}"));
    }
}

/// Drop a `crate::` or `self::` prefix, which names where the item is written
/// rather than how a caller says it.
fn strip_local_prefix(path: &str) -> &str {
    path.strip_prefix("crate::")
        .or_else(|| path.strip_prefix("self::"))
        .unwrap_or(path)
}

/// The code spans in the first column of the table under `## Public API`.
///
/// Every span in the cell counts, not just the first, because a row may name a
/// type and the function that returns it together. That reserves the first cell
/// for item paths: a span there naming anything else is read as one and
/// reported as one.
fn items_named_by_table(readme: &str) -> BTreeSet<String> {
    let section = readme
        .split_once("\n## Public API\n")
        .expect("README has no `## Public API` section")
        .1
        .split("\n## ")
        .next()
        .expect("split always yields a first element");

    let mut named = BTreeSet::new();
    for row in section.lines() {
        // Rows only. The prose above the table carries code spans of its own —
        // the module paths the rule names as deliberately absent — and none of
        // it starts with a `|`. The header and the `|---|` separator hold no
        // spans and fall out on their own.
        let Some(first_cell) = row
            .trim()
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

/// The re-exports and the table rows of one crate, or `None` outside a
/// workspace checkout.
fn measured(crate_name: &str) -> Option<(BTreeSet<String>, BTreeSet<String>)> {
    let root = repo_root();
    if !is_workspace_root(&root) {
        return None;
    }
    let dir = root.join("crates").join(crate_name);
    let reexported = reexported_items(&read(&dir.join("src/lib.rs")));
    assert!(
        !reexported.is_empty(),
        "no crate-root `pub use` found in {crate_name}/src/lib.rs — the parse has stopped \
         matching the file",
    );
    Some((
        reexported,
        items_named_by_table(&read(&dir.join("README.md"))),
    ))
}

#[test]
fn the_table_names_every_reexported_item() {
    for crate_name in TABLED_CRATES {
        let Some((reexported, named)) = measured(crate_name) else {
            return;
        };
        let undocumented: Vec<_> = reexported.difference(&named).collect();
        assert!(
            undocumented.is_empty(),
            "{crate_name} re-exports these at its root but the `## Public API` table does not \
             name them: {undocumented:?}. Give each one a row, or drop the re-export if it was \
             not meant to be part of the crate's root surface. A row writes the full \
             `module::item` path, so a bare item name will not do.",
        );
    }
}

#[test]
fn the_table_names_nothing_the_crate_does_not_reexport() {
    for crate_name in TABLED_CRATES {
        let Some((reexported, named)) = measured(crate_name) else {
            return;
        };
        let phantom: Vec<_> = named.difference(&reexported).collect();
        assert!(
            phantom.is_empty(),
            "{crate_name}'s `## Public API` table names paths the crate root does not \
             re-export: {phantom:?}. A renamed or moved item needs its row corrected — the \
             module prefix is part of the path this checks. An item that is only `pub` inside a \
             module is not part of this table by the rule the README states; describe it in \
             prose if it needs describing.",
        );
    }
}

/// The set of crates carrying a `## Public API` heading is exactly
/// [`TABLED_CRATES`].
///
/// Both directions matter. A crate that grows a table without being listed here
/// gets no guard and drifts the way `cairn-lang-formats` did; a crate that
/// loses its heading while still listed would make the two tests above pass by
/// panicking nowhere near the reason.
#[test]
fn exactly_these_crates_carry_a_public_api_table() {
    let root = repo_root();
    if !is_workspace_root(&root) {
        return;
    }

    let mut found = BTreeSet::new();
    for entry in fs::read_dir(root.join("crates")).expect("crates/ is readable") {
        let dir = entry.expect("a readable directory entry").path();
        let readme = dir.join("README.md");
        if !readme.is_file() {
            continue;
        }
        if read(&readme).contains("\n## Public API\n") {
            let name = dir
                .file_name()
                .expect("a crate directory has a name")
                .to_string_lossy()
                .into_owned();
            found.insert(name);
        }
    }

    let listed: BTreeSet<String> = TABLED_CRATES
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    assert_eq!(
        found, listed,
        "the crates carrying a `## Public API` table are not the ones this test guards. \
         A new table needs its crate added to TABLED_CRATES; a table that has gone away needs \
         its crate removed, and that is a decision worth making on purpose.",
    );
}

/// The statement forms that would otherwise fall out of the parse silently.
///
/// Each is a shape a crate root could take tomorrow. A regression here does not
/// announce itself in the tests above — it makes them pass — so the parse is
/// exercised directly.
#[test]
fn a_reexport_is_never_dropped_silently() {
    for (source, expected) in [
        (
            "pub use bedrock::write_bedrock_uncompressed;",
            vec!["bedrock::write_bedrock_uncompressed"],
        ),
        (
            "pub use tag::{Compound, List, Tag};",
            vec!["tag::Compound", "tag::List", "tag::Tag"],
        ),
        (
            "pub use bedrock_structure::{\n    BedrockStructureError,\n    ParityNote,\n};",
            vec![
                "bedrock_structure::BedrockStructureError",
                "bedrock_structure::ParityNote",
            ],
        ),
        (
            "pub use registry::load::PackSource;",
            vec!["registry::load::PackSource"],
        ),
        ("pub use crate::tag::Tag;", vec!["tag::Tag"]),
        ("pub use tag::Tag; // the owned tree", vec!["tag::Tag"]),
        // Not re-exports, and not part of the published surface.
        ("use cairn_lang_nbt::Compound;", vec![]),
        ("pub(crate) use tag::Tag;", vec![]),
        ("//! pub use tag::Tag; in a doc comment", vec![]),
        ("/// pub use tag::Tag; in a rustdoc line", vec![]),
    ] {
        let expected: BTreeSet<String> = expected.into_iter().map(str::to_owned).collect();
        assert_eq!(reexported_items(source), expected, "misread {source:?}");
    }
}

/// A row's first cell is read as full paths, one per code span.
///
/// The prefix is half of what makes a row checkable: without it `PackEdition`
/// would satisfy a table wherever the item happened to live, and moving an item
/// between modules would leave the README quietly wrong — the state this file
/// exists to end. Comparing bare names would make every test above pass on
/// today's tree, so the rule is asserted here rather than left to a fixture
/// nobody can write without breaking a real README.
///
/// The other two cases are the shapes the tables actually take: a row naming
/// two items that share one sentence, and prose above the table carrying code
/// spans of its own.
#[test]
fn a_row_is_read_as_full_paths_only() {
    let readme = "\
# a crate

## Public API

Not a row, but it carries spans: `registry::AliasCatalog`, `portability::PortabilityEntries`.

| Item | Role |
|---|---|
| `registry::PackEdition` | Closed edition enum. |
| `data_version::JavaTarget` / `data_version::resolve_java_target` | One sentence, two items. |

## Something else

| Item | Role |
|---|---|
| `not_in::the_table` | A later section's table is not this one. |
";

    let named = items_named_by_table(readme);
    let expected: BTreeSet<String> = [
        "data_version::JavaTarget",
        "data_version::resolve_java_target",
        "registry::PackEdition",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(named, expected);

    // Spelled out, because an implementation that dropped the module prefix
    // would still satisfy the set comparison above if `reexported_items` were
    // mutated to match.
    assert!(
        !named.contains("PackEdition"),
        "a bare item name is not a row's identity; the module prefix is part of it",
    );
}
