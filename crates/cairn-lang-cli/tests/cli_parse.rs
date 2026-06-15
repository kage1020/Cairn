//! End-to-end tests for `cairn parse <file>`.
//!
//! These tests invoke the freshly compiled binary against every example so the
//! M1 gate ("source parses") is enforced from the CLI surface, not just from
//! the library API.

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

fn run_parse(file: &str) -> std::process::Output {
    let path = examples_dir().join(file);
    Command::new(cargo_bin())
        .arg("parse")
        .arg(&path)
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn parses_cottage_example() {
    let out = run_parse("cottage.crn");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed.get("items").is_some());
}

#[test]
fn parses_themed_tower_example() {
    let out = run_parse("themed-tower.crn");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
}

#[test]
fn parses_village_example() {
    let out = run_parse("village.crn");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
}

#[test]
fn parses_redstone_door_example() {
    let out = run_parse("redstone-door.crn");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
}

#[test]
fn missing_file_exits_with_code_two() {
    let out = Command::new(cargo_bin())
        .arg("parse")
        .arg("does-not-exist.crn")
        .output()
        .expect("invoke cairn");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn debug_format_runs_successfully() {
    // `--format debug` is a developer-facing escape hatch; its concrete shape
    // is whatever `{:?}` happens to produce and is not part of the CLI
    // contract. We only assert that the subcommand exits successfully and
    // writes non-empty UTF-8 to stdout, so the test does not couple to the
    // derived `Debug` representation of internal AST types.
    let path = examples_dir().join("cottage.crn");
    let out = Command::new(cargo_bin())
        .arg("parse")
        .arg(&path)
        .arg("--format")
        .arg("debug")
        .output()
        .expect("invoke cairn");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(!stdout.trim().is_empty(), "debug stdout should not be empty");
}
