//! AC A1–A8: `cairn compile` reads the lockfile it is about to replace.
//!
//! `spec/versioning-editions.md` §10.6 says a recompile for a different
//! target "shows the difference from the verified one as a loud warning",
//! and prints the two lines this file pins. Until now `read_from_path` had
//! no production caller at all: the previous lockfile was overwritten
//! without being looked at, so neither warning could exist and a stale or
//! tampered file was discarded in silence.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// A copy of `cottage.crn` in a writable directory of its own, so the
/// default lockfile path is exercisable and nothing lands in the repo.
fn cottage_in_tempdir() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join("cottage.crn");
    fs::copy(examples_dir().join("cottage.crn"), &dst).expect("copy cottage");
    (tmp, dst)
}

struct Run {
    code: Option<i32>,
    stderr: String,
}

fn compile(src: &Path, out_dir: &Path, edition: &str, target: &str, lock: Option<&Path>) -> Run {
    let mut args: Vec<String> = vec![
        "compile".to_owned(),
        src.to_string_lossy().into_owned(),
        "--edition".to_owned(),
        edition.to_owned(),
        "--target".to_owned(),
        target.to_owned(),
        "--out".to_owned(),
        out_dir.to_string_lossy().into_owned(),
    ];
    if let Some(lock) = lock {
        args.push("--lock".to_owned());
        args.push(lock.to_string_lossy().into_owned());
    }
    let out = Command::new(cargo_bin())
        .args(&args)
        .output()
        .expect("run cairn compile");
    Run {
        code: out.status.code(),
        stderr: String::from_utf8(out.stderr).expect("stderr utf-8"),
    }
}

fn warning_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("W_") || line.starts_with("warning:"))
        .collect()
}

#[test]
fn a_recompile_for_another_version_prints_the_shape_the_spec_prints() {
    // The exact line spec §10.6 shows. The second half deliberately drops
    // the `DataVersion` word — the spec prints `now 1.21.4/3955.` — so this
    // pins the punctuation as well as the numbers.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    let first = compile(&src, out_dir.path(), "java", "1.20.4", Some(&lock));
    assert_eq!(first.code, Some(0), "stderr={}", first.stderr);
    assert!(
        warning_lines(&first.stderr).is_empty(),
        "the first compile has nothing to compare against: {:?}",
        warning_lines(&first.stderr),
    );

    let second = compile(&src, out_dir.path(), "java", "1.21.4", Some(&lock));
    assert_eq!(second.code, Some(0), "stderr={}", second.stderr);
    assert!(
        second.stderr.contains(
            "W_PREVIOUSLY_VERIFIED_TARGET: verified for 1.20.4/DataVersion 3700, now 1.21.4/4189."
        ),
        "stderr does not carry the spec's line: {}",
        second.stderr,
    );
}

#[test]
fn a_recompile_for_the_same_target_says_nothing() {
    // The warning is about divergence. Firing it on every rebuild would
    // train the reader to skip it, which is the same as not having it.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, out_dir.path(), "java", "1.21.4", Some(&lock));
    let again = compile(&src, out_dir.path(), "java", "1.21.4", Some(&lock));
    assert_eq!(again.code, Some(0), "stderr={}", again.stderr);
    assert!(
        warning_lines(&again.stderr).is_empty(),
        "an unchanged target should warn about nothing: {:?}",
        warning_lines(&again.stderr),
    );
}

#[test]
fn a_change_of_edition_names_both_editions() {
    // Two editions number their releases differently, so the version pair
    // alone reads as noise — `1.21.4` and `1.21.60` are not comparable
    // numbers. The edition is what makes the line mean something.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, out_dir.path(), "java", "1.21.4", Some(&lock));
    let switched = compile(&src, out_dir.path(), "bedrock", "1.21.60", Some(&lock));
    assert_eq!(switched.code, Some(0), "stderr={}", switched.stderr);
    let line = switched
        .stderr
        .lines()
        .find(|l| l.starts_with("W_PREVIOUSLY_VERIFIED_TARGET:"))
        .unwrap_or_else(|| panic!("no warning in: {}", switched.stderr));
    assert!(
        line.contains("java 1.21.4") && line.contains("bedrock 1.21.60"),
        "the line should name both editions: {line}",
    );
}

#[test]
fn the_verified_side_names_the_field_the_recorded_edition_actually_has() {
    // The left half of the line names the version integer; Bedrock's is the
    // block palette's own `version`, not Minecraft's `DataVersion`, and
    // calling it `DataVersion` would name the Java concept for a number
    // that is not one. Only a Bedrock lockfile on the *left* shows this —
    // the edition-change test above has Bedrock on the right, where the
    // integer is printed bare.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, out_dir.path(), "bedrock", "1.21.60", Some(&lock));
    let switched = compile(&src, out_dir.path(), "java", "1.21.4", Some(&lock));
    let line = switched
        .stderr
        .lines()
        .find(|l| l.starts_with("W_PREVIOUSLY_VERIFIED_TARGET:"))
        .unwrap_or_else(|| panic!("no warning in: {}", switched.stderr));
    assert!(
        line.contains("verified for bedrock 1.21.60/block version 18168865,"),
        "the verified side should name Bedrock's own version field: {line}",
    );
    assert!(
        line.ends_with("now java 1.21.4/4189."),
        "the new side prints the integer bare: {line}",
    );
}

