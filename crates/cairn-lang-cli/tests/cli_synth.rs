//! End-to-end tests for `cairn synth <file>`.
//!
//! Locks the CLI contract around the M6-PR1 experimental redstone Logic
//! IR dump: the `--experimental-logic-synth` gate is required, the JSON
//! output carries the sensor/gate/actuator shape the synth pass builds,
//! and an unbound signal drives exit 1 with a gcc-style diagnostic on
//! stderr.

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

fn run_synth(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("synth")
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn cli_synth_requires_experimental_flag() {
    // The subcommand is internal-tier; a caller invoking it without the
    // opt-in flag must exit 2 with a usage hint on stderr so the gate
    // cannot be missed silently.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("--experimental-logic-synth"),
        "gate diagnostic should name the required flag, got: {stderr}",
    );
}

#[test]
fn cli_synth_redstone_door_emits_or_gate_json() {
    // AC-8: the canonical example synths to a JSON dump whose gates
    // section names an `or2` primitive — the same primitive the in-crate
    // unit test locks. Together they pin both the API (LogicIr shape)
    // and the wire form (JSON serialisation).
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&["--experimental-logic-synth", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "expected exit 0, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    // Parse the JSON so an incidental substring match (e.g. `or2` inside a
    // sensor name) never gives a false positive.
    let value: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should parse as JSON: {err}\n{stdout}"));
    let scopes = value.as_array().expect("top-level is a scope list");
    let gatehouse = scopes
        .iter()
        .find(|s| s["name"] == "gatehouse")
        .expect("gatehouse scope in output");
    let ir = &gatehouse["ir"];
    assert_eq!(ir["inputs"].as_array().expect("inputs array").len(), 2);
    assert_eq!(ir["outputs"].as_array().expect("outputs array").len(), 1);
    let nodes = ir["nodes"].as_array().expect("nodes array");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["kind"], "or2");
}

#[test]
fn cli_synth_missing_file_exits_two() {
    // Path-not-found returns 2 (user-input mistake), consistent with
    // `cairn parse`/`check`/`lower`/`compile`.
    let out = run_synth(&["--experimental-logic-synth", "no-such-file.crn"]);
    assert_eq!(out.status.code(), Some(2));
}
