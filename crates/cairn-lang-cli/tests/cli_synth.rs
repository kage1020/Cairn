//! End-to-end tests for `cairn synth <file>`.
//!
//! Locks the CLI contract around the experimental redstone Logic IR
//! dump: the `--experimental-logic-synth` gate is required, the JSON
//! output carries the sensor/gate/actuator shape the synth pass builds,
//! parse failures exit 1 with a gcc-style diagnostic on stderr, and a
//! missing file exits 2.

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
    // The canonical example synths to a JSON dump whose gates section
    // names an `or2` primitive — the same primitive the in-crate
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
    // GateKind uses `#[serde(tag = "kind", ...)]` so a gate node's `kind`
    // field is itself an object carrying the primitive name plus operand
    // fields; the primitive tag lives one level deep.
    assert_eq!(nodes[0]["kind"]["kind"], "or2");
}

#[test]
fn cli_synth_stage_netlist_emits_or_cell_json() {
    // `--stage netlist` prints the Netlist IR of the same example, one
    // step down the pipeline: gates become cells tagged with a
    // `LogicalCell`, and drivers become `NetRef`s. Pins the JSON shape
    // the Netlist IR stage exposes.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "netlist",
        path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "expected exit 0, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
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
    let cells = ir["cells"].as_array().expect("cells array");
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0]["cell"], "or");
    let drivers = cells[0]["drivers"].as_array().expect("drivers array");
    assert_eq!(drivers.len(), 2);
    assert_eq!(drivers[0]["port"], "a");
    assert_eq!(drivers[1]["port"], "b");
    assert_eq!(ir["outputs"][0]["driver"]["kind"], "cell");
}

#[test]
fn cli_synth_stage_edition_java_maps_or_cell_to_java_repeater_or() {
    // `--stage edition --edition java` picks the Java realisation of each
    // Netlist IR cell. `redstone-door.crn`'s sole Or cell should surface
    // as `java_repeater_or` and the scope should carry `edition: "java"`.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "edition",
        "--edition",
        "java",
        path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "expected exit 0, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let value: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should parse as JSON: {err}\n{stdout}"));
    let scopes = value.as_array().expect("top-level is a scope list");
    let gatehouse = scopes
        .iter()
        .find(|s| s["name"] == "gatehouse")
        .expect("gatehouse scope in output");
    let ir = &gatehouse["ir"];
    assert_eq!(ir["edition"], "java");
    let cells = ir["cells"].as_array().expect("cells array");
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0]["cell"], "java_repeater_or");
    let drivers = cells[0]["drivers"].as_array().expect("drivers array");
    assert_eq!(drivers.len(), 2);
    assert_eq!(drivers[0]["port"], "a");
    assert_eq!(drivers[1]["port"], "b");
}

#[test]
fn cli_synth_stage_edition_bedrock_maps_or_cell_to_bedrock_torch_or() {
    // `--edition bedrock` swaps in the Bedrock realisation. Everything
    // else about the scope (inputs, outputs, driver arity) is
    // edition-independent and should match the Java run byte-for-byte
    // apart from the cell tag and the edition field.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "edition",
        "--edition",
        "bedrock",
        path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "expected exit 0, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let value: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout should parse as JSON: {err}\n{stdout}"));
    let gatehouse = value
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == "gatehouse"))
        .expect("gatehouse scope");
    let ir = &gatehouse["ir"];
    assert_eq!(ir["edition"], "bedrock");
    assert_eq!(ir["cells"][0]["cell"], "bedrock_torch_or");
}

#[test]
fn cli_synth_stage_edition_requires_edition_flag() {
    // `--stage edition` without `--edition` is a usage mistake: exit 2 so
    // a script that forgets the flag cannot silently emit a default
    // Java-tagged IR the caller did not ask for.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "edition",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("--edition"),
        "usage hint should name the missing flag, got: {stderr}",
    );
}

#[test]
fn cli_synth_missing_file_exits_two() {
    // Path-not-found returns 2 (user-input mistake), consistent with
    // `cairn parse`/`check`/`lower`/`compile`.
    let out = run_synth(&["--experimental-logic-synth", "no-such-file.crn"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn cli_synth_unparseable_source_exits_one() {
    // Parse-level failure follows the same exit-code convention as
    // `cairn parse` / `check`: exit 1 (build problem), position-anchored
    // error on stderr. Using a scratch file rather than a fixture so the
    // test does not add a permanent broken example under `tests/`.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("broken.crn");
    std::fs::write(&path, "@cairn 2026.06\n\nstruct 3xnot-a-size\n").expect("write scratch file");
    let out = run_synth(&["--experimental-logic-synth", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("error"),
        "expected an error line on stderr, got: {stderr}",
    );
}
