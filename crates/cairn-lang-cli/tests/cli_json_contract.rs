//! `--format json` writes one JSON document to stdout, for every input.
//!
//! The flag advertises a machine-readable product. It delivered one for
//! every source except the ones that fail — where stdout was empty and the
//! reason went to stderr as prose — so a consumer parsing stdout saw
//! nothing and had to guess from the exit code. These tests hold the
//! contract on the paths that used to fall through it.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

mod common;
use common::{cairn, examples_dir};

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
    let out = cairn("check", &[path.to_str().unwrap(), "--format", "json"]);
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
    let out = cairn("check", &[path.to_str().unwrap()]);
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
    let out = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
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
    let out = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
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
    let out = cairn("info", &[path.to_str().unwrap(), "--format", "json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed.get("registry_compat").is_some(), "{stdout}");
    assert!(parsed.get("diagnostics").is_none(), "{stdout}");
}

#[test]
fn an_unreadable_source_is_still_told_apart_from_an_unparsable_one() {
    // A missing file is the caller's mistake and exits 2; a file that does
    // not parse is the source's and exits 1. Neither should have moved, and
    // every subcommand that reads a source draws the line in the same
    // place — each row carries the flags the subcommand insists on.
    let out_dir = TempDir::new().expect("out tempdir");
    let out_arg = out_dir.path().to_str().expect("utf-8 path");
    let subcommands: [(&str, &[&str]); 6] = [
        ("parse", &[]),
        ("check", &["--format", "json"]),
        ("info", &["--format", "json"]),
        ("lower", &[]),
        ("synth", &["--experimental-logic-synth"]),
        ("compile", &["--edition", "java", "--out", out_arg]),
    ];
    for (sub, flags) in subcommands {
        let mut args = vec!["definitely-not-a-file.crn"];
        args.extend_from_slice(flags);
        let missing = cairn(sub, &args);
        assert_eq!(
            missing.status.code(),
            Some(2),
            "{sub} should still exit 2 for a missing file, stderr={}",
            String::from_utf8_lossy(&missing.stderr),
        );
        let tmp = TempDir::new().expect("tempdir");
        let bad = unparsable(&tmp);
        let mut args = vec![bad.to_str().unwrap()];
        args.extend_from_slice(flags);
        let out = cairn(sub, &args);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{sub} should still exit 1 for an unparsable file, stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn parse_json_renders_a_parse_failure_as_a_document_of_its_own() {
    // `parse`'s product is the AST, so a failure is not that dump with a
    // hole in it — it is the document `info` writes, told apart by its
    // keys.
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let out = cairn("parse", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    assert!(
        parsed.get("items").is_none(),
        "a failure document is not an AST: {stdout}",
    );
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "E_PARSE");
    assert_eq!(diagnostics[0]["line"], 2);
}

#[test]
fn lower_json_renders_a_parse_failure_as_a_document_of_its_own() {
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let out = cairn("lower", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    assert!(
        parsed.get("structures").is_none(),
        "a failure document is not an IR: {stdout}",
    );
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "E_PARSE");
}

#[test]
fn lower_json_renders_a_later_pass_failure_the_same_way() {
    // Not only the parse path. `lower` refuses to dump an IR built from a
    // source `check` rejects, and that refusal was the other way stdout
    // came out empty.
    let tmp = TempDir::new().expect("tempdir");
    let path = unresolved(&tmp);
    let out = cairn("lower", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON, got {stdout:?}: {err}"));
    let diagnostics = parsed["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("expected a diagnostics array, got {stdout}"));
    assert!(
        diagnostics.iter().any(|d| d["code"] == "E_UNRESOLVED_SLOT"),
        "the finding that stopped the dump should be in it: {stdout}",
    );
}

#[test]
fn a_dump_in_any_other_format_still_reports_as_prose_on_stderr() {
    // The document belongs to `--format json` alone: `parse --format
    // debug` and `lower --format ascii` are read by a person, and a JSON
    // object on their stdout would be the regression this test guards
    // against.
    let tmp = TempDir::new().expect("tempdir");
    let path = unparsable(&tmp);
    let dumps: [(&str, &[&str]); 3] = [
        ("parse", &["--format", "debug"]),
        ("lower", &["--format", "ascii"]),
        ("lower", &["--format", "debug"]),
    ];
    for (sub, flags) in dumps {
        let mut args = vec![path.to_str().unwrap()];
        args.extend_from_slice(flags);
        let out = cairn(sub, &args);
        assert_eq!(out.status.code(), Some(1), "{sub} {flags:?}");
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        let stderr = String::from_utf8(out.stderr).expect("utf-8");
        assert!(stdout.is_empty(), "{sub} {flags:?} wrote {stdout}");
        assert!(
            stderr.contains("error[E_PARSE]:"),
            "{sub} {flags:?} should carry the code: {stderr}",
        );
    }
}

#[test]
fn a_dump_on_a_clean_source_is_still_the_dump() {
    // The failure document is additive here too: a source that has a dump
    // gets exactly the dump, with no `diagnostics` key grafted on.
    let path = examples_dir().join("cottage.crn");
    for sub in ["parse", "lower"] {
        let out = cairn(sub, &[path.to_str().unwrap(), "--format", "json"]);
        assert!(
            out.status.success(),
            "{sub}: {}",
            String::from_utf8_lossy(&out.stderr),
        );
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
        assert!(parsed.get("diagnostics").is_none(), "{sub}: {stdout}");
    }
}
