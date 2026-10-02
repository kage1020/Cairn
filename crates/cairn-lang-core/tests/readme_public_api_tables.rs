//! A crate README's `## Public API` table names exactly the items that
//! crate's root re-exports.
//!
//! The table is an inventory, and an inventory nobody checks drifts. This one
//! had: `cairn-lang-formats` published a table of 13 rows carrying 17 code
//! spans, against 36 re-exports, so the front page crates.io shows answered
//! "what can I call" with a third of the answer, and had done for as long as it
//! took nineteen re-exports to accumulate without a row.
//!
//! **What `## Public API` means.** The crate root's `pub use` re-exports, and
//! nothing else. The wider reading — every `pub` item inside every `pub mod` —
//! is a different and much larger set: `cairn-lang-formats`' `registry` is a
//! directory module whose own `mod.rs` re-exports thirty-odd names, of which
//! the crate root lifts eleven. The other twenty-two, `registry::AliasCatalog`
//! and `registry::BlocksIndex` among them, are deliberately off the table, as
//! is `portability::PortabilityEntries`, which is the whole reason the rule is
//! written down in both READMEs rather than left to be inferred from a row
//! count.
//!
//! **What a row names.** One full `module::item` path per code span. A row may
//! hold several spans when the items share one sentence — `data_version::JavaTarget`
//! beside `data_version::resolve_java_target` — and every span is the path the
//! crate root's `pub use` spells, prefix included, so a move that changes that
//! spelling makes the row fail here rather than in a reader's head. Nothing is
//! resolved: `registry::PackSource` is a row about an item defined in
//! `registry::load`, and moving it to another file under `registry/` leaves both
//! the spelling and the row intact.
//!
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
//! sibling crates, has nothing to read: every test that reads the tree returns
//! early, and the fixture tests run regardless.

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
/// `cairn-lang-lsp`, `cairn-lang-redstone`, `cairn-lang-tree-sitter` and
/// `cairn-lang-wasm` are absent because they publish no such table yet; adding
/// one means adding it here, and the test below says so by name. That is all
/// eight workspace members.
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

