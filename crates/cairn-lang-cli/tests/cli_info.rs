//! End-to-end tests for `cairn info <file>`.

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

fn run_info(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("info")
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn info_1_clean_example_exits_zero_with_three_section_headers() {
    let path = examples_dir().join("cottage.crn");
    let out = run_info(&[path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    for header in [
        "registry compatibility:",
        "edition portability:",
        "semantic-sensitive:",
    ] {
        assert!(
            stdout.contains(header),
            "expected `{header}` line in output, got: {stdout}",
        );
    }
}

#[test]
fn info_2_json_format_is_valid_version_axes() {
    let path = examples_dir().join("cottage.crn");
    let out = run_info(&[path.to_str().unwrap(), "--format", "json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    for field in [
        "registry_compat",
        "edition_portability",
        "semantic_sensitive",
    ] {
        assert!(
            parsed.get(field).is_some(),
            "JSON missing `{field}`: {parsed}",
        );
    }
    let compat = &parsed["registry_compat"];
    assert_eq!(compat["max"], "latest");
    // Cottage example pins `@requires version>=1.20`.
    assert_eq!(compat["min"], "1.20");

    let portability = parsed["edition_portability"]
        .as_array()
        .expect("portability is an array");
    // Default `--editions java,bedrock` → two entries.
    assert_eq!(portability.len(), 2);
    for entry in portability {
        for key in ["edition", "portable", "degraded", "unsupported"] {
            assert!(entry.get(key).is_some(), "missing `{key}` in {entry}");
        }
        assert_eq!(entry["degraded"], 0);
        assert_eq!(entry["unsupported"], 0);
    }

    let sensitive = parsed["semantic_sensitive"]
        .as_array()
        .expect("semantic_sensitive is an array");
    // M2-PR3 leaves the catalog empty; populated in 2026.12.0.
    assert!(sensitive.is_empty());
}

#[test]
fn info_3_missing_file_exits_with_code_two() {
    let out = run_info(&["does-not-exist.crn"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn info_4_editions_flag_controls_portability_entries() {
    let path = examples_dir().join("cottage.crn");
    let out = run_info(&[
        path.to_str().unwrap(),
        "--editions",
        "java",
        "--format",
        "json",
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let portability = parsed["edition_portability"]
        .as_array()
        .expect("portability is an array");
    assert_eq!(portability.len(), 1);
    assert_eq!(portability[0]["edition"], "java");
}

#[test]
fn info_5_all_examples_exit_zero() {
    for name in [
        "cottage.crn",
        "themed-tower.crn",
        "village.crn",
        "redstone-door.crn",
    ] {
        let path = examples_dir().join(name);
        let out = run_info(&[path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{name} should exit 0, stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

#[test]
fn info_6_registry_compatibility_renders_as_single_range_line() {
    // The registry-compatible range is currently edition-agnostic
    // (single `min/max` pair in `RegistryRange`). The text output must
    // not duplicate it per edition — that misled reviewers into reading
    // a per-edition divergence that the data does not carry.
    let path = examples_dir().join("cottage.crn");
    let out = run_info(&[path.to_str().unwrap()]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let line = stdout
        .lines()
        .find(|l| l.starts_with("registry compatibility:"))
        .expect("registry compatibility line");
    // One range, no `Java:` / `Bedrock:` prefix here — those belong on
    // axis 2 (edition portability) only.
    assert!(line.contains("1.20 .. latest"), "got: {line}");
    assert!(
        !line.contains("Java:"),
        "axis 1 should not repeat editions: {line}"
    );
    assert!(
        !line.contains("Bedrock:"),
        "axis 1 should not repeat editions: {line}"
    );
}

#[test]
fn info_7_empty_editions_value_is_rejected_with_exit_two() {
    let path = examples_dir().join("cottage.crn");
    let out = run_info(&[path.to_str().unwrap(), "--editions", ""]);
    assert_eq!(out.status.code(), Some(2));
    let out = run_info(&[path.to_str().unwrap(), "--editions", "java,,bedrock"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn info_8_file_without_requires_defaults_min_to_zero_zero() {
    let path = tempfile_with_contents("struct s size=4x4\n  walls height=3\n");
    let out = run_info(&[path.to_str().unwrap(), "--format", "json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(parsed["registry_compat"]["min"], "0.0");
}

fn tempfile_with_contents(contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let pid = std::process::id();
    path.push(format!("cairn-cli-info-{pid}.crn"));
    std::fs::write(&path, contents).expect("write tempfile");
    path
}
