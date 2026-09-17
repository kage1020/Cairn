//! The crate README's subcommand table names exactly the subcommands the
//! binary dispatches.
//!
//! Holding the table to that is the point: a subcommand can land without
//! anything failing, because prose has no compiler, and the front page went on
//! calling the table "planned" long after the commands under it worked.
//!
//! The comparison is against `cairn --help` rather than against the `Command`
//! enum, so a variant that is declared but never reachable does not count as
//! shipped here either. What a row *says* stays unchecked: nothing here can
//! tell whether "Runs no check passes" is still true of `cairn parse`. This
//! holds the inventory, not the prose.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

/// clap's own subcommand, which exists on every binary and documents itself.
/// A README row for it would describe the help system rather than Cairn.
const BUILTIN: &str = "help";

/// The subcommand names `cairn --help` lists.
///
/// clap prints them under `Commands:`, one per line, indented two spaces, with
/// the description following on the same line however long it runs — the
/// `synth` description is a single line of some two thousand characters, since
/// clap only wraps with its `wrap_help` feature and this crate takes
/// `features = ["derive"]`. The indent filter therefore matches every line in
/// the block today, and is kept for the case `wrap_help` is switched on later,
/// where it is what keeps a wrapped description from being read as a
/// subcommand.
fn subcommands_the_binary_offers() -> BTreeSet<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_cairn"))
        .arg("--help")
        .output()
        .expect("failed to invoke cairn binary");
    assert!(output.status.success(), "cairn --help exited non-zero");
    let help = String::from_utf8(output.stdout).expect("stdout is utf-8");

    let commands = help
        .split_once("\nCommands:\n")
        .expect("cairn --help printed no `Commands:` section")
        .1
        .split("\n\n")
        .next()
        .expect("split always yields a first element");

    commands
        .lines()
        .filter_map(|line| {
            let indent = line.len() - line.trim_start().len();
            line.split_whitespace().next().filter(|_| indent == 2)
        })
        .map(str::to_owned)
        .filter(|name| name != BUILTIN)
        .collect()
}

/// The subcommand named by each row of the table under `## Subcommands`.
///
/// Every first cell reads `` `cairn <name> …` ``, so the name is the second
/// word of the row's first code span.
fn subcommands_named_by_readme() -> BTreeSet<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
    let readme = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

    let table = readme
        .split_once("\n## Subcommands\n")
        .expect("README has no `## Subcommands` section")
        .1
        .split("\n## ")
        .next()
        .expect("split always yields a first element");

    let mut named = BTreeSet::new();
    for row in table.lines() {
        let row = row.trim();
        // Rows only: the header (`| Subcommand |`) and the `|---|` separator
        // carry no code spans, so they fall out on their own.
        let Some(first_cell) = row
            .strip_prefix('|')
            .and_then(|rest| rest.split('|').next())
        else {
            continue;
        };
        let Some(invocation) = first_cell.split('`').nth(1) else {
            continue;
        };
        let mut words = invocation.split_whitespace();
        assert_eq!(
            words.next(),
            Some("cairn"),
            "a subcommand row's first cell does not start with `cairn`: {first_cell}",
        );
        if let Some(name) = words.next() {
            named.insert(name.to_owned());
        }
    }
    named
}

#[test]
fn the_subcommand_table_names_every_subcommand_the_binary_offers() {
    let offered = subcommands_the_binary_offers();
    let named = subcommands_named_by_readme();

    assert!(
        !offered.is_empty(),
        "no subcommands found in `cairn --help` — the parse above has stopped matching clap's \
         output",
    );

    let undocumented: Vec<_> = offered.difference(&named).collect();
    assert!(
        undocumented.is_empty(),
        "the binary dispatches these subcommands but the README's table does not name them: \
         {undocumented:?}. A shipped subcommand that the front page does not list is a feature \
         nobody can find.",
    );
}

#[test]
fn the_subcommand_table_names_nothing_the_binary_refuses() {
    let offered = subcommands_the_binary_offers();
    let named = subcommands_named_by_readme();

    let phantom: Vec<_> = named.difference(&offered).collect();
    assert!(
        phantom.is_empty(),
        "the README's table names subcommands the binary does not dispatch: {phantom:?}. A row \
         for a subcommand that does not exist is the failure this test was written for, in the \
         other direction — a reader types it and gets an error.",
    );
}
