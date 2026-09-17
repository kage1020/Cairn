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

/// Every `.crn` under `examples/`, sorted, so a sweep covers a new example
/// the moment it lands. Refuses a set too small to be the shipped one: a
/// sweep over nothing passes.
pub fn crn_examples() -> Vec<PathBuf> {
    let dir = examples_dir();
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()));
    let mut found: Vec<PathBuf> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("cannot read an entry of {}: {err}", dir.display()))
                .path()
        })
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("crn"))
        .collect();
    found.sort();
    assert!(
        found.len() >= 5,
        "found only {} examples under {}, which is not the shipped set",
        found.len(),
        dir.display(),
    );
    found
}

/// A source plus the directory its artifacts would land in, both removed
/// when the test ends.
///
/// `prefix` names the binary that owns the directory, so two binaries
/// running the same `label` at once do not share one.
pub struct Fixture {
    /// Directory holding the source, the output, and the lock.
    dir: PathBuf,
}

impl Fixture {
    pub fn new(prefix: &str, label: &str, source: &str) -> Self {
        let mut dir = std::env::temp_dir();
        dir.push(format!("{prefix}-{}-{label}", std::process::id()));
        // A leftover from an interrupted run would let a test read a stale
        // artifact as a fresh one, so the removal has to have happened —
        // "it was not there" is the only other acceptable outcome.
        match fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!("cannot clear {}: {err}", dir.display()),
        }
        fs::create_dir_all(&dir).expect("create fixture dir");
        fs::write(dir.join("s.crn"), source).expect("write source");
        Self { dir }
    }

    pub fn source(&self) -> PathBuf {
        self.dir.join("s.crn")
    }

    pub fn lock(&self) -> PathBuf {
        self.dir.join("s.crn.lock")
    }

    pub fn out(&self) -> PathBuf {
        self.dir.join("out")
    }

    /// Every file the compile could have produced.
    ///
    /// Every I/O failure panics rather than reading as "nothing there":
    /// the absence of artifacts is what some of these tests observe, and a
    /// walk that quietly gives up folds them toward passing.
    pub fn artifacts(&self) -> Vec<String> {
        fn walk(dir: &Path, into: &mut Vec<String>) {
            let entries = fs::read_dir(dir)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()));
            for entry in entries {
                let path = entry
                    .unwrap_or_else(|err| {
                        panic!("cannot read an entry of {}: {err}", dir.display())
                    })
                    .path();
                if path.is_dir() {
                    walk(&path, into);
                } else if path.extension().and_then(|e| e.to_str()) != Some("crn") {
                    into.push(path.file_name().expect("named").to_string_lossy().into());
                }
            }
        }
        let mut found = Vec::new();
        walk(&self.dir, &mut found);
        found.sort();
        found
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// `cairn compile` of the fixture's source for `edition` / `target`, with
/// the output directory and the lockfile inside the fixture's directory.
pub fn compile_as(fixture: &Fixture, edition: &str, target: &str) -> Output {
    Command::new(cargo_bin())
        .arg("compile")
        .arg(fixture.source())
        .args(["--edition", edition, "--target", target])
        .arg("--out")
        .arg(fixture.out())
        .arg("--lock")
        .arg(fixture.lock())
        .output()
        .expect("failed to invoke cairn binary")
}
