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
fn cli_synth_stage_logic_rejects_edition_flag() {
    // The Logic IR is edition-neutral by contract, so `--edition` cannot
    // shape its output. Rather than silently ignoring the flag (which
    // would leave the caller believing it took effect), refuse the run
    // with exit 2. Same policy applies to `--stage netlist`.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "logic",
        "--edition",
        "java",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("--edition"),
        "usage hint should name the stray flag, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_netlist_rejects_edition_flag() {
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "netlist",
        "--edition",
        "bedrock",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn cli_synth_stage_placement_java_places_or_cell_at_origin() {
    // `--stage placement --edition java` runs the Edition Netlist IR
    // through the placement pass. `redstone-door.crn`'s sole cell should
    // land at `{x:0,y:0,z:0}` inside its `circuit region=floor void=2`
    // reservation (width/depth copied from `size=7x5`). `wire_length`
    // and `delay_ticks` are absent from the JSON today because Steiner
    // routing and delay insertion are follow-up passes.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "placement",
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
    let gatehouse = value
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == "gatehouse"))
        .expect("gatehouse scope");
    let ir = &gatehouse["ir"];
    assert_eq!(ir["edition"], "java");
    let region = &ir["region"];
    assert_eq!(region["label"], "floor");
    assert_eq!(region["void"], 2);
    assert_eq!(region["width"], 7);
    assert_eq!(region["depth"], 5);
    let cells = ir["cells"].as_array().expect("cells array");
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0]["cell"], "java_repeater_or");
    let coord = &cells[0]["coord"];
    assert_eq!(coord["x"], 0);
    assert_eq!(coord["y"], 0);
    assert_eq!(coord["z"], 0);
    assert!(
        cells[0].get("wire_length").is_none(),
        "wire_length must be elided today: {stdout}",
    );
    assert!(
        cells[0].get("delay_ticks").is_none(),
        "delay_ticks must be elided today: {stdout}",
    );
}

#[test]
fn cli_synth_stage_placement_bedrock_matches_java_layout() {
    // Swapping to `--edition bedrock` picks the Bedrock cell realisation
    // but the reservation and coordinate are edition-independent by
    // contract, so only the `cell` tag and `edition` field differ from
    // the Java run.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "placement",
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
    assert_eq!(ir["cells"][0]["coord"]["x"], 0);
}

#[test]
fn cli_synth_stage_placement_requires_edition_flag() {
    // `--stage placement` without `--edition` is a usage mistake: the
    // Placement IR carries an `edition` field on every scope, so
    // running without a target would silently pick a default the
    // caller did not choose. Exit 2 with a usage hint instead.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "placement",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("--edition"),
        "usage hint should name the missing flag, got: {stderr}",
    );
    assert!(
        stderr.contains("--stage placement"),
        "usage hint should name the failing stage as it is spelled on \
         the CLI so the mirror stays in sync with clap, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_placement_missing_region_exits_one() {
    // A scope with cells but no `circuit region=` line surfaces
    // `E_NO_CIRCUIT_REGION` on stderr and exits 1 — the symmetric E2E
    // gate to the congestion case below, so both fail-loud paths are
    // wired through the CLI in the same shape.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("noregion.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct noregion size=7x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b\n  \
        logic sig.open = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.open\n";
    std::fs::write(&path, source).expect("write missing-region fixture");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "placement",
        "--edition",
        "java",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_NO_CIRCUIT_REGION"),
        "expected E_NO_CIRCUIT_REGION on stderr, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_placement_congestion_exits_one() {
    // A scope whose synthesised netlist overflows its `circuit
    // region=... void=N` reservation should fail loud with
    // `E_ROUTE_CONGESTION` on stderr and exit 1 — the same convention
    // the synth pass's earlier fail-loud diagnostics follow.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("tiny.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct tiny size=3x3\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b\n  \
        logic sig.and_ab   = sig.a and sig.b\n  \
        logic sig.or_ab    = sig.a or sig.b\n  \
        logic sig.combined = sig.and_ab and sig.or_ab\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.combined\n  \
        circuit region=floor void=1\n";
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "placement",
        "--edition",
        "java",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ROUTE_CONGESTION"),
        "expected E_ROUTE_CONGESTION on stderr, got: {stderr}",
    );
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
    assert!(
        stderr.contains("--stage edition"),
        "usage hint should name the failing stage as it is spelled on \
         the CLI so the mirror stays in sync with clap, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_route_java_fills_wire_length() {
    // `--stage route --edition java` runs Steiner routing over the
    // Placement IR. `redstone-door.crn`'s sole OR cell should carry
    // `wire_length = 3` (Manhattan(input_pad_0 → cell) + Manhattan(
    // input_pad_1 → cell) = 1 + 2) in the routed JSON, while
    // `delay_ticks` stays elided because the delay-insertion pass is
    // stage 3 of §14.5 and has not landed yet.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "route",
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
    let gatehouse = value
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == "gatehouse"))
        .expect("gatehouse scope");
    let ir = &gatehouse["ir"];
    assert_eq!(ir["edition"], "java");
    let cells = ir["cells"].as_array().expect("cells array");
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0]["cell"], "java_repeater_or");
    assert_eq!(cells[0]["wire_length"], 3);
    assert!(
        cells[0].get("delay_ticks").is_none(),
        "delay_ticks must be elided at this stage: {stdout}",
    );
}

