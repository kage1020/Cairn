//! `cairn check <file>` end to end: findings go to stderr as one located
//! `FILE:LINE:COL: SEVERITY[CODE]` line each, or to stdout as a
//! `--format json` array, and the exit code follows the worst severity.

use std::path::PathBuf;

use crate::common::{Fixture, assert_stream, cairn, examples_dir, reported, stderr_stream};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("cairn-lang-core")
        .join("tests")
        .join("fixtures")
        .join("check")
}

/// On stderr, as every subcommand reports, so `cairn check f.crn > out`
/// cannot swallow a finding; `--format json`, the machine payload, stays
/// on stdout.
#[test]
fn text_findings_go_to_stderr_one_located_line_each() {
    let path = fixtures_dir().join("duplicate.crn");
    let out = cairn("check", &[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stdout.is_empty(),
        "text diagnostics must not reach stdout, got: {stdout}",
    );
    assert_eq!(
        stderr_stream(&stderr),
        reported(&[
            (1, 19, "error", "E_DUPLICATE_SIZE"),
            (2, 3, "error", "E_MISSING_MATERIAL"),
            (2, 18, "error", "E_DUPLICATE_ARG"),
            (4, 8, "error", "E_DUPLICATE_ID"),
        ]),
        "{stderr}",
    );
}

#[test]
fn a_header_that_names_no_version_is_reported_without_refusing_the_file() {
    // `@cairn` is provenance that reaches no pass, so an unreadable value
    // costs the build nothing.
    let path = fixtures_dir().join("cairn_version.crn");
    let out = cairn("check", &[path.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    assert_stream(&stderr, &[(1, 1, "warning", "W_INVALID_CAIRN_VERSION")]);
}

/// A value holding a character no token starts with must reach the check
/// pass rather than be refused by the lexer as an error.
#[test]
fn an_unlexable_cairn_value_is_a_warning_and_the_build_goes_on() {
    let fixture = Fixture::new("@cairn 2026.6+build\n\ntheme t:\n  slot wall -> @stone\n");
    let file = fixture.source();
    let out = cairn("check", &[file.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    assert_stream(&stderr, &[(1, 1, "warning", "W_INVALID_CAIRN_VERSION")]);
}

/// `~=` is an operator Cairn does not define, which is `E_INVALID_REQUIRES`
/// whether or not the lexer has a token for `~`.
#[test]
fn an_unlexable_requires_value_is_an_invalid_floor_rather_than_a_parse_error() {
    for (label, source, at) in [
        (
            "requires-unlexable",
            "@requires version~=1.21\n\ntheme t:\n  slot wall -> @stone\n",
            (1, 1),
        ),
        (
            "member-requires-unlexable",
            "theme t:\n  requires version~=1.21\n  slot wall -> @stone\n",
            (2, 3),
        ),
    ] {
        let fixture = Fixture::new(source);
        let file = fixture.source();
        let out = cairn("check", &[file.to_str().unwrap()]);
        let stderr = String::from_utf8(out.stderr).expect("utf-8");
        assert_eq!(out.status.code(), Some(1), "{label}: stderr={stderr}");
        assert_stream(&stderr, &[(at.0, at.1, "error", "E_INVALID_REQUIRES")]);
    }
}

#[test]
fn clean_example_exits_zero_and_says_nothing_on_either_stream() {
    let path = examples_dir().join("cottage.crn");
    let out = cairn("check", &[path.to_str().unwrap()]);
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
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.trim().is_empty(),
        "clean example should produce no stderr, got: {stderr}",
    );
}

#[test]
fn clean_fixture_json_output_is_empty_array() {
    let path = fixtures_dir().join("clean.crn");
    let out = cairn("check", &[path.to_str().unwrap(), "--format", "json"]);
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
fn parse_failure_exits_one_with_parse_style_message() {
    let fixture = Fixture::new("@unknown_directive nope\n");
    let bad = fixture.source();
    let out = cairn("check", &[bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("unknown directive"),
        "expected parse error in stderr, got: {stderr}",
    );
}

/// The CLI is handed the file an editor wrote, terminator and all: the
/// whole of `def foo bar\n` is on line 1, so a `:2:1` prefix would point at
/// a line holding none of it.
#[test]
fn end_of_line_parse_error_names_the_line_that_has_it() {
    for (label, source) in [
        ("lf", "def foo bar\n"),
        ("crlf", "def foo bar\r\n"),
        ("cr", "def foo bar\r"),
        ("none", "def foo bar"),
    ] {
        let fixture = Fixture::new(source);
        let file = fixture.source();
        let out = cairn("check", &[file.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(1), "{label}");
        let stderr = String::from_utf8(out.stderr).expect("utf-8");
        assert!(
            stderr.contains(":1:12: "),
            "{label}: expected the error at 1:12, got: {stderr}",
        );
    }
}

#[test]
fn unknown_keyword_fixture_lists_known_keywords_in_a_note() {
    let path = fixtures_dir().join("unknown_keyword.crn");
    let out = cairn("check", &[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let reported = String::from_utf8(out.stderr).expect("utf-8");
    assert_stream(&reported, &[(2, 3, "error", "E_UNKNOWN_KEYWORD")]);
    assert!(
        reported.contains("note:") && reported.contains("floor"),
        "expected a known-keywords note, got: {reported}",
    );
    // The informational note carries no span of its own, so it must not
    // re-print the primary diagnostic's `file:L:C:` prefix.
    let primary_prefix = reported
        .lines()
        .find(|l| l.contains("E_UNKNOWN_KEYWORD"))
        .expect("expected a primary line")
        .split_once(':')
        .map(|(_, rest)| {
            let (line_col, _) = rest.split_once(": error").unwrap_or((rest, ""));
            line_col
        })
        .expect("expected gcc-style prefix");
    assert!(
        !reported.contains(&format!(":{primary_prefix}:   note:")),
        "informational note must not duplicate the primary's file:L:C prefix, got: {reported}",
    );
}

#[test]
fn json_output_carries_line_and_col_for_every_diagnostic() {
    let path = fixtures_dir().join("duplicate.crn");
    let out = cairn("check", &[path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let arr = parsed.as_array().expect("expected an array");
    assert!(!arr.is_empty());
    for entry in arr {
        for field in [
            "code", "severity", "line", "col", "end_line", "end_col", "primary",
        ] {
            assert!(
                entry.get(field).is_some(),
                "JSON diagnostic missing `{field}`: {entry}",
            );
        }
        let line = entry["line"].as_u64().expect("line should be a number");
        let col = entry["col"].as_u64().expect("col should be a number");
        assert!(line >= 1 && col >= 1, "1-based positions, got {line}:{col}");
        assert!(
            entry["code"].as_str().is_some_and(|s| s.starts_with("E_")),
            "code should be the E_-prefixed string, got: {}",
            entry["code"],
        );
        // A code without a payload omits `data`, keeping the contract
        // additive. No `E_DUPLICATE_*` code carries one, so a `data` key
        // here means `Diagnostic::data`'s `skip_serializing_if` broke, or
        // one of them gained a payload without this fixture following.
        assert!(
            entry.get("data").is_none(),
            "diagnostic without a payload must omit the `data` key, got: {entry}",
        );
    }
}
