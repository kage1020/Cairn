//! Every subcommand that writes to stdout, run with a stdout whose reader
//! has already gone (`cairn lower f.crn | head -1` once `head` exits).
//!
//! The exit code is the Stable surface here (the CLI README's "Exit
//! codes"), so a reader that stops early must not change it: the run
//! ends with the code it decides with stdout open, and says the same
//! thing on stderr, rather than panicking on the failed write.

use std::path::Path;
use std::process::{Command, Output, Stdio};

mod common;
use common::{cargo_bin, example_in_tempdir, examples_dir};

/// Run `cairn argv` with stdout captured.
fn with_open_stdout(argv: &[&str]) -> Output {
    Command::new(cargo_bin())
        .args(argv)
        .output()
        .expect("failed to invoke cairn binary")
}

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

/// Run `argv` both ways and compare. The open run must write to stdout,
/// or the closed run never reaches a failing write and passes by default.
fn assert_closed_stdout_keeps_the_verdict(open_argv: &[&str], closed_argv: &[&str]) {
    let open = with_open_stdout(open_argv);
    assert!(
        !open.stdout.is_empty(),
        "`cairn {}` writes nothing to stdout, so a closed stdout proves nothing",
        open_argv.join(" "),
    );
    let closed = with_closed_stdout(closed_argv);
    let open_stderr = String::from_utf8_lossy(&open.stderr);
    let closed_stderr = String::from_utf8_lossy(&closed.stderr);
    assert!(
        !closed_stderr.contains("panicked"),
        "`cairn {}` panicked on a closed stdout: {closed_stderr}",
        closed_argv.join(" "),
    );
    assert_eq!(
        closed.status.code(),
        open.status.code(),
        "`cairn {}` exits differently once stdout is closed; stderr={closed_stderr}",
        closed_argv.join(" "),
    );
    assert_eq!(
        closed_stderr,
        open_stderr,
        "`cairn {}` reports differently once stdout is closed",
        closed_argv.join(" "),
    );
}

fn same_argv(argv: &[&str]) {
    assert_closed_stdout_keeps_the_verdict(argv, argv);
}

#[test]
fn parse_keeps_its_exit_code() {
    let village = example("village.crn");
    same_argv(&["parse", &village, "--format", "json"]);
    same_argv(&["parse", &village, "--format", "debug"]);
}

#[test]
fn lower_keeps_its_exit_code() {
    let village = example("village.crn");
    same_argv(&["lower", &village, "--format", "ascii"]);
    same_argv(&["lower", &village, "--format", "json"]);
    same_argv(&["lower", &village, "--format", "debug"]);
    // The ASCII view's one line for a source with nothing to lower.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let empty = common::write_source(tmp.path(), "empty.crn", "");
    same_argv(&["lower", empty.to_str().unwrap(), "--format", "ascii"]);
}

#[test]
fn info_keeps_its_exit_code() {
    let village = example("village.crn");
    same_argv(&["info", &village, "--format", "text"]);
    same_argv(&["info", &village, "--format", "json"]);
}

#[test]
fn check_keeps_both_verdicts() {
    // A refused source as well as an accepted one: the refusal's 1 is the
    // verdict a consumer that stops reading early still has to see.
    same_argv(&["check", &example("village.crn"), "--format", "json"]);
    let duplicate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cairn-lang-core/tests/fixtures/check/duplicate.crn");
    let duplicate = duplicate.to_str().unwrap();
    let open = with_open_stdout(&["check", duplicate, "--format", "json"]);
    assert_eq!(open.status.code(), Some(1), "the fixture must be refused");
    same_argv(&["check", duplicate, "--format", "json"]);
}

#[test]
fn synth_keeps_its_exit_code() {
    same_argv(&[
        "synth",
        "--experimental-logic-synth",
        &example("redstone-door.crn"),
    ]);
}

#[test]
fn compile_reports_the_build_it_committed() {
    // The `wrote …` lines come after the build has committed, so the
    // artifacts and the lock have to be there as well as the exit code.
    // Two copies, so neither run reads the other's lock.
    let (_open_dir, open_src) = example_in_tempdir("village.crn");
    let (closed_dir, closed_src) = example_in_tempdir("village.crn");
    let open_src = open_src.to_str().unwrap();
    let closed_src = closed_src.to_str().unwrap();
    assert_closed_stdout_keeps_the_verdict(
        &["compile", open_src, "--edition", "java"],
        &["compile", closed_src, "--edition", "java"],
    );
    let lock = closed_dir.path().join("village.crn.lock");
    assert!(lock.is_file(), "no lock at {}", lock.display());
}