#[test]
fn cli_synth_stage_route_bedrock_matches_java_wire_length() {
    // Wire length is edition-independent by construction (the cell
    // and pad coordinates are the same on Java and Bedrock), so
    // routing must produce the same `wire_length` on both editions.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "route",
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
    assert_eq!(ir["cells"][0]["wire_length"], 3);
}

#[test]
fn cli_synth_stage_route_requires_edition_flag() {
    // `--stage route` without `--edition` is a usage mistake symmetric
    // to `--stage placement` / `--stage edition`: exit 2 with a hint.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "route",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("--edition"),
        "usage hint should name the missing flag, got: {stderr}",
    );
    assert!(
        stderr.contains("--stage route"),
        "usage hint should name the tripped stage so a caller cannot mis-attribute the error, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_route_congestion_exits_one() {
    // A scope that passes placement at the cell-only budget boundary
    // but overflows once routing lays wires must fail loud with
    // `E_ROUTE_CONGESTION` on stderr and exit 1 — the same convention
    // the earlier stages follow.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("pack.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct pack size=4x3\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b\n  \
        logic sig.and_ab   = sig.a and sig.b\n  \
        logic sig.or_ab    = sig.a or sig.b\n  \
        logic sig.combined = sig.and_ab and sig.or_ab\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.combined\n  \
        circuit region=floor void=1\n";
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "route",
        "--edition",
        "java",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ROUTE_CONGESTION"),
        "expected E_ROUTE_CONGESTION on stderr, got: {stderr}",
    );
    assert!(
        stderr.contains("routed netlist for struct `pack`"),
        "primary should name the routing origin and failed scope, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_route_rejects_missing_edition_when_stage_neutral() {
    // Consistency check with the existing `--stage logic` /
    // `--stage netlist` refuse-`--edition` behaviour: passing
    // `--edition` on `--stage netlist` still exits 2 even after
    // `route` joins the accept list.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "netlist",
        "--edition",
        "java",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn cli_synth_stage_delay_java_fills_delay_ticks() {
    // `--stage delay --edition java` runs delay insertion over the
    // routed IR. `redstone-door.crn`'s sole `JavaRepeaterOr` cell
    // picks up `delay_ticks = 1` (base 1 tick, no implicit buffer
    // repeater because both driver segments sit under the 15-block
    // attenuation limit) and `wire_length` survives from the routing
    // stage.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "delay",
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
    let gatehouse = value
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == "gatehouse"))
        .expect("gatehouse scope");
    let ir = &gatehouse["ir"];
    assert_eq!(ir["edition"], "java");
    let cells = ir["cells"].as_array().expect("cells array");
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0]["cell"], "java_repeater_or");
    assert_eq!(cells[0]["wire_length"], 3);
    assert_eq!(cells[0]["delay_ticks"], 1);
}

#[test]
fn cli_synth_stage_delay_bedrock_matches_bedrock_torch_or() {
    // BedrockTorchOr is a bare dust merge, so `delay_ticks = 0` on
    // Bedrock even though the same DSL source yields `delay_ticks = 1`
    // on Java. Pins the "delay is edition-specific by cell choice"
    // split into the CLI surface.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "delay",
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
    assert_eq!(ir["cells"][0]["wire_length"], 3);
    assert_eq!(ir["cells"][0]["delay_ticks"], 0);
}

