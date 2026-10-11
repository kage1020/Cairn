//! End-to-end tests for `cairn check <file>`.

use std::path::PathBuf;

use crate::common::{Fixture, cairn, examples_dir, reported, stderr_stream};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("cairn-lang-core")
        .join("tests")
        .join("fixtures")
        .join("check")
}

/// `check` reports on stderr, as `lower`, `info`, `compile` and `synth` do,
/// so `cairn check f.crn > out` cannot swallow a finding: one gcc-style
/// `FILE:LINE:COL: SEVERITY[CODE]: ...` line each, in source order, and
/// nothing on stdout. Only the text format goes there; `--format json` is
/// the machine payload and belongs on stdout, where a consumer redirects
/// it deliberately.
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
    // `@cairn` is provenance: the value reaches no pass, so a value no
    // compiler can read costs the build nothing and the finding is a
    // warning. The exit code is the part worth pinning end to end —
    // promoting the code would turn every file carrying a typo'd header
    // into a refused build, and the severity table alone does not say
    // what `cairn check` does with it.
    let path = fixtures_dir().join("cairn_version.crn");
    let out = cairn("check", &[path.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    assert!(
        stderr.contains("W_INVALID_CAIRN_VERSION"),
        "the warning still reaches the author: {stderr}",
    );
}

/// A `@cairn` value holding a character no token starts with is still a
/// header that names no version, and so still a warning: the build goes
/// on. Pinned end to end because the difference is the exit code — the
/// file used to be refused by the lexer before the check pass saw it.
#[test]
fn an_unlexable_cairn_value_is_a_warning_and_the_build_goes_on() {
    let fixture = Fixture::new("@cairn 2026.6+build\n\ntheme t:\n  slot wall -> @stone\n");
    let file = fixture.source();
    let out = cairn("check", &[file.to_str().unwrap()]);
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert_eq!(out.status.code(), Some(0), "stderr={stderr}");
    assert!(stderr.contains("W_INVALID_CAIRN_VERSION"), "{stderr}");
    assert!(!stderr.contains("E_PARSE"), "{stderr}");
}

/// The same for a floor: `~=` is an operator Cairn does not define, which
/// is `E_INVALID_REQUIRES` whether or not the lexer has a token for `~`.
#[test]
fn an_unlexable_requires_value_is_an_invalid_floor_rather_than_a_parse_error() {
    for (label, source) in [
        (
            "requires-unlexable",
            "@requires version~=1.21\n\ntheme t:\n  slot wall -> @stone\n",
        ),
        (
            "member-requires-unlexable",
            "theme t:\n  requires version~=1.21\n  slot wall -> @stone\n",
        ),
    ] {
        let fixture = Fixture::new(source);
        let file = fixture.source();
        let out = cairn("check", &[file.to_str().unwrap()]);
        let stderr = String::from_utf8(out.stderr).expect("utf-8");
        assert_eq!(out.status.code(), Some(1), "{label}: stderr={stderr}");
        assert!(stderr.contains("E_INVALID_REQUIRES"), "{label}: {stderr}");
        assert!(!stderr.contains("E_PARSE"), "{label}: {stderr}");
    }
}

#[test]
fn cli_1_clean_example_exits_zero_and_says_nothing_on_either_stream() {
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
    // The load-bearing half now that diagnostics live on stderr: a clean
    // input has nothing to report anywhere.
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.trim().is_empty(),
        "clean example should produce no stderr, got: {stderr}",
    );
}

#[test]
fn cli_4_clean_fixture_json_output_is_empty_array() {
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
fn cli_5_parse_failure_exits_one_with_parse_style_message() {
    // A file the parser rejects must still hand the user a gcc-style
    // location, just like `cairn parse` does today.
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

/// A parse error at the end of a line names that line, in the output the
/// user actually reads.
///
/// The layer below is covered by `cairn-lang-core`'s `line_positions`
/// tests; this one exists because the file the CLI is handed is the one an
/// editor wrote, terminator and all. The whole of `def foo bar\n` is on
/// line 1, and the gcc-style prefix used to read `:2:1` — sending a reader,
/// or a model repairing its own output, to a line holding none of it.
#[test]
fn cli_end_of_line_parse_error_names_the_line_that_has_it() {
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
fn cli_unknown_keyword_fixture_lists_known_keywords_in_a_note() {
    let path = fixtures_dir().join("unknown_keyword.crn");
    let out = cairn("check", &[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let reported = String::from_utf8(out.stderr).expect("utf-8");
    assert!(reported.contains("E_UNKNOWN_KEYWORD"));
    assert!(
        reported.contains("note:") && reported.contains("floor"),
        "expected a known-keywords note, got: {reported}",
    );
    // The informational note must not re-print the same `file:L:C:`
    // prefix as the primary diagnostic — that was the bug fixed when
    // `DiagnosticNote.span` became `Option<Span>`.
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
fn cli_json_output_carries_line_and_col_for_every_diagnostic() {
    // The `--format json` contract is that downstream tooling (LSP, CI
    // gates) receives the same position information as the text format.
    // Without `line` / `col` the JSON form would be useless to anything
    // that wants to underline the offending range.
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
        // Codes without a structured payload must
        // omit the `data` key entirely so the JSON contract stays
        // additive for consumers that pin a fixed key set. The
        // `duplicate.crn` fixture only triggers `E_DUPLICATE_*` codes,
        // none of which carry a payload today, so any `data` here would
        // mean one of two regressions:
        //   (a) the `serde(skip_serializing_if = "Option::is_none")`
        //       wiring on `Diagnostic::data` was dropped or inverted, or
        //   (b) a future change attached a payload to one of the
        //       `E_DUPLICATE_*` codes without updating this fixture and
        //       its assertion in step.
        // Either case should fail loud here rather than ship a
        // silently-changed JSON contract.
        assert!(
            entry.get("data").is_none(),
            "diagnostic without a payload must omit the `data` key, got: {entry}",
        );
    }
}
