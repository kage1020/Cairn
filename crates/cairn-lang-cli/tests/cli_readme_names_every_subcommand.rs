//! The crate README's subcommand table must name exactly the subcommands the
//! binary dispatches.
//!
//! The README is the front page a crates.io visitor reads, and for a while it
//! read as a skeleton: a Status line saying every real subcommand returned a
//! "not implemented yet" error, over a heading that called the table below it
//! "Subcommands (planned)". By then `parse`, `check`, `info`, `lower`,
//! `compile`, and `synth` all did their work and several hundred assertions in
//! this directory held them to it. Nothing failed when each one landed,
//! because prose has no compiler, and the front page went on telling readers
//! the compiler did not exist.
//!
//! So the table is held to the one fact it is a table *of*, and against the
//! binary rather than against the `Command` enum: `--help` is the same list a
//! user sees, arrived at through clap, so a variant that is declared but never
//! reachable does not count as shipped here either.
//!
//! What the row *says* stays prose and stays unchecked — this cannot tell
//! whether "Runs no check passes" is still true of `cairn parse`. It holds the
//! inventory, which is the half that went wrong.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

/// clap's own subcommand, which exists on every binary and documents itself.
/// A README row for it would describe the help system rather than Cairn.
const BUILTIN: &str = "help";

/// The subcommand names `cairn --help` lists.
///
/// clap prints them one per line under `Commands:`, indented, with the
/// description following on the same line — descriptions here run to several
/// hundred characters and wrap, so a continuation line is any line indented
/// past the name column.
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
            // clap indents a subcommand by two spaces and wraps its
            // description well past that.
            (indent == 2).then(|| line.split_whitespace().next())?
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
