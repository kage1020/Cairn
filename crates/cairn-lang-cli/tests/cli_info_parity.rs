//! Parity table AC pins for `cairn info --editions ...`.
//!
//! The `edition_portability` axis grew from a hard-coded
//! `degraded: 0, unsupported: 0` fill to a real per-edition classification
//! driven by `cairn-lang-formats::portability`. These tests hold the
//! contract from the CLI side end-to-end so a future refactor of the
//! dry-run wiring can't silently re-introduce the zero-fill.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn info_json(file: &str, editions: &str) -> Value {
    let path = examples_dir().join(file);
    let out = Command::new(cargo_bin())
        .args([
            "info",
            path.to_str().unwrap(),
            "--editions",
            editions,
            "--format",
            "json",
        ])
        .output()
        .expect("run cairn");
    assert!(
        out.status.success(),
        "cairn info failed for {file}; stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    serde_json::from_str(&stdout).expect("valid JSON")
}

fn portability_entry<'a>(axes: &'a Value, edition: &str) -> &'a Value {
    axes["edition_portability"]
        .as_array()
        .expect("edition_portability is a JSON array")
        .iter()
        .find(|e| e["edition"] == edition)
        .unwrap_or_else(|| panic!("edition `{edition}` missing from portability list"))
}

fn as_u64(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or_else(|| {
        panic!(
            "expected {key} to be an unsigned integer, got {}",
            value[key]
        )
    })
}

#[test]
fn ac1_themed_tower_bedrock_reports_at_least_one_degraded_palette_entry() {
    // AC1: themed-tower interns non-`straight` stair `shape` at least
    // once (the eave with `shape=outer_left`); Bedrock has no `shape`
    // state so that palette entry counts as degraded.
    let axes = info_json("themed-tower.crn", "bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    let degraded = as_u64(bedrock, "degraded");
    let unsupported = as_u64(bedrock, "unsupported");
    assert!(
        degraded >= 1,
        "expected at least one degraded Bedrock palette entry in themed-tower, got {degraded}",
    );
    assert_eq!(
        unsupported, 0,
        "themed-tower on Bedrock should have no unsupported entries (only degraded stair shape), got {unsupported}",
    );
}

#[test]
fn ac2_cottage_bedrock_has_no_unsupported_entries() {
    // AC2: cottage carries only bare blocks + gable stairs whose
    // `shape=straight` is Bedrock's default (lossless drop). No family
    // outside the mapped set should appear.
    let axes = info_json("cottage.crn", "bedrock");
    let bedrock = portability_entry(&axes, "bedrock");
    assert_eq!(
        as_u64(bedrock, "unsupported"),
        0,
        "cottage on Bedrock must not report unsupported entries",
    );
    // `degraded` is a real measurement — pin exactly whatever the current
    // roof lowering produces so a future regression that inflates it
    // (e.g. a gable-corner promotion) trips this AC on purpose.
    let degraded = as_u64(bedrock, "degraded");
    assert!(
        degraded <= 1,
        "cottage on Bedrock: unexpectedly high degraded count {degraded} — the gable roof should stay all-straight",
    );
}

#[test]
fn ac3_java_axis_is_always_pure_portable() {
    // AC3: Java is the base edition (spec versioning-editions §10.3);
    // every non-air palette entry must classify as portable. This holds
    // across every example — pin it on the two files carrying the widest
    // block variety.
    for file in ["themed-tower.crn", "cottage.crn"] {
        let axes = info_json(file, "java");
        let java = portability_entry(&axes, "java");
        assert_eq!(
            as_u64(java, "degraded"),
            0,
            "{file}: Java must never report degraded entries",
        );
        assert_eq!(
            as_u64(java, "unsupported"),
            0,
            "{file}: Java must never report unsupported entries",
        );
        assert!(
            as_u64(java, "portable") > 0,
            "{file}: Java should always have at least one portable palette entry",
        );
    }
}

#[test]
fn info_degraded_count_matches_compile_intent_degraded_warnings() {
    // Info↔compile cross-consistency: `portability_for_bedrock` and
    // `build_mcstructure_tag` share `translate_states` as the source of
    // truth. If `cairn info` reports `degraded >= 1` on Bedrock, then
    // `cairn compile --edition bedrock` on the same file must emit at
    // least one `W_INTENT_DEGRADED` warning. Without this pin, the two
    // sides could drift silently — e.g. an internal refactor that adds
    // a degradation source to `translate_states` in the compile path but
    // not the info path (or vice versa) would let the parity table
    // undercount vs. what the writer actually degrades.
    let src_repo = examples_dir().join("themed-tower.crn");
    // 1. Info-side degraded count.
    let axes = info_json("themed-tower.crn", "bedrock");
    let degraded = as_u64(portability_entry(&axes, "bedrock"), "degraded");
    assert!(
        degraded >= 1,
        "expected themed-tower Bedrock degraded >= 1 for cross-consistency check, got {degraded}",
    );

    // 2. Compile-side warning count. Copy the source into a tempdir so
    //    the lockfile doesn't land next to the repo copy.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let copied = tmp.path().join("themed-tower.crn");
    std::fs::copy(&src_repo, &copied).expect("copy themed-tower");
    let out_dir = tempfile::TempDir::new().expect("out tempdir");
    let compile = Command::new(cargo_bin())
        .args([
            "compile",
            copied.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--out",
            out_dir.path().to_str().unwrap(),
        ])
        .output()
        .expect("run cairn");
    assert!(
        compile.status.success(),
        "compile failed; stderr={}",
        String::from_utf8_lossy(&compile.stderr),
    );
    let stderr = String::from_utf8_lossy(&compile.stderr);
    let warning_count = stderr.matches("W_INTENT_DEGRADED").count();
    assert!(
        warning_count >= 1,
        "info reported degraded >= 1 but compile emitted no W_INTENT_DEGRADED; \
         info and compile paths must share `translate_states` as source of truth; stderr={stderr}",
    );
}

#[test]
fn ac4_unknown_edition_rejected_with_exit_two() {
    // AC4: `--editions foo` fails before dry-run lowering runs (which
    // would otherwise silently produce a zero-fill row that a caller
    // couldn't distinguish from a real portable-only result).
    let path = examples_dir().join("cottage.crn");
    let out = Command::new(cargo_bin())
        .args(["info", path.to_str().unwrap(), "--editions", "foo"])
        .output()
        .expect("run cairn");
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2 for unknown edition, got {:?}; stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    // The message threads the offending value + the closed valid set,
    // matching the spec §10.4 self-correction triple (what is wrong /
    // what is valid / how to fix).
    assert!(
        stderr.contains("foo"),
        "stderr should name the offending value, got: {stderr}",
    );
    assert!(
        stderr.contains("java, bedrock"),
        "stderr should list valid editions, got: {stderr}",
    );
}
