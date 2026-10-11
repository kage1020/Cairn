//! `@intended_targets` against the floors the same file declares, as the
//! commands that report it see it: which commands weigh the header, in
//! which edition's table, and that a file whose two version headers
//! contradict each other exits 1. What each header earns is pinned in
//! `cairn-lang-core/tests/resolve/intended_targets.rs`.

use std::process::Command;

use crate::common::{Fixture, assert_stream, cargo_bin};

/// One structure with something to paint, so nothing here is reported
/// against an empty build.
const BUILD: &str = "\
theme t:
  slot wall -> @oak_planks

struct hut size=5x5
  walls mat_slot=wall height=3
";

/// `headers` above [`BUILD`], in a directory of its own.
fn fixture(headers: &str) -> Fixture {
    Fixture::new(&format!("{headers}{BUILD}"))
}

fn run(command: &str, fixture: &Fixture, args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(cargo_bin())
        .arg(command)
        .arg(fixture.source())
        .args(args)
        .output()
        .expect("failed to invoke cairn binary");
    (
        out.status.code(),
        String::from_utf8(out.stdout).expect("utf-8"),
        String::from_utf8(out.stderr).expect("utf-8"),
    )
}

#[test]
fn a_floor_above_every_intended_target_stops_check_exiting_zero() {
    let fixture = fixture("@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n");
    let (code, _, stderr) = run("check", &fixture, &[]);
    assert_eq!(code, Some(1), "got: {stderr}");
    assert_stream(&stderr, &[(2, 1, "error", "E_INTENDED_TARGET_CAP")]);
    assert!(
        stderr.contains("1.20.4") && stderr.contains("version>=1.21"),
        "both halves of the contradiction have to be named: {stderr}",
    );
}

#[test]
fn the_contradiction_needs_no_edition_pin() {
    let fixture = fixture("@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n");
    let (pinned, _, pinned_err) = run("check", &fixture, &["--edition", "java"]);
    let (bare, _, bare_err) = run("check", &fixture, &[]);
    assert_eq!(pinned, Some(1), "got: {pinned_err}");
    assert_eq!(bare, Some(1), "got: {bare_err}");
}

/// The header is a hint, and one stated too widely does not make the file
/// unbuildable.
#[test]
fn part_of_the_list_below_the_floor_is_a_warning_and_exits_zero() {
    let fixture = fixture("@requires version>=1.21\n@intended_targets [\"1.20.4\",\"1.21.4\"]\n");
    let (code, _, stderr) = run("check", &fixture, &[]);
    assert_eq!(code, Some(0), "got: {stderr}");
    assert_stream(&stderr, &[(2, 1, "warning", "W_INTENDED_TARGET_CAP")]);
}

#[test]
fn a_version_no_target_names_is_reported_only_with_one_edition_in_scope() {
    let fixture = fixture("@intended_targets [\"1.19\"]\n");
    let (bare_code, _, bare_err) = run("check", &fixture, &[]);
    assert_eq!(bare_code, Some(0), "got: {bare_err}");
    assert_stream(&bare_err, &[]);
    let (code, _, stderr) = run("check", &fixture, &["--edition", "java"]);
    assert_eq!(
        code,
        Some(0),
        "a hint about a version we cannot build is not an error: {stderr}"
    );
    assert_stream(
        &stderr,
        &[(1, 1, "warning", "W_INTENDED_TARGET_UNSUPPORTED")],
    );
    assert!(
        stderr.contains("java builds against 1.20.4, 1.21, 1.21.4"),
        "the closed set of candidates is part of the message: {stderr}",
    );
}

/// `1.21.4` is a Java release and names no Bedrock one.
#[test]
fn the_pinned_edition_decides_which_table_weighs_the_list() {
    let fixture = fixture("@intended_targets [\"1.21.4\"]\n");
    let (java, _, java_err) = run("check", &fixture, &["--edition", "java"]);
    assert_eq!(java, Some(0));
    assert_stream(&java_err, &[]);
    let (bedrock, _, bedrock_err) = run("check", &fixture, &["--edition", "bedrock"]);
    assert_eq!(bedrock, Some(0));
    assert!(
        bedrock_err.contains("names no bedrock release"),
        "and names no Bedrock one: {bedrock_err}",
    );
}

/// `--target 1.21` satisfies the floor; the contradiction is between two
/// lines of the file, which no target resolves.
#[test]
fn compile_refuses_the_source_check_refuses() {
    let fixture = fixture("@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n");
    let out = Command::new(cargo_bin())
        .arg("compile")
        .arg(fixture.source())
        .args(["--edition", "java", "--target", "1.21"])
        .arg("--out")
        .arg(fixture.out())
        .arg("--lock")
        .arg(fixture.lock())
        .output()
        .expect("failed to invoke cairn binary");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(1), "got: {stderr}");
    assert_stream(&stderr, &[(2, 1, "error", "E_INTENDED_TARGET_CAP")]);
    assert!(
        !fixture.lock().exists(),
        "a refused source must not leave a lock reading `verified: true`",
    );
}

