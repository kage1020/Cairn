//! Every subcommand that writes to stdout, plus clap's `--version` and
//! `--help`, run with a stdout whose reader has already gone
//! (`cairn lower f.crn | head -1` once `head` exits).
//!
//! The exit code is the Stable surface here (the CLI README's "Exit
//! codes"), so a reader that stops early must not change it: the run
//! ends with the code it decides with stdout open, and says the same
//! thing on stderr, rather than panicking on the failed write.

use std::process::{Command, Output, Stdio};

mod common;
use common::{Fixture, cairn_argv, cargo_bin, examples_dir, write_source};

/// Run `cairn argv` with stdout the write end of a pipe whose read end is
/// closed before the child starts, so its first write to stdout fails
/// with a broken pipe on every platform — no race with a reader that
/// exits after the child has written.
fn with_closed_stdout(argv: &[&str]) -> Output {
    let (reader, writer) = std::io::pipe().expect("create pipe");
    drop(reader);
    Command::new(cargo_bin())
        .args(argv)
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .expect("failed to invoke cairn binary")
}

fn example(name: &str) -> String {
    examples_dir().join(name).to_str().unwrap().to_owned()
}

/// Run `open_argv` with stdout open and `closed_argv` with it closed, and
/// return the open run.
///
/// The open run must exit with `code`, so a case cannot drift onto
/// another path that still exits the same way both times, and it must
/// write to stdout, or the closed run never reaches a failing write and
/// passes by default. The two argv may differ only in paths (two copies
/// of one source); stderr is compared with each open path replaced by its
/// closed counterpart.
fn assert_closed_stdout_keeps_the_verdict(
    open_argv: &[&str],
    closed_argv: &[&str],
    code: i32,
) -> Output {
    let open = cairn_argv(open_argv);
    let open_stderr = String::from_utf8_lossy(&open.stderr);
    assert_eq!(
        open.status.code(),
        Some(code),
        "`cairn {}` with stdout open; stderr={open_stderr}",
        open_argv.join(" "),
    );
    assert!(
        !open.stdout.is_empty(),
        "`cairn {}` writes nothing to stdout, so a closed stdout proves nothing",
        open_argv.join(" "),
    );
    let closed = with_closed_stdout(closed_argv);
    let closed_stderr = String::from_utf8_lossy(&closed.stderr);
    assert!(
        !closed_stderr.contains("panicked"),
        "`cairn {}` panicked on a closed stdout: {closed_stderr}",
        closed_argv.join(" "),
    );
    assert_eq!(
        closed.status.code(),
        Some(code),
        "`cairn {}` exits differently once stdout is closed; stderr={closed_stderr}",
        closed_argv.join(" "),
    );
    let mut expected_stderr = open_stderr.into_owned();
    for (open_arg, closed_arg) in open_argv.iter().zip(closed_argv) {
        if open_arg != closed_arg {
            expected_stderr = expected_stderr.replace(open_arg, closed_arg);
        }
    }
    assert_eq!(
        closed_stderr,
        expected_stderr,
        "`cairn {}` reports differently once stdout is closed",
        closed_argv.join(" "),
    );
    open
}

fn same_argv(argv: &[&str], code: i32) -> Output {
    assert_closed_stdout_keeps_the_verdict(argv, argv, code)
}

#[test]
fn parse_keeps_its_exit_code() {
    let village = example("village.crn");
    same_argv(&["parse", &village, "--format", "json"], 0);
    same_argv(&["parse", &village, "--format", "debug"], 0);
}