#[test]
fn cli_synth_stage_delay_requires_edition_flag() {
    // `--stage delay` without `--edition` is a usage mistake symmetric
    // to `--stage route`: exit 2 with a hint naming the tripped stage.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "delay",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("--edition"),
        "usage hint should name the missing flag, got: {stderr}",
    );
    assert!(
        stderr.contains("--stage delay"),
        "usage hint should name the tripped stage so a caller cannot mis-attribute the error, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_delay_inherits_upstream_congestion_failure() {
    // A scope that fails at the routing stage (E_ROUTE_CONGESTION) is
    // elided from routing's output. The delay stage runs on the elided
    // set, so its own diagnostics list is empty; but the routing
    // failure was already reported and the process exits 1. Pins the
    // "delay stage inherits upstream fail-loud" contract so an author
    // running `--stage delay` sees the same congestion errors they
    // would have seen from `--stage route`.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("pack.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct pack size=4x3\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b\n  \
        logic sig.and_ab   = sig.a and sig.b\n  \
        logic sig.or_ab    = sig.a or sig.b\n  \
        logic sig.combined = sig.and_ab and sig.or_ab\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.combined\n  \
        circuit region=floor void=1\n";
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "delay",
        "--edition",
        "java",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ROUTE_CONGESTION"),
        "expected E_ROUTE_CONGESTION inherited from routing on stderr, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_delay_attenuation_limit_exits_one() {
    // A scope whose routed output-pad segment exceeds the v1
    // attenuation cap must fail loud with `E_ATTENUATION_LIMIT` on
    // stderr and exit 1. Uses a 300-block-wide region so the
    // `sig.out` output driver spans the full x-axis to the right-edge
    // output pad — that segment (~300 blocks) sits well past the
    // 256-block cap.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("wide.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct wide_pack size=300x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b\n  \
        logic sig.out = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.out\n  \
        circuit region=floor void=3\n";
    std::fs::write(&path, source).expect("write attenuation fixture");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "delay",
        "--edition",
        "java",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ATTENUATION_LIMIT"),
        "expected E_ATTENUATION_LIMIT on stderr, got: {stderr}",
    );
    assert!(
        stderr.contains("routed netlist for struct `wide_pack`"),
        "primary should name the delay-side origin and failed scope, got: {stderr}",
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

#[test]
fn cli_synth_stage_crossing_java_legalizes_or_cell_scope() {
    // `--stage crossing --edition java` runs the full pipeline
    // through stage 4. The `redstone-door.crn` fixture has a single
    // net with short segments, so the legalized IR matches the
    // delayed IR verbatim (no crossings, no buffers) — but the
    // stage's JSON round-trip must still succeed and expose the same
    // scope shape.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "crossing",
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
    let gatehouse = value
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == "gatehouse"))
        .expect("gatehouse scope");
    let ir = &gatehouse["ir"];
    assert_eq!(ir["edition"], "java");
    let cells = ir["cells"].as_array().expect("cells array");
    assert_eq!(cells.len(), 1);
    assert!(
        cells[0].get("buffer_coords").is_none(),
        "empty buffer_coords must serde-skip so the wire form stays byte-identical to --stage delay: {stdout}",
    );
    assert!(
        cells[0]["coord"].get("layer").is_none(),
        "plane cell coord must serde-skip its layer field: {stdout}",
    );
}

#[test]
fn cli_synth_stage_crossing_requires_edition_flag() {
    // `--stage crossing` without `--edition` follows the placement /
    // route / delay pattern: exit 2 with a usage hint that names the
    // required flag. The crossing pass reads edition-tagged cells, so
    // the flag is not optional.
    let path = examples_dir().join("redstone-door.crn");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "crossing",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("--edition"),
        "usage hint should name the required flag, got: {stderr}",
    );
    assert!(
        stderr.contains("--stage crossing"),
        "usage hint should name the failing stage as it is spelled on \
         the CLI so the mirror stays in sync with clap, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_crossing_inherits_upstream_attenuation_failure() {
    // `--stage crossing` runs stages 1-4 in sequence, so an
    // Error-severity diagnostic from any prior stage (here, the
    // delay pass's `E_ATTENUATION_LIMIT` on a 300-block-wide region)
    // short-circuits with exit 1 before stage 4 runs. The stderr
    // still names the origin stage so a downstream reader can tell
    // which pass tripped.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("wide.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct wide_pack size=300x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b\n  \
        logic sig.out = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.out\n  \
        circuit region=floor void=3\n";
    std::fs::write(&path, source).expect("write attenuation fixture");
    let out = run_synth(&[
        "--experimental-logic-synth",
        "--stage",
        "crossing",
        "--edition",
        "java",
        path.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ATTENUATION_LIMIT"),
        "expected upstream E_ATTENUATION_LIMIT on stderr, got: {stderr}",
    );
}