/// The workspace's `crates/` directory, or `None` when this is not running
/// from a checkout.
///
/// The predicate is the directory this file reads, rather than a marker in the
/// workspace manifest, because a skip here is silent: it leaves every
/// tree-reading test green having read nothing, while `CONTRIBUTING.md` and the
/// CHANGELOG go on saying the tables are held by a test. A `[workspace]` line
/// that grows a trailing comment, or a formatter that respaces it, would be
/// enough. Asking for `crates/` cannot drift that way, and still answers the
/// case it has to: a packaged crate unpacked into a registry directory has no
/// sibling crates at all. [`the_skip_is_taken_only_outside_a_checkout`] holds
/// the other half.
fn crates_dir() -> Option<PathBuf> {
    let dir = repo_root().join("crates");
    dir.is_dir().then_some(dir)
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
///
/// [`every_shape_the_parse_refuses_says_so`] exercises each of those panics,
/// because an assert nothing reaches is an assert a refactor can drop without
/// anything going red.
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

    // Only a `pub use ` that starts its own line is read, and the count says so
    // out loud. Anchoring on the newline is what keeps a `pub use` inside an
    // inline `pub mod { … }` block from being counted as a crate-root
    // re-export, but on its own it would also drop an attributed, indented, or
    // second-on-the-line one without a word. `cargo fmt` normalises most of
    // those to column 0, so this is a guard against the shapes fmt leaves
    // alone — `#[rustfmt::skip]`, a macro body — rather than a live defect.
    // An inline `pub mod { pub use … }` trips it too, which is the intended
    // answer: that re-export is not the crate root's, and the parse should be
    // taught before anyone assumes either reading.
    let anchored = code.split("\npub use ").count() - 1;
    let written = code.matches("pub use ").count();
    assert_eq!(
        anchored, written,
        "this crate root holds a `pub use` that does not begin its own line — an attribute or \
         another statement ahead of it, an indent, or an inline `mod` block around it. Only the \
         ones at column 0 are read, so that re-export would be dropped in silence, and a \
         re-export missing from this set can never be reported as missing from the README. Put \
         it on a line of its own at the crate root, or teach the parse the shape.",
    );

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

    // Before anything else reads the path, because `crate` and `self` name
    // where the item is written rather than how a caller says it — and because
    // stripping after the module prefix is taken would leave a bare `crate` or
    // `self` standing in a row's path.
    let path = strip_local_prefix(&flat);

    let Some((prefix, rest)) = path.split_once('{') else {
        assert!(
            path.contains("::"),
            "a re-export with no `module::` prefix has no path a row can write: \
             `pub use {flat};`. Decide how such a row is spelled before teaching the parse \
             this shape.",
        );
        items.insert(path.to_owned());
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

    let prefix = prefix.trim();
    assert!(
        !prefix.is_empty(),
        "a braced re-export list with no module ahead of it publishes names with no \
         `module::` prefix to write in a row: `pub use {flat};`. Decide how such a row is \
         spelled before teaching the parse this shape.",
    );
    let prefix = prefix.strip_suffix("::").unwrap_or_else(|| {
        panic!("a braced re-export list has no `::` before its `{{`: `pub use {flat};`")
    });

    for item in inner.split(',') {
        let item = item.trim();
        // The trailing comma rustfmt leaves on a wrapped list.
        if item.is_empty() {
            continue;
        }
        assert!(
            item != "self",
            "`self` in a re-export list publishes the module, not an item: `pub use {flat};`. \
             The table is an inventory of items, so decide whether a module earns a row before \
             teaching the parse this shape.",
        );
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
///
/// Three constraints on how the section may be written, each because the
/// alternative fails for a reason that does not resemble the cause:
///
/// - the section ends at the next heading of any level, so a `### Deprecated`
///   table below it is a different section and not more rows;
/// - a fenced block inside the section is skipped, so the row format can be
///   documented by example without the example becoming rows;
/// - a row is a line starting with `|`, and its first cell's backticks must
///   pair. GFM allows a table without leading pipes; this does not, and
///   [`measured`] says so when a table comes back empty.
fn items_named_by_table(readme: &str) -> BTreeSet<String> {
    let mut lines = readme
        .lines()
        .skip_while(|line| line.trim_end() != "## Public API");
    assert!(
        lines.next().is_some(),
        "README has no `## Public API` section",
    );

    let mut named = BTreeSet::new();
    let mut fenced = false;
    for row in lines.take_while(|line| !line.starts_with('#')) {
        let row = row.trim();
        if row.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        // Rows only. The prose around the table carries code spans of its own —
        // the module paths the rule names as deliberately absent — and none of
        // it starts with a `|`. The header and the `|---|` separator hold no
        // spans and fall out on their own.
        let Some(first_cell) = row
            .strip_prefix('|')
            .and_then(|rest| rest.split('|').next())
        else {
            continue;
        };
        assert!(
            first_cell.matches('`').count() % 2 == 0,
            "a table row's first cell has an unpaired backtick, so the span it opens runs to \
             the end of the cell: {row:?}. An unclosed span is harvested as a path and \
             reported as one, which is a confusing way to learn about a typo.",
        );
        for span in first_cell.split('`').skip(1).step_by(2) {
            named.insert(span.trim().to_owned());
        }
    }
    named
}

/// The re-exports and the table rows of one crate.
fn measured(crates: &Path, crate_name: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let dir = crates.join(crate_name);
    let reexported = reexported_items(&read(&dir.join("src/lib.rs")));
    assert!(
        !reexported.is_empty(),
        "no crate-root `pub use` found in {crate_name}/src/lib.rs — the parse has stopped \
         matching the file",
    );
    let named = items_named_by_table(&read(&dir.join("README.md")));
    assert!(
        !named.is_empty(),
        "{crate_name}'s `## Public API` section has a heading but no rows this reads. A row is \
         a line starting with `|`; a GFM table written without leading pipes is not read, and \
         would leave the phantom direction below checking nothing.",
    );
    (reexported, named)
}

#[test]
fn the_table_names_every_reexported_item() {
    let Some(crates) = crates_dir() else {
        return;
    };
    for crate_name in TABLED_CRATES {
        let (reexported, named) = measured(&crates, crate_name);
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
    let Some(crates) = crates_dir() else {
        return;
    };
    for crate_name in TABLED_CRATES {
        let (reexported, named) = measured(&crates, crate_name);
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
/// loses its heading while still listed fails the two tests above on a missing
/// `## Public API` section, which is a different thing from deciding to stop
/// guarding it — a distinction worth failing on explicitly.
#[test]
fn exactly_these_crates_carry_a_public_api_table() {
    let Some(crates) = crates_dir() else {
        return;
    };

    let mut found = BTreeSet::new();
    for entry in fs::read_dir(&crates).expect("crates/ is readable") {
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

/// The skip is taken only outside a checkout.
///
/// Every tree-reading test above returns early when [`crates_dir`] finds
/// nothing, so a skip taken inside a workspace leaves all of them green having
/// read no file at all. The marker here is a sibling crate reached from
/// `CARGO_MANIFEST_DIR` directly, rather than through [`repo_root`], so a
/// `repo_root` that drifts by a directory is caught rather than silently
/// switching the guard off.
#[test]
fn the_skip_is_taken_only_outside_a_checkout() {
    let sibling = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../cairn-lang-formats/src/lib.rs")
        .is_file();
    if sibling {
        assert!(
            crates_dir().is_some(),
            "a sibling crate is right there, so this is a checkout, but `crates_dir` found \
             nothing and every test above would have skipped in silence",
        );
    }
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
        ("pub use self::tag::Tag;", vec!["tag::Tag"]),
        (
            "pub use crate::registry::{PackEdition};",
            vec!["registry::PackEdition"],
        ),
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

/// Every shape the parse refuses, refused with a message that names it.
///
/// This file's whole claim is that a shape it cannot read stops the test rather
/// than vanishing from the set, and that claim lives entirely in asserts no
/// other test reaches. Nothing above goes red if one is dropped while its
/// `as`-form support is half-written, or moved below the `insert` it was
/// guarding — every one of those leaves five green tests and a re-export
/// quietly gone. The expected fragment is pinned so a panic for some other
/// reason is not mistaken for the right one.
///
/// The first four are shapes `cargo fmt` would normalise to column 0, so in
/// this repo they reach the parse only under `#[rustfmt::skip]` or inside a
/// macro body. They are listed anyway: the fmt gate is not this test's to rely
/// on.
#[test]
fn every_shape_the_parse_refuses_says_so() {
    for (source, expected) in [
        (
            "#[cfg(feature = \"x\")] pub use tag::Tag;",
            "does not begin its own line",
        ),
        ("    pub use tag::Tag;", "does not begin its own line"),
        ("pub use a::B; pub use c::D;", "does not begin its own line"),
        (
            "pub mod outer { pub use inner::Thing; }",
            "does not begin its own line",
        ),
        ("/* a block */\npub use tag::Tag;", "block comment"),
        ("pub use tag::Tag", "never closed by a `;`"),
        ("pub use tag::*;", "glob re-export"),
        ("pub use tag::Tag as Tree;", "renaming re-export"),
        (
            "pub use registry::{load::{PackSource}, manifest};",
            "nested braces",
        ),
        ("pub use registry{PackEdition};", "no `::` before its `{`"),
        ("pub use crate::{Compound, Tag};", "no module ahead of it"),
        ("pub use crate::Tag;", "no `module::` prefix"),
        (
            "pub use registry::{self, PackEdition};",
            "publishes the module, not an item",
        ),
    ] {
        let panicked = std::panic::catch_unwind(|| reexported_items(source));
        let payload = panicked.expect_err(&format!(
            "{source:?} was read without a panic; this parse is supposed to refuse it",
        ));
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("<non-string panic payload>");
        assert!(
            message.contains(expected),
            "{source:?} panicked, but not for the stated reason. Expected the message to \
             contain {expected:?}; it was: {message}",
        );
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
/// The rest of the fixture is the shapes a section can take: a row naming two
/// items that share one sentence, prose carrying code spans of its own, a
/// fenced block documenting the row format by example, a `###` subheading with
/// a table of its own, and a later `##` section.
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

A row is written like this:

```text
| `module::item` | What it is for. |
```

### Deprecated

| Item | Role |
|---|---|
| `old::Gone` | A subheading starts a different section. |

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

/// An unpaired backtick in a row's first cell stops the test.
///
/// Separate from the fixture above because it is a panic, and because the shape
/// is a typo rather than a way a section can legitimately be written.
#[test]
#[should_panic(expected = "unpaired backtick")]
fn an_unclosed_span_in_a_row_is_refused() {
    items_named_by_table(
        "## Public API\n\n| Item | Role |\n|---|---|\n| `tag::Tag` and `tag::List | x |\n",
    );
}
