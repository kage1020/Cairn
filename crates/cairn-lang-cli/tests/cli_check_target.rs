//! `cairn check --edition E --target V`: the check gate with a version
//! pinned.
//!
//! Without a target the gate cannot ask whether a block id exists, because
//! the question has no answer at the edition level — `stone_bricks` is a
//! block on Bedrock 1.21.40 and `stonebrick` is the same block on Bedrock
//! 1.21.0. So `cairn check` skipped it, and a CI job gating on `check`
//! went green on a source `cairn compile` refuses with `E_UNKNOWN_ID`.
//! Pinning the pair the id belongs to closes that: the same lowering pass
//! `compile` runs, against the same table, writing nothing.

use std::path::PathBuf;
use std::process::Command;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// The issue's repro: a theme slot bound to a block no version has.
const UNKNOWN_ID: &str =
    "theme t:\n  slot floor -> @totally_not_a_block\nstruct s size=2x2\n  floor mat_slot=floor\n";

/// A slot bound to the id Bedrock only learned to spell in 1.21.40. The
/// interesting half of the check: not a typo, and correct on one target of
/// the very edition the other one refuses it on.
const RENAMED_ID: &str =
    "theme t:\n  slot floor -> @stone_bricks\nstruct s size=2x2\n  floor mat_slot=floor\n";

fn fixture(dir: &std::path::Path, source: &str) -> PathBuf {
    let path = dir.join("s.crn");
    std::fs::write(&path, source).expect("write fixture");
    path
}

fn run_check(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("check")
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn a_pinned_target_reports_an_id_that_target_does_not_declare() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
    ]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("E_UNKNOWN_ID"),
        "the pinned check must reach the lowering-stage finding, got: {stderr}",
    );
    // The registry the answer is about is named, so the reader knows which
    // of the two pins decided it.
    assert!(
        stderr.contains("java 1.21.4"),
        "the refusal names the pinned target, got: {stderr}",
    );
}

#[test]
fn without_a_target_the_same_source_still_passes() {
    // The premise of the flag being opt-in: no source that passes today
    // starts failing, because with no version pinned there is nothing to
    // check the id against and guessing one would refuse ids that are fine
    // on the target the author actually compiles for.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[src.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    // And an `--edition` pin alone is still not a version: it picks theme
    // variants and the table `@intended_targets` is weighed in, neither of
    // which is an id table.
    let out = run_check(&[src.to_str().unwrap(), "--edition", "java"]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
}

#[test]
fn a_renamed_id_is_judged_per_version_not_per_edition() {
    // One id, one edition, two answers. This is why the flag pins a
    // version rather than reusing `--edition`, and why option 2 of the
    // issue — check against every version the edition ships and report the
    // ids valid in none — would have said nothing here.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), RENAMED_ID);

    let refused = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "bedrock",
        "--target",
        "1.21.0",
    ]);
    let stderr = String::from_utf8(refused.stderr).expect("utf-8");
    assert_eq!(refused.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("E_UNKNOWN_ID"),
        "1.21.0 does not declare it, got: {stderr}",
    );
    // A rename is answered from the pack's alias table rather than by a
    // distance search, so the older spelling is offered by name.
    assert!(
        stderr.contains("stonebrick"),
        "the older spelling is the repair, got: {stderr}",
    );

    let accepted = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "bedrock",
        "--target",
        "1.21.40",
    ]);
    let stderr = String::from_utf8(accepted.stderr).expect("utf-8");
    assert_eq!(accepted.status.code(), Some(0), "stderr={stderr}");
}

#[test]
fn the_json_report_carries_the_unknown_id_payload() {
    // `DiagnosticData::UnknownId` (spec/lint §11.2) was documented and
    // unreachable from any CLI JSON output: `compile` prints text and
    // `check --format json` never produced the code. This is the run that
    // makes the documented shape observable.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
        "--format",
        "json",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON document");
    let first = parsed
        .as_array()
        .and_then(|a| a.first())
        .unwrap_or_else(|| panic!("a diagnostics array with the finding in it: {stdout}"));
    assert_eq!(first["code"], "E_UNKNOWN_ID", "got: {stdout}");
    assert_eq!(first["data"]["kind"], "unknown_id", "got: {stdout}");
    assert_eq!(
        first["data"]["id"], "minecraft:totally_not_a_block",
        "got: {stdout}",
    );
    assert_eq!(first["data"]["registry"], "java 1.21.4", "got: {stdout}");
    assert_eq!(first["data"]["origin"], "authored", "got: {stdout}");
}

#[test]
fn target_without_edition_is_refused_as_a_usage_error() {
    // Spec §4.2: `--target` alone is forbidden, because "1.21" names
    // different releases on Java and Bedrock. Exit 2 is the usage-error
    // code, not the "your file has a problem" one — the file was never
    // read.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[src.to_str().unwrap(), "--target", "1.21.4"]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(2), "stderr={stderr}");
    assert!(
        stderr.contains("--edition"),
        "the message names the flag that is missing, got: {stderr}",
    );
}

#[test]
fn a_target_the_edition_does_not_ship_refuses_the_run() {
    // A check that could not check the ids it was asked to must not exit
    // 0: the caller asked a question about a version, and "that version
    // does not exist" is not an answer that clears the file.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path(), UNKNOWN_ID);
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "9.9.9",
    ]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("unsupported java target `9.9.9`"),
        "the same refusal `cairn compile` gives, got: {stderr}",
    );
}

#[test]
fn a_bad_target_is_reported_after_the_findings_in_the_file() {
    // The ordering `run_compile` keeps, for the same reason: the lines the
    // author edits come first, and a command-line mistake printed above
    // them buries the syntax error that is also true.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(
        tmp.path(),
        "struct s size=2x2 size=3x3\n  floor mat_slot=floor\n",
    );
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "9.9.9",
    ]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    let finding = stderr
        .find("E_DUPLICATE_SIZE")
        .unwrap_or_else(|| panic!("premise: the file's own finding is reported: {stderr}"));
    let usage = stderr
        .find("unsupported java target")
        .unwrap_or_else(|| panic!("premise: the target is still refused: {stderr}"));
    assert!(finding < usage, "got: {stderr}");
}

#[test]
fn a_clean_example_passes_the_pinned_check_and_writes_nothing() {
    // `check --target` runs the lowering `compile` runs; what it must not
    // acquire is `compile`'s output. Run from a scratch directory so an
    // artifact or lockfile written next to the source would show up here.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = tmp.path().join("cottage.crn");
    std::fs::copy(examples_dir().join("cottage.crn"), &src).expect("copy example");
    let out = run_check(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
    ]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    assert!(stderr.trim().is_empty(), "nothing to report, got: {stderr}");

    let mut left: Vec<String> = std::fs::read_dir(tmp.path())
        .expect("read scratch dir")
        .map(|entry| {
            entry
                .expect("read an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    left.sort();
    assert_eq!(left, vec!["cottage.crn".to_owned()], "check writes nothing");
}
