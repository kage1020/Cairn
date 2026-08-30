//! `--format json` writes one JSON document to stdout, for every input.
//!
//! The flag advertises a machine-readable product. It delivered one for
//! every source except the ones that fail — where stdout was empty and the
//! reason went to stderr as prose — so a consumer parsing stdout saw
//! nothing and had to guess from the exit code. These tests hold the
//! contract on the paths that used to fall through it.

use std::fs;
use std::path::PathBuf;
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

fn run(sub: &str, args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .arg(sub)
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

/// A source no lexer pass will take: `%` is in no token.
fn unparsable(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("bad.crn");
    fs::write(&path, "struct s size=3x3\n  floor a=%\n").expect("write");
    path
}

/// A source that parses and then fails the check pass.
fn unresolved(dir: &TempDir) -> PathBuf {
    let path = dir.path().join("unresolved.crn");
    fs::write(
        &path,
        "theme t:\n  slot floor -> @oak_planks\n\nstruct s size=3x3\n  floor mat_slot=missing\n",
    )
    .expect("write");
    path
}

#[test]
fn check_json_renders_a_parse_failure_as_a_diagnostic() {
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let out = run("check", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    let array = parsed.as_array().expect("check emits an array");
    assert_eq!(array.len(), 1, "one failure, one diagnostic: {stdout}");
    let d = &array[0];
    assert_eq!(d["code"], "E_PARSE");
    assert_eq!(d["severity"], "error");
    assert_eq!(d["line"], 2, "the failure is on the second line: {stdout}");
    assert!(
        d["primary"].as_str().is_some_and(|s| !s.is_empty()),
        "the diagnostic should say what is wrong: {stdout}",
    );
}

#[test]
fn check_text_renders_a_parse_failure_like_every_other_finding() {
    // The text form used to be a bare `error: file:pos: message` with no
    // code — the one finding a reader could not look up, and the one a
    // grep for `error[E_` did not match.
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let out = run("check", &[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        stdout.is_empty(),
        "text diagnostics stay off stdout: {stdout}"
    );
    assert!(
        stderr.contains("error[E_PARSE]:"),
        "the failure should carry its code: {stderr}",
    );
    assert!(
        stderr.contains(&format!("{}:2:", path.display())),
        "and its position: {stderr}",
    );
}

#[test]
fn info_json_renders_a_parse_failure_as_a_document_of_its_own() {
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let out = run("info", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    // Distinguishable from the report by its keys: a report has axes and no
    // `diagnostics`, this has `diagnostics` and no axes.
    assert!(
        parsed.get("registry_compat").is_none(),
        "a failure document is not a report: {stdout}",
    );
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "E_PARSE");
}

#[test]
fn info_json_renders_a_check_failure_the_same_way() {
    // Not only the parse path: `info` wrote nothing to stdout for *any*
    // error-severity finding, so fixing the parse case alone would leave
    // the flag honest for one kind of failure and silent for the other.
    let tmp = TempDir::new().expect("tempdir");
    let path = unresolved(&tmp);
    let out = run("info", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert!(
        diagnostics.iter().any(|d| d["code"] == "E_UNRESOLVED_SLOT"),
        "the finding that stopped the report should be in it: {stdout}",
    );
}

#[test]
fn info_json_on_a_clean_source_is_still_the_report() {
    // The failure document is additive: a source that has a report still
    // gets exactly the report, with no `diagnostics` key grafted on.
    let path = examples_dir().join("cottage.crn");
    let out = run("info", &[path.to_str().unwrap(), "--format", "json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed.get("registry_compat").is_some(), "{stdout}");
    assert!(parsed.get("diagnostics").is_none(), "{stdout}");
}

#[test]
fn an_unreadable_source_is_still_told_apart_from_an_unparsable_one() {
    // A missing file is the caller's mistake and exits 2; a file that does
    // not parse is the source's and exits 1. Neither should have moved.
    for sub in ["check", "info"] {
        let missing = run(sub, &["definitely-not-a-file.crn", "--format", "json"]);
        assert_eq!(
            missing.status.code(),
            Some(2),
            "{sub} should still exit 2 for a missing file",
        );
        let tmp = TempDir::new().expect("tempdir");
        let bad = unparsable(&tmp);
        let out = run(sub, &[bad.to_str().unwrap(), "--format", "json"]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{sub} should still exit 1 for an unparsable file",
        );
    }
}