#[test]
fn info_prints_the_declared_targets_beside_the_buildable_ones() {
    let fixture = fixture("@requires version>=1.20\n@intended_targets [\"1.20.4\",\"1.21.4\"]\n");
    let (code, stdout, stderr) = run("info", &fixture, &[]);
    assert_eq!(code, Some(0), "got: {stderr}");
    assert!(
        stdout.contains("intended targets:        1.20.4, 1.21.4"),
        "expected the declared row, got: {stdout}",
    );
    assert!(
        stdout.contains("buildable targets:"),
        "beside the computed one, got: {stdout}",
    );
}

#[test]
fn info_says_so_when_no_targets_are_declared() {
    let fixture = fixture("");
    let (code, stdout, stderr) = run("info", &fixture, &[]);
    assert_eq!(code, Some(0), "got: {stderr}");
    assert!(
        stdout.contains("intended targets:        (none declared)"),
        "got: {stdout}",
    );
}

#[test]
fn the_json_report_carries_the_declared_targets() {
    let fixture = fixture("@intended_targets [\"1.21.4\"]\n");
    let (code, stdout, stderr) = run("info", &fixture, &["--format", "json"]);
    assert_eq!(code, Some(0), "got: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(
        parsed["intended_targets"]
            .as_array()
            .expect("intended_targets is an array"),
        &vec![serde_json::Value::String("1.21.4".to_owned())],
    );
}

#[test]
fn the_finding_carries_a_structured_payload() {
    let fixture = fixture("@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n");
    let (code, stdout, _) = run("check", &fixture, &["--format", "json"]);
    assert_eq!(code, Some(1));
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let first = &parsed[0];
    assert_eq!(first["code"], "E_INTENDED_TARGET_CAP");
    assert_eq!(first["severity"], "error");
    assert_eq!(first["data"]["kind"], "intended_targets");
    assert_eq!(first["data"]["edition"], "java");
    assert_eq!(first["data"]["floor"], "version>=1.21");
    assert_eq!(
        first["data"]["targets"].as_array().expect("targets"),
        &vec![serde_json::Value::String("1.20.4".to_owned())],
    );
}

/// `1.21` is a target both editions build (Bedrock's `1.21.0`; trailing
/// zeros are not a different version), and the floor is above every
/// release of either, so both judge the header alike.
#[test]
fn a_header_both_editions_judge_alike_is_reported_once() {
    let fixture = fixture("@requires version>=99.0\n@intended_targets [\"1.21\"]\n");
    let (code, stdout, _) = run("check", &fixture, &["--format", "json"]);
    assert_eq!(code, Some(1));
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let findings = parsed.as_array().expect("an array of findings");
    assert_eq!(
        findings.len(),
        1,
        "1.21 is below the floor in both editions' tables: {stdout}",
    );
}

/// Java keeps a buildable version above its floor and earns the warning;
/// Bedrock has nothing left and earns the error.
#[test]
fn two_editions_disagreeing_about_reach_report_the_error() {
    let fixture = fixture(
        "@requires java version>=1.21.4\n@requires bedrock version>=1.21.60\n\
         @intended_targets [\"1.20.4\",\"1.21.4\",\"1.21.40\"]\n",
    );
    let (code, stdout, _) = run("check", &fixture, &["--format", "json"]);
    assert_eq!(code, Some(1));
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let findings = parsed.as_array().expect("an array of findings");
    assert_eq!(findings.len(), 1, "one line, one answer: {stdout}");
    assert_eq!(findings[0]["code"], "E_INTENDED_TARGET_CAP");
    assert_eq!(findings[0]["data"]["edition"], "bedrock");
}

#[test]
fn info_scoped_to_one_edition_is_not_refused_by_the_other() {
    let fixture = fixture("@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n");
    let (bedrock, stdout, stderr) = run("info", &fixture, &["--editions", "bedrock"]);
    assert_eq!(bedrock, Some(0), "got: {stderr}");
    assert!(
        stdout.contains("buildable targets:"),
        "the report has to be produced: {stdout}",
    );
    // One edition in scope, so the version Bedrock cannot build is named.
    assert_stream(
        &stderr,
        &[(2, 1, "warning", "W_INTENDED_TARGET_UNSUPPORTED")],
    );
    // The same file, with Java in scope, is the contradiction.
    let (java, _, java_err) = run("info", &fixture, &["--editions", "java"]);
    assert_eq!(java, Some(1), "got: {java_err}");
    assert_stream(&java_err, &[(2, 1, "error", "E_INTENDED_TARGET_CAP")]);
}

#[test]
fn info_across_both_editions_does_not_name_an_unbuildable_version() {
    let fixture = fixture("@intended_targets [\"1.20.4\"]\n");
    let (code, _, stderr) = run("info", &fixture, &[]);
    assert_eq!(code, Some(0), "got: {stderr}");
    assert_stream(&stderr, &[]);
}

#[test]
fn info_naming_one_edition_twice_still_names_an_unbuildable_version() {
    let fixture = fixture("@intended_targets [\"1.20.4\"]\n");
    let (code, stdout, stderr) = run("info", &fixture, &["--editions", "bedrock,bedrock"]);
    assert_stream(
        &stderr,
        &[(1, 1, "warning", "W_INTENDED_TARGET_UNSUPPORTED")],
    );
    assert_eq!(
        (code, stdout, stderr),
        run("info", &fixture, &["--editions", "bedrock"]),
        "--editions bedrock,bedrock should report what --editions bedrock does",
    );
}
