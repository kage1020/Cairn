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
fn text_diagnostics_go_where_every_other_subcommand_sends_them() {
    // `lower`, `info`, `compile` and `synth` all report on stderr, so
    // `cairn check f.crn > out` was the one build command whose findings a
    // redirect could swallow: stdout carried them, and a pipeline capturing
    // stdout for something else saw nothing at all.
    //
    // Only the text format moves. `--format json` is the machine payload
    // and belongs on stdout, where a consumer redirects it deliberately —
    // pinned separately below.
    let path = fixtures_dir().join("duplicate.crn");
    let out = run_check(&[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stdout.is_empty(),
        "text diagnostics must not reach stdout, got: {stdout}",
    );
    assert!(
        stderr.contains("E_DUPLICATE_SIZE"),
        "stderr should carry the diagnostics, got: {stderr}",
    );
}

#[test]
fn cli_1_clean_example_exits_zero_and_says_nothing_on_either_stream() {
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
    let reported = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        reported.contains("E_DUPLICATE_SIZE"),
        "expected E_DUPLICATE_SIZE in output, got: {reported}",
    );
    assert!(
        reported.contains("E_DUPLICATE_ARG"),
        "expected E_DUPLICATE_ARG in output, got: {reported}",
    );
    assert!(
        reported.contains("E_DUPLICATE_ID"),
        "expected E_DUPLICATE_ID in output, got: {reported}",
    );
    // gcc-style: `<file>:<line>:<col>: error[<CODE>]: <msg>` on every line.
    for line in reported.lines().filter(|l| l.contains("E_")) {
        assert!(
            line.contains(':'),
            "diagnostic line should be gcc-style, got: {line}",
        );
        assert!(
            line.contains("error["),
            "every error-severity diagnostic renders as `error[CODE]`, got: {line}",
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
    let bad = tempfile_with_contents("directive", "@unknown_directive nope\n");
    let out = run_check(&[bad.arg()]);
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
        let file = tempfile_with_contents(label, source);
        let out = run_check(&[file.arg()]);
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
    let out = run_check(&[path.to_str().unwrap()]);
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
    let out = run_check(&[path.to_str().unwrap(), "--format", "json"]);
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
        // Forward-compatibility guard: when a future code lands with a
        // documented payload, the fixture-and-fixture-only matcher above
        // is not enough — the JSON has to carry the matching `data.kind`.
        // `W_WALKWAY_BLOCKED` is the first such code, and currently
        // `cairn check` does not drive walkway lowering so this branch
        // never fires on the `duplicate.crn` fixture. If a future wiring
        // pipes walkway diagnostics through `check`, this assertion
        // catches a payload-stripped regression at the CLI surface.
        if entry["code"].as_str() == Some("W_WALKWAY_BLOCKED") {
            assert_eq!(
                entry["data"]["kind"].as_str(),
                Some("walkway_blocked"),
                "W_WALKWAY_BLOCKED must carry data.kind=\"walkway_blocked\", got: {entry}",
            );
        }
    }
}

#[test]
fn cli_type_mismatch_fixture_reports_both_label_and_size_codes() {
    let path = fixtures_dir().join("type_mismatch.crn");
    let out = run_check(&[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let reported = String::from_utf8(out.stderr).expect("utf-8");
    assert!(reported.contains("E_TYPE_MISMATCH_LABEL"));
    assert!(reported.contains("E_TYPE_MISMATCH_SIZE"));
}

/// Write a transient `.crn` file under the system temp dir, removing it
/// when the returned handle drops. Lets a test depend on a source that is
/// intentionally broken at the parse layer without checking one in, and
/// without leaving one behind on every run.
///
/// `label` distinguishes concurrent callers: the harness runs these tests
/// as threads of one process, so the pid alone would have two of them
/// writing and reading the same path.
fn tempfile_with_contents(label: &str, contents: &str) -> TempSource {
    let mut path = std::env::temp_dir();
    let pid = std::process::id();
    path.push(format!("cairn-cli-check-{pid}-{label}.crn"));
    std::fs::write(&path, contents).expect("write tempfile");
    TempSource { path }
}

/// A `.crn` file that exists for the length of one test.
struct TempSource {
    /// Where it was written; `Drop` removes it.
    path: PathBuf,
}

impl TempSource {
    /// The path as the CLI wants it on its argument list.
    fn arg(&self) -> &str {
        self.path.to_str().expect("temp path is utf-8")
    }
}

impl Drop for TempSource {
    fn drop(&mut self) {
        // Best effort: a test that has already failed should report that
        // failure, not a cleanup error on top of it.
        let _ = std::fs::remove_file(&self.path);
    }
}
