//! `cairn info` builds each struct's circuit for every edition it reports on, and reports a circuit
//! finding once when each of those editions raises it, else under the edition that does.

use tempfile::TempDir;

use crate::common::{
    QUASI_CONNECTED_DISPENSER, Run, WALL_LEVER, assert_stream, cairn, write_source,
};

/// `cairn info` of `source` with `args` after the file.
fn info(source: &str, args: &[&str]) -> Run {
    let tmp = TempDir::new().expect("tempdir");
    let path = write_source(tmp.path(), "s.crn", source);
    let mut command_line = vec![path.to_str().unwrap()];
    command_line.extend_from_slice(args);
    Run::from(cairn("info", &command_line))
}

#[test]
fn a_circuit_finding_every_edition_raises_is_reported_once() {
    let run = info(WALL_LEVER, &[]);
    assert_eq!(run.code, Some(1), "stderr={}", run.stderr);
    assert_stream(
        &run.stderr,
        &[(8, 3, "error", "E_ROUTE_UNREACHABLE_COMPONENT")],
    );
    assert!(!run.stderr.contains("note: reported for"), "{}", run.stderr);

    let json = info(WALL_LEVER, &["--format", "json"]);
    assert_eq!(json.code, Some(1), "stderr={}", json.stderr);
    let parsed: serde_json::Value = serde_json::from_str(&json.stdout).expect("valid JSON");
    let codes: Vec<&str> = parsed["diagnostics"]
        .as_array()
        .expect("diagnostics is an array")
        .iter()
        .map(|d| d["code"].as_str().expect("code is a string"))
        .collect();
    assert_eq!(codes, ["E_ROUTE_UNREACHABLE_COMPONENT"]);
}

#[test]
fn a_circuit_finding_one_edition_raises_is_reported_under_that_edition() {
    let run = info(QUASI_CONNECTED_DISPENSER, &[]);
    assert_eq!(run.code, Some(1), "stderr={}", run.stderr);
    assert_stream(
        &run.stderr,
        &[
            (8, 3, "error", "E_NO_PORTABLE_IMPL"),
            (9, 3, "error", "E_ROUTE_UNREACHABLE_COMPONENT"),
        ],
    );
    let headers: Vec<&str> = run
        .stderr
        .lines()
        .filter(|line| line.starts_with("note: reported for"))
        .collect();
    assert_eq!(
        headers,
        [
            "note: reported for --editions java",
            "note: reported for --editions bedrock"
        ],
    );

    let bedrock = info(QUASI_CONNECTED_DISPENSER, &["--editions", "bedrock"]);
    assert_eq!(bedrock.code, Some(1), "stderr={}", bedrock.stderr);
    assert_stream(
        &bedrock.stderr,
        &[(9, 3, "error", "E_ROUTE_UNREACHABLE_COMPONENT")],
    );
}
