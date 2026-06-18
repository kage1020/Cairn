//! End-to-end tests for `cairn check <file>`.

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

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("cairn-lang-core")
        .join("tests")
        .join("fixtures")
        .join("check")
}

fn run_check(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("check")
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn cli_1_clean_example_exits_zero_with_empty_stdout() {
    let path = examples_dir().join("cottage.crn");
    let out = run_check(&[path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        stdout.trim().is_empty(),
        "clean example should produce no stdout, got: {stdout}",
    );
}

#[test]
fn cli_2_broken_fixture_exits_one_with_position_anchored_output() {
    let path = fixtures_dir().join("duplicate.crn");
    let out = run_check(&[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        stdout.contains("E_DUPLICATE_SIZE"),
        "expected E_DUPLICATE_SIZE in output, got: {stdout}",
    );
    assert!(
        stdout.contains("E_DUPLICATE_ARG"),
        "expected E_DUPLICATE_ARG in output, got: {stdout}",
    );
    assert!(
        stdout.contains("E_DUPLICATE_ID"),
        "expected E_DUPLICATE_ID in output, got: {stdout}",
    );
    // gcc-style: `<file>:<line>:<col>: error[<CODE>]: <msg>` on every line.
    for line in stdout.lines().filter(|l| l.contains("E_")) {
        assert!(
            line.contains(':'),
            "diagnostic line should be gcc-style, got: {line}",
        );
        assert!(
            line.contains("error["),
            "every M2-PR2 diagnostic is `error[CODE]`, got: {line}",
        );
    }
}

#[test]
fn cli_3_missing_file_exits_with_code_two() {
    let out = run_check(&["does-not-exist.crn"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn cli_4_clean_fixture_json_output_is_empty_array() {
    let path = fixtures_dir().join("clean.crn");
    let out = run_check(&[path.to_str().unwrap(), "--format", "json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let arr = parsed.as_array().expect("expected a JSON array");
    assert!(arr.is_empty(), "expected empty array, got {parsed}");
}

#[test]
fn cli_5_parse_failure_exits_one_with_parse_style_message() {
    // A file the parser rejects must still hand the user a gcc-style
    // location, just like `cairn parse` does today.
    let bad_path = tempfile_with_contents("@unknown_directive nope\n");
    let out = run_check(&[bad_path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("unknown directive"),
        "expected parse error in stderr, got: {stderr}",
    );
}

#[test]
fn cli_unknown_keyword_fixture_lists_known_keywords_in_a_note() {
    let path = fixtures_dir().join("unknown_keyword.crn");
    let out = run_check(&[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(stdout.contains("E_UNKNOWN_KEYWORD"));
    assert!(
        stdout.contains("note:") && stdout.contains("floor"),
        "expected a known-keywords note, got: {stdout}",
    );
}

#[test]
fn cli_type_mismatch_fixture_reports_both_label_and_size_codes() {
    let path = fixtures_dir().join("type_mismatch.crn");
    let out = run_check(&[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(stdout.contains("E_TYPE_MISMATCH_LABEL"));
    assert!(stdout.contains("E_TYPE_MISMATCH_SIZE"));
}

/// Write a transient `.crn` file under the system temp dir, returning its
/// path. Used by [`cli_5_parse_failure_exits_one_with_parse_style_message`]
/// so the test does not depend on a checked-in fixture intentionally
/// broken at the parse layer.
fn tempfile_with_contents(contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let pid = std::process::id();
    path.push(format!("cairn-cli-check-{pid}.crn"));
    std::fs::write(&path, contents).expect("write tempfile");
    path
}