#[test]
fn the_sensitivity_list_reported_is_the_one_the_lockfile_recorded() {
    // `member_version_sensitivity` is empty until the constraint-catalog
    // ingest lands, so the compiler cannot produce a populated one — but a
    // lockfile that carries entries is exactly the case the second warning
    // exists for, and what it reports has to be what was recorded rather
    // than anything synthesised here.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, out_dir.path(), "java", "1.20.4", Some(&lock));
    let recorded = fs::read_to_string(&lock).expect("read lock");
    let doctored = recorded.replace(
        "member_version_sensitivity: []\n",
        "member_version_sensitivity:\n\
         - id: yard_water\n  \
           reason: cauldron split at 1.17\n\
         - id: fence\n  \
           reason: connects differently since 1.16\n",
    );
    assert_ne!(doctored, recorded, "the fixture did not add any entries");
    fs::write(&lock, &doctored).expect("write lock");

    let second = compile(&src, out_dir.path(), "java", "1.21.4", Some(&lock));
    assert!(
        second.stderr.contains(
            "W_SEMANTIC_SENSITIVITY: 2 members may resolve differently: yard_water, fence"
        ),
        "stderr does not carry the recorded members: {}",
        second.stderr,
    );
}

#[test]
fn nothing_is_said_about_sensitivity_when_the_lockfile_recorded_none() {
    // The paired half of the test above: the second warning is not a
    // fixed companion of the first.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, out_dir.path(), "java", "1.20.4", Some(&lock));
    let second = compile(&src, out_dir.path(), "java", "1.21.4", Some(&lock));
    assert!(
        second.stderr.contains("W_PREVIOUSLY_VERIFIED_TARGET:"),
        "the target warning should still fire: {}",
        second.stderr,
    );
    assert!(
        !second.stderr.contains("W_SEMANTIC_SENSITIVITY"),
        "no members were recorded, so there is nothing to report: {}",
        second.stderr,
    );
}

#[test]
fn a_lockfile_that_does_not_parse_is_reported_rather_than_discarded() {
    // "A stale or tampered lockfile is silently discarded rather than
    // detected" is the defect. It stays a warning: the compile itself is
    // valid, and an unrelated corrupt file next to the source is no reason
    // to refuse to build.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");
    fs::write(&lock, "this: [is not, a lockfile\n").expect("write junk");

    let run = compile(&src, out_dir.path(), "java", "1.21.4", Some(&lock));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert!(
        run.stderr.contains("existing lockfile"),
        "stderr should say the old lockfile could not be read: {}",
        run.stderr,
    );
    let replaced = fs::read_to_string(&lock).expect("read lock");
    assert!(
        replaced.starts_with("lock_schema_version:"),
        "the lockfile should still have been replaced: {replaced}",
    );
}

#[test]
fn a_lockfile_from_a_newer_schema_is_reported_rather_than_read() {
    // The schema version exists so a later format is not read as if the
    // field names still meant the same thing. Reaching the compile path
    // proves the check is not confined to the library.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out");
    let lock = out_dir.path().join("cottage.lock");

    compile(&src, out_dir.path(), "java", "1.20.4", Some(&lock));
    let recorded = fs::read_to_string(&lock).expect("read lock");
    let doctored = recorded.replace("lock_schema_version: 1\n", "lock_schema_version: 99\n");
    assert_ne!(doctored, recorded, "the fixture did not change the version");
    fs::write(&lock, doctored).expect("write lock");

    let run = compile(&src, out_dir.path(), "java", "1.21.4", Some(&lock));
    assert_eq!(run.code, Some(0), "stderr={}", run.stderr);
    assert!(
        run.stderr.contains("existing lockfile") && run.stderr.contains("99"),
        "stderr should name the schema it refused: {}",
        run.stderr,
    );
    assert!(
        !run.stderr.contains("W_PREVIOUSLY_VERIFIED_TARGET"),
        "a document that was not read cannot be compared: {}",
        run.stderr,
    );
}

#[test]
fn the_default_lock_path_is_read_back_too() {
    // `--lock` is optional; compile writes `<source>.lock` beside the
    // source without it. A read-back that only ran under the explicit flag
    // would miss the path almost every build uses.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out");

    let first = compile(&src, out_dir.path(), "java", "1.20.4", None);
    assert_eq!(first.code, Some(0), "stderr={}", first.stderr);
    let default_lock = src.with_extension("crn.lock");
    assert!(
        default_lock.exists(),
        "expected the default lockfile at {}",
        default_lock.display(),
    );

    let second = compile(&src, out_dir.path(), "java", "1.21.4", None);
    assert!(
        second.stderr.contains("W_PREVIOUSLY_VERIFIED_TARGET:"),
        "the default path was not read back: {}",
        second.stderr,
    );
}