#[test]
fn lower_keeps_its_exit_code() {
    let village = example("village.crn");
    let ascii = same_argv(&["lower", &village, "--format", "ascii"], 0);
    // `village.crn`'s stairs are what reach the palette line that prints
    // block-state properties (`[  4] c  minecraft:spruce_stairs[…]`);
    // without one, that line leaves the matrix.
    assert!(
        String::from_utf8_lossy(&ascii.stdout).lines().any(|line| {
            line.trim_start().starts_with('[')
                && line.ends_with(']')
                && line.matches('[').count() == 2
        }),
        "no palette entry with block-state properties in `lower --format ascii`",
    );
    same_argv(&["lower", &village, "--format", "json"], 0);
    same_argv(&["lower", &village, "--format", "debug"], 0);
    // The ASCII view's one line for a source with nothing to lower.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let empty = write_source(tmp.path(), "empty.crn", "");
    same_argv(&["lower", empty.to_str().unwrap(), "--format", "ascii"], 0);
}

#[test]
fn a_run_that_warns_says_so_with_stdout_closed() {
    // Every other case here writes nothing to stderr, so their stderr
    // comparison holds trivially. This one warns while it writes stdout.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let source = write_source(
        tmp.path(),
        "warns.crn",
        "theme t:\n\
         \x20\x20slot floor -> @spruce_planks\n\n\
         struct s size=2x2\n\
         \x20\x20floor mat_slot=floor\n\
         \x20\x20roof kind=flat overhang=1\n\
         \x20\x20stair kind=stairs side=front shape=inner_left\n",
    );
    let source = source.to_str().unwrap();
    for argv in [
        ["lower", source, "--format", "ascii"],
        ["info", source, "--format", "text"],
    ] {
        let open = same_argv(&argv, 0);
        assert!(
            String::from_utf8_lossy(&open.stderr).contains("warning[W_DEFERRED_MEMBER]"),
            "`cairn {}` no longer warns, so its stderr comparison holds trivially",
            argv.join(" "),
        );
    }
}

#[test]
fn info_keeps_its_exit_code() {
    let village = example("village.crn");
    same_argv(&["info", &village, "--format", "text"], 0);
    same_argv(&["info", &village, "--format", "json"], 0);
}

#[test]
fn check_keeps_both_verdicts() {
    // A refused source as well as an accepted one: the refusal's 1 is the
    // verdict a consumer that stops reading early still has to see.
    same_argv(&["check", &example("village.crn"), "--format", "json"], 0);
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let duplicate = write_source(
        tmp.path(),
        "duplicate.crn",
        "struct s size=4x4 size=5x5\n\
         \x20\x20walls height=4 height=5\n\
         \x20\x20door id=front\n\
         \x20\x20door id=front\n",
    );
    same_argv(
        &["check", duplicate.to_str().unwrap(), "--format", "json"],
        1,
    );
}

#[test]
fn synth_keeps_its_exit_code() {
    same_argv(
        &[
            "synth",
            "--experimental-logic-synth",
            &example("redstone-door.crn"),
        ],
        0,
    );
}

#[test]
fn clap_keeps_its_exit_code() {
    // clap writes these itself and discards its own write error, so they
    // never went through `outln!`. Pinned so a clap bump that stops
    // discarding it fails here.
    same_argv(&["--version"], 0);
    same_argv(&["--help"], 0);
}

#[test]
fn compile_reports_the_build_it_committed() {
    // The `wrote …` lines come after the build has committed, so the
    // closed run has to leave the same artifacts and lock as the open one.
    // Two copies, so neither run reads the other's lock.
    let village = std::fs::read_to_string(examples_dir().join("village.crn")).expect("read");
    let open_fixture = Fixture::new("cli_closed_stdout", "open", &village);
    let closed_fixture = Fixture::new("cli_closed_stdout", "closed", &village);
    let open_src = open_fixture.source();
    let closed_src = closed_fixture.source();
    assert_closed_stdout_keeps_the_verdict(
        &["compile", open_src.to_str().unwrap(), "--edition", "java"],
        &["compile", closed_src.to_str().unwrap(), "--edition", "java"],
        0,
    );
    let artifacts = closed_fixture.artifacts();
    assert!(
        artifacts.iter().any(|name| name == "s.crn.lock"),
        "no lock among {artifacts:?}",
    );
    assert_eq!(artifacts, open_fixture.artifacts());
}
