//! Helpers shared by the `cairn` binary's integration tests.
//!
//! Every binary here spawns the same executable against the same
//! `examples/` tree; the plumbing for that lives once, so a change to
//! where the binary or the examples are found lands in one place.

// Each test binary uses its own subset of these.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// The `cairn` binary cargo built for this test run.
pub fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

/// The repository's `examples/` directory.
pub fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// Run the binary with `argv` and wait for it to exit.
pub fn cairn_argv(argv: &[&str]) -> Output {
    Command::new(cargo_bin())
        .args(argv)
        .output()
        .expect("failed to invoke cairn binary")
}

/// Run one subcommand of the binary and wait for it to exit.
pub fn cairn(subcommand: &str, args: &[&str]) -> Output {
    Command::new(cargo_bin())
        .arg(subcommand)
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

/// Copy `examples/<name>` into a fresh temp dir so a test can rely on the
/// source's parent being writable without leaving lock artefacts in the
/// repository.
pub fn example_in_tempdir(name: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join(name);
    fs::copy(examples_dir().join(name), &dst).expect("copy example");
    (tmp, dst)
}

/// Write `source` to `dir/name` and return the path.
pub fn write_source(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, source).expect("write source");
    path
}
