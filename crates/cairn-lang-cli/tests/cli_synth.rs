//! End-to-end tests for `cairn synth <file>`.
//!
//! Locks the CLI contract around the experimental redstone Logic IR
//! dump: the `--experimental-logic-synth` gate is required, the JSON
//! output carries the sensor/gate/actuator shape the synth pass builds,
//! parse failures exit 1 with a gcc-style diagnostic on stderr, and a
//! missing file exits 2.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::OnceLock;

mod common;
use common::{cairn, examples_dir, write_source};

#[test]
fn cli_synth_requires_experimental_flag() {
    // The subcommand is internal-tier; a caller invoking it without the
    // opt-in flag must exit 2 with a usage hint on stderr so the gate
    // cannot be missed silently.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn("synth", &[path.to_str().unwrap()]);
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
    let out = cairn(
        "synth",
        &["--experimental-logic-synth", path.to_str().unwrap()],
    );
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
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "netlist",
            path.to_str().unwrap(),
        ],
    );
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
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "edition",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
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
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "edition",
            "--edition",
            "bedrock",
            path.to_str().unwrap(),
        ],
    );
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
fn cli_synth_stage_placement_java_places_or_cell_beside_the_pad_column() {
    // `--stage placement --edition java` runs the Edition Netlist IR
    // through the placement pass. `redstone-door.crn`'s sole cell should
    // land at `{x:1,y:0,z:1}` inside its `circuit region=floor void=2`
    // reservation (width/depth copied from `size=7x5`) — one column in
    // from the pad column and one row in from the near edge, per the
    // spaced row. `wire_length` and `local_delay_ticks` are absent at
    // `--stage placement`, which runs neither routing nor delay
    // insertion.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "placement",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
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
    assert_eq!(
        cells[0]["stage"], "placement",
        "the stage tag must echo the --stage flag that produced the dump: {stdout}",
    );
    let coord = &cells[0]["coord"];
    assert_eq!(coord["x"], 1);
    assert_eq!(coord["y"], 0);
    assert_eq!(coord["z"], 1);
    assert!(
        cells[0].get("wire_length").is_none(),
        "wire_length must be elided today: {stdout}",
    );
    assert!(
        cells[0].get("local_delay_ticks").is_none(),
        "local_delay_ticks must be elided today: {stdout}",
    );
}

#[test]
fn cli_synth_stage_placement_bedrock_matches_java_layout() {
    // Swapping to `--edition bedrock` picks the Bedrock cell realisation
    // but the reservation and coordinate are edition-independent by
    // contract, so only the `cell` tag and `edition` field differ from
    // the Java run.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "placement",
            "--edition",
            "bedrock",
            path.to_str().unwrap(),
        ],
    );
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
    assert_eq!(ir["cells"][0]["coord"]["x"], 1);
}

#[test]
fn cli_synth_stage_placement_requires_edition_flag() {
    // `--stage placement` without `--edition` is a usage mistake: the
    // Placement IR carries an `edition` field on every scope, so
    // running without a target would silently pick a default the
    // caller did not choose. Exit 2 with a usage hint instead.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "placement",
            path.to_str().unwrap(),
        ],
    );
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
        pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
        logic sig.open = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.open\n";
    std::fs::write(&path, source).expect("write missing-region fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "placement",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
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
        pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
        logic sig.and_ab   = sig.a and sig.b\n  \
        logic sig.or_ab    = sig.a or sig.b\n  \
        logic sig.combined = sig.and_ab and sig.or_ab\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.combined\n  \
        circuit region=floor void=1\n";
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "placement",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
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
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "edition",
            path.to_str().unwrap(),
        ],
    );
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
    // `wire_length = 4` — two from each pad, which stand a row either
    // side of the cell's row because the pad column steps over it —
    // in the routed JSON, while `local_delay_ticks`
    // stays elided because this dump stops at stage 2.
    // `cli_synth_stage_delay_java_fills_local_delay_ticks` is the stage-3
    // dump where it appears.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "route",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
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
    assert_eq!(
        cells[0]["stage"], "route",
        "the stage tag must echo the --stage flag that produced the dump: {stdout}",
    );
    assert_eq!(cells[0]["wire_length"], 4);
    assert!(
        cells[0].get("local_delay_ticks").is_none(),
        "local_delay_ticks must be elided at this stage: {stdout}",
    );
}

#[test]
fn cli_synth_stage_route_bedrock_matches_java_wire_length() {
    // Wire length is edition-independent by construction (the cell
    // and pad coordinates are the same on Java and Bedrock), so
    // routing must produce the same `wire_length` on both editions.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "route",
            "--edition",
            "bedrock",
            path.to_str().unwrap(),
        ],
    );
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
    assert_eq!(ir["cells"][0]["wire_length"], 4);
}

#[test]
fn cli_synth_stage_route_requires_edition_flag() {
    // `--stage route` without `--edition` is a usage mistake symmetric
    // to `--stage placement` / `--stage edition`: exit 2 with a hint.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "route",
            path.to_str().unwrap(),
        ],
    );
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
    // Two cells in the shortest row that holds them, in the shallowest
    // region the row fits in: seven coords are left over once the cells
    // are counted, and the wire spends more than seven. One sensor
    // rather than two so the chain routes — a stranded sink would
    // refuse this a stage earlier, for a different reason.
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct pack size=5x3\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        logic sig.c0 = not sig.a\n  \
        logic sig.c1 = sig.c0 and sig.a\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.c1\n  \
        circuit region=floor void=1\n";
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "route",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
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
fn cli_synth_stage_delay_java_fills_local_delay_ticks() {
    // `--stage delay --edition java` runs delay insertion over the
    // routed IR. `redstone-door.crn`'s sole `JavaRepeaterOr` cell
    // picks up `local_delay_ticks = 1` (base 1 tick, no implicit buffer
    // repeater because both driver segments sit under the 15-block
    // attenuation limit) and `wire_length` survives from the routing
    // stage.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "delay",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
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
    assert_eq!(
        cells[0]["stage"], "delay",
        "the stage tag must echo the --stage flag that produced the dump: {stdout}",
    );
    assert_eq!(cells[0]["wire_length"], 4);
    assert_eq!(cells[0]["local_delay_ticks"], 1);
    // The rename is a breaking change to this dump, so the old key
    // has to be gone and not merely joined by the new one.
    assert!(
        cells[0].get("delay_ticks").is_none(),
        "the pre-rename key must not survive alongside local_delay_ticks: {stdout}",
    );
}

#[test]
fn cli_synth_stage_delay_bedrock_matches_bedrock_torch_or() {
    // BedrockTorchOr is a bare dust merge, so `local_delay_ticks = 0` on
    // Bedrock even though the same DSL source yields `local_delay_ticks = 1`
    // on Java. Pins the "delay is edition-specific by cell choice"
    // split into the CLI surface.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "delay",
            "--edition",
            "bedrock",
            path.to_str().unwrap(),
        ],
    );
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
    assert_eq!(ir["cells"][0]["wire_length"], 4);
    assert_eq!(ir["cells"][0]["local_delay_ticks"], 0);
    assert!(
        ir["cells"][0].get("delay_ticks").is_none(),
        "the pre-rename key must not survive alongside local_delay_ticks: {stdout}",
    );
}

#[test]
fn cli_synth_stage_delay_requires_edition_flag() {
    // `--stage delay` without `--edition` is a usage mistake symmetric
    // to `--stage route`: exit 2 with a hint naming the tripped stage.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "delay",
            path.to_str().unwrap(),
        ],
    );
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
    // Two cells in the shortest row that holds them, in the shallowest
    // region the row fits in: seven coords are left over once the cells
    // are counted, and the wire spends more than seven. One sensor
    // rather than two so the chain routes — a stranded sink would
    // refuse this a stage earlier, for a different reason.
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct pack size=5x3\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        logic sig.c0 = not sig.a\n  \
        logic sig.c1 = sig.c0 and sig.a\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.c1\n  \
        circuit region=floor void=1\n";
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "delay",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ROUTE_CONGESTION"),
        "expected E_ROUTE_CONGESTION inherited from routing on stderr, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_delay_attenuation_limit_exits_one() {
    // A scope whose output-pad segment exceeds the v1 attenuation cap
    // must fail loud with `E_ATTENUATION_LIMIT` on stderr and exit 1.
    // Uses a 300-block-wide region so the `sig.out` output driver spans
    // the full x-axis to the right-edge output pad — that segment
    // (~300 blocks) sits well past the 256-block cap.
    //
    // `--stage delay` is asked for, but the refusal now comes from the
    // routing pass: the straight line alone is already over the cap, so
    // stage 2's gate answers first and the CLI stops there. Hence the
    // `placed` noun below — the cells are still `Unrouted` when it
    // fires. The delay pass's own routed-length check is exercised by
    // the detour fixtures in `cairn-lang-redstone`, where the straight
    // line fits and only the route does not.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("wide.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct wide_pack size=300x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
        logic sig.out = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.out\n  \
        circuit region=floor void=3\n";
    std::fs::write(&path, source).expect("write attenuation fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "delay",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ATTENUATION_LIMIT"),
        "expected E_ATTENUATION_LIMIT on stderr, got: {stderr}",
    );
    assert!(
        stderr.contains("placed netlist for struct `wide_pack`"),
        "primary should name the stage that refused and the failed scope, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_route_refuses_a_detour_past_the_attenuation_cap() {
    // `sig.s1` feeds the gate from its pad at `(0,0,2)` through `(1,0,2)`,
    // so `sig.s2`'s wire, from its pad at `(0,0,3)`, cannot start along
    // its own row: `(1,0,3)` is one step from that dust. It steps out a
    // row, runs the length of the region and steps back to the far
    // door's pad at `(width - 1, 0, 2)`: its straight line is `width`
    // and its route `width + 2`. At `width = cap - 2` the route is the
    // cap and the scope routes; one wider it is one block over, and the
    // router's search, bounded by the cap, refuses it at stage 2 — still
    // within the cap in a straight line, so the straight-line gate lets
    // it through to the router.
    use cairn_lang_redstone::MAX_ATTENUATION_SEGMENT as CAP;
    let dir = tempfile::tempdir().expect("temp dir");
    for (width, refuses) in [(CAP - 2, false), (CAP - 1, true)] {
        let path = dir.path().join(format!("detour{width}.crn"));
        let source = format!(
            "@cairn 2026.06\n@requires version>=1.20\n\n\
             theme t:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n\
             struct s size={width}x6\n  \
             floor mat_slot=wall\n  \
             door id=d0 side=front at=center mat_slot=door\n  \
             door id=d1 side=back at=center mat_slot=door\n  \
             pressure_plate id=p0 at=front.outside offset=0 y=0 -> sig.s0\n  \
             pressure_plate id=p1 at=back.outside offset=0 y=0 -> sig.s1\n  \
             pressure_plate id=p2 at=front.outside offset=1 y=0 -> sig.s2\n  \
             logic sig.g0 = sig.s0 and sig.s1\n  \
             door[id=d0] opened_by=sig.g0\n  \
             door[id=d1] opened_by=sig.s2\n  \
             circuit region=floor void=1\n"
        );
        std::fs::write(&path, source).expect("write detour fixture");
        let out = cairn(
            "synth",
            &[
                "--experimental-logic-synth",
                "--stage",
                "route",
                "--edition",
                "java",
                path.to_str().unwrap(),
            ],
        );
        let stderr = String::from_utf8(out.stderr).expect("utf-8");
        if !refuses {
            assert_eq!(out.status.code(), Some(0), "width {width}: {stderr}");
            let stdout = String::from_utf8(out.stdout).expect("utf-8");
            let value: serde_json::Value = serde_json::from_str(&stdout)
                .unwrap_or_else(|err| panic!("stdout should parse as JSON: {err}\n{stdout}"));
            let lengths: Vec<u64> = value[0]["ir"]["outputs"]
                .as_array()
                .expect("outputs array")
                .iter()
                .map(|output| output["wire_length"].as_u64().expect("routed"))
                .collect();
            assert_eq!(
                lengths,
                [u64::from(CAP - 3), u64::from(CAP)],
                "the far door's wire is exactly the cap",
            );
            continue;
        }
        assert_eq!(out.status.code(), Some(1), "width {width}: {stderr}");
        let expected = format!(
            "error[E_ATTENUATION_LIMIT]: placed netlist for struct `s` has no route from the \
             driver at (0,0,3) to ({},0,2) within the v1 attenuation limit of {CAP} blocks; the \
             faces it could arrive through are taken by cell #0\n  note: Fix: give the wire a \
             shorter way round",
            width - 1,
        );
        assert!(stderr.contains(&expected), "width {width}: got {stderr}");
    }
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
    let out = cairn(
        "synth",
        &["--experimental-logic-synth", path.to_str().unwrap()],
    );
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
    // through stage 4. Every segment in the `redstone-door.crn`
    // fixture is short, so the legalized IR matches the delayed IR
    // apart from the `stage` tag — but the stage's JSON round-trip
    // must still succeed and expose the same scope shape.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "crossing",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "expected exit 0, stderr={stderr}");
    // Nothing at all. The two sensor pads stand a row either side of
    // the cell's, so each wire comes in through the lane on its own
    // side, and the cell's outward run reaches the door's pad in the
    // straight-line distance, over nothing and round nothing.
    assert!(
        stderr.is_empty(),
        "a scope that routes has nothing to say: {stderr}",
    );
    let stdout = String::from_utf8(out.stdout)
        .unwrap_or_else(|err| panic!("stdout should be utf-8: {err}\nstderr={stderr}"));
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!("stdout should parse as JSON: {err}\nstdout={stdout}\nstderr={stderr}")
    });
    let gatehouse = value
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == "gatehouse"))
        .expect("gatehouse scope");
    let ir = &gatehouse["ir"];
    assert_eq!(ir["edition"], "java");
    let cells = ir["cells"].as_array().expect("cells array");
    assert_eq!(cells.len(), 1);
    assert_eq!(
        cells[0]["stage"], "crossing",
        "the stage tag must echo the --stage flag that produced the dump — it is what \
         distinguishes this zero-buffer legalized dump from its --stage delay input: {stdout}",
    );
    assert!(
        cells[0].get("buffer_coords").is_none(),
        "empty buffer_coords must serde-skip: the stage tag, not a sentinel empty array, is what marks a dump as legalized: {stdout}",
    );
    assert!(
        cells[0]["coord"].get("layer").is_none(),
        "plane cell coord must serde-skip its layer field: {stdout}",
    );
    // Stage 4 carries stage 3's figure forward, under stage 3's new
    // name and not its old one.
    assert_eq!(cells[0]["local_delay_ticks"], 1);
    assert!(
        cells[0].get("delay_ticks").is_none(),
        "the pre-rename key must not survive alongside local_delay_ticks: {stdout}",
    );
}

#[test]
fn cli_synth_stage_crossing_bedrock_legalizes_or_cell_scope() {
    // Everything about crossing legalization on the redstone-door
    // fixture is edition-independent (short segments, and nothing
    // that crosses), so the Bedrock run
    // differs from the Java run only in the cell tag and the edition
    // field. Mirrors
    // the placement / route / delay stage's Java+Bedrock pattern so
    // `--stage crossing` gets the same edition-parity coverage the
    // other edition-tagged stages already have.
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "crossing",
            "--edition",
            "bedrock",
            path.to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "expected exit 0, stderr={stderr}");
    // Nothing at all. The two sensor pads stand a row either side of
    // the cell's, so each wire comes in through the lane on its own
    // side, and the cell's outward run reaches the door's pad in the
    // straight-line distance, over nothing and round nothing.
    assert!(
        stderr.is_empty(),
        "a scope that routes has nothing to say: {stderr}",
    );
    let stdout = String::from_utf8(out.stdout)
        .unwrap_or_else(|err| panic!("stdout should be utf-8: {err}\nstderr={stderr}"));
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!("stdout should parse as JSON: {err}\nstdout={stdout}\nstderr={stderr}")
    });
    let gatehouse = value
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == "gatehouse"))
        .expect("gatehouse scope");
    let ir = &gatehouse["ir"];
    assert_eq!(ir["edition"], "bedrock");
    let cells = ir["cells"].as_array().expect("cells array");
    assert_eq!(cells.len(), 1);
    assert_eq!(
        cells[0]["cell"], "bedrock_torch_or",
        "Bedrock edition realises `or` as the torch-based cell",
    );
    assert_eq!(
        cells[0]["stage"], "crossing",
        "the stage tag must echo the --stage flag that produced the dump: {stdout}",
    );
    // Same serde-skip contract as the Java run: empty buffer_coords
    // and a plane-layer coord both elide their fields so the wire
    // form stays byte-identical to the earlier stages on this fixture
    // apart from the stage tag asserted above.
    assert!(
        cells[0].get("buffer_coords").is_none(),
        "empty buffer_coords must serde-skip: the stage tag, not a sentinel empty array, is what marks a dump as legalized: {stdout}",
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
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "crossing",
            path.to_str().unwrap(),
        ],
    );
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
    // routing pass's `E_ATTENUATION_LIMIT` on a 300-block-wide region)
    // short-circuits with exit 1 before stage 4 runs. The stderr
    // still names the origin stage so a downstream reader can tell
    // which pass tripped.
    //
    // The origin used to be the delay pass; stage 2 now answers first
    // for a segment whose straight line alone is over the cap, and
    // every `.crn` that reaches this code does so by that route. A
    // scope that passes the straight-line gate and fails only the
    // routed-length one can no longer be written in `.crn` at all, so
    // "crossing inherits a *delay* failure" is exercised by a
    // hand-built two-scope fixture in `cairn-lang-redstone`'s
    // `tests/delay.rs` rather than here.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("wide.crn");
    let source = "@cairn 2026.06\n@requires version>=1.20\n\n\
        theme t:\n  slot wall -> @oak_planks\n\n\
        struct wide_pack size=300x5\n  \
        floor mat_slot=wall\n  \
        pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
        pressure_plate id=q at=inside.front  offset=1 y=0 -> sig.b\n  \
        logic sig.out = sig.a or sig.b\n  \
        door id=d side=front at=center mat_slot=wall opened_by=sig.out\n  \
        circuit region=floor void=3\n";
    std::fs::write(&path, source).expect("write attenuation fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "crossing",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ATTENUATION_LIMIT"),
        "expected upstream E_ATTENUATION_LIMIT on stderr, got: {stderr}",
    );
}

#[test]
fn cli_synth_stage_route_reports_cross_layer_clearance_and_exits_zero() {
    // `examples/crossbar.crn` routes: its nets climb past each other,
    // and what an escape leaves a layer apart is the physical tile
    // layer's to separate rather than the router's. The finding that
    // names those pairs is advisory, so it has to reach the caller the
    // way a refusal does — code and coords on stderr — while the IR
    // still reaches stdout and the exit code stays 0. A warning
    // promoted to a refusal by accident would show up here as an empty
    // stdout and an exit of 1.
    let path = examples_dir().join("crossbar.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "route",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("warning[W_ROUTE_CROSS_LAYER_CLEARANCE]"),
        "expected the advisory on stderr, got: {stderr}",
    );
    assert!(
        stderr.contains("within one step of each other across layers")
            && stderr.contains("stands directly over")
            && stderr.contains("the physical tile layer\'s obligation"),
        "the finding says what is left a layer apart, names a pair, and says \
         whose obligation separating them is, got: {stderr}",
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert!(
        stdout.contains("\"name\": \"crossbar\""),
        "an advisory elides nothing — the routed IR still reaches stdout, got: {stdout}",
    );
}

#[test]
fn cli_synth_stage_crossing_two_nets_over_one_coord_exits_one() {
    // Two nets that want one coord, with no layer above the plane to
    // escape onto. The compiler refuses the scope rather than emitting
    // a layout in which those two signals are one strand of dust, and
    // the refusal has to reach the caller through `--stage crossing`
    // the way every earlier stage's does: exit 1, code on stderr,
    // nothing on stdout. Without this case a dropped diagnostic report
    // between the pipeline and the JSON dump would let a refused scope
    // be printed as a legalized IR.
    //
    // One cell, two sensors, two actuators, one service layer. `sig.a`
    // drives both the cell and the back door, so it is laid first, and
    // its run out to that door takes the coords beside the front door's
    // pad; the cell's own output has none left to arrive through and no
    // layer to climb onto.
    let source = "\
theme cross:
  slot wall -> @oak_planks
  slot door -> @oak_door

struct crossbar size=4x4
  floor mat_slot=wall
  door  id=front side=front at=center mat_slot=door
  door  id=back  side=back  at=center mat_slot=door

  pressure_plate id=plate1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=plate2 at=inside.front  offset=1 y=0 -> sig.b

  logic sig.f = sig.a and sig.b

  door[id=front] opened_by=sig.f
  door[id=back]  opened_by=sig.a

  circuit region=floor void=1
";
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("crossbar.crn");
    std::fs::write(&path, source).expect("write congestion fixture");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "crossing",
            "--edition",
            "java",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(
        stderr.contains("E_ROUTE_CONGESTION"),
        "expected E_ROUTE_CONGESTION on stderr, got: {stderr}",
    );
    // `placed`, not `routed`: `--stage crossing` runs the whole pipeline
    // and the CLI stops at the first Error-severity stage, so this scope
    // never reaches the crossing pass — the routing pass refuses it while
    // laying nets over a netlist whose cells are still unrouted. The noun
    // names the netlist the refusing pass read, which is the only way to
    // tell from the message where in the pipeline the scope died.
    assert!(
        stderr.contains("placed netlist for struct `crossbar`")
            && stderr.contains("another net's dust, on the coord or one step from it")
            && stderr.contains("the faces it could arrive through are taken by sig.a"),
        "the refusal names the scope, which of the three kinds of obstacle it \
         means and how far it reaches, and the net standing in the way, got: \
         {stderr}",
    );
    // Which other code leaked would say the fixture drifted into an
    // over-long segment rather than into the congestion this is about.
    assert!(
        !stderr.contains("E_ATTENUATION_LIMIT"),
        "the fixture must refuse for want of room, not for segment length, \
         got: {stderr}",
    );
    assert!(
        out.stdout.is_empty(),
        "a refused scope must not reach stdout as a legalized IR dump, got: {}",
        String::from_utf8_lossy(&out.stdout),
    );
}

/// The number of values `--stage` accepts. Bump it with the next stage;
/// the sweeps below refuse a different count, and a loop over a
/// filtered-down set passes silently.
const SYNTH_STAGES: usize = 7;

/// The number of values `--edition` accepts, on the same terms as
/// [`SYNTH_STAGES`].
const SYNTH_EDITIONS: usize = 2;

/// Every value `--stage` accepts, read back off the binary.
///
/// The unit tests inside the binary walk `SynthStage` through clap's
/// `ValueEnum`; an integration test cannot see that enum, and a
/// literal list here would quietly stop covering a stage the day one
/// lands. Refusing a bogus value gets the same list from the outside,
/// on one line — steadier to parse than the `--help` block, where each
/// value carries a paragraph of prose.
fn stage_values() -> Vec<String> {
    possible_values("--stage", SYNTH_STAGES)
}

/// Every value `--edition` accepts, read back off the binary the same
/// way as [`stage_values`].
fn edition_values() -> Vec<String> {
    possible_values("--edition", SYNTH_EDITIONS)
}

/// The `[possible values: ...]` list clap prints when `synth` is given
/// a bogus value for `flag`, checked against the `expected` count.
fn possible_values(flag: &str, expected: usize) -> Vec<String> {
    let out = cairn("synth", &[flag, "not-a-value", "unused.crn"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "an unknown {flag} value should be a usage error",
    );
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    let Some((_, rest)) = stderr.split_once("[possible values: ") else {
        panic!("clap should list {flag}'s values, got: {stderr}");
    };
    let Some((list, _)) = rest.split_once(']') else {
        panic!("clap's value list should be closed, got: {stderr}");
    };
    let values: Vec<String> = list.split(", ").map(str::to_string).collect();
    assert_eq!(
        values.len(),
        expected,
        "{flag} accepts {values:?}, not the {expected} this file sweeps; bump the count \
         alongside the new value",
    );
    values
}

/// Run `--stage <stage>` on `redstone-door.crn` without `--edition` and
/// report whether the stage ran (edition-neutral) or was stopped by the
/// missing-`--edition` usage gate (edition-tagged).
///
/// Exit 2 is not taken on trust: a missing fixture exits 2 as well, so
/// the refusal is held to [`assert_missing_edition_usage_error`]. The
/// pipeline passes are silent on this fixture, so the single-line check
/// holds wherever the gate stands among them; whether it stands ahead of
/// a pass that does print is
/// `cli_synth_missing_edition_is_reported_ahead_of_the_sources_findings`'s
/// to pin.
fn stage_is_edition_neutral(stage: &str) -> bool {
    let path = examples_dir().join("redstone-door.crn");
    let out = cairn(
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            stage,
            path.to_str().unwrap(),
        ],
    );
    match out.status.code() {
        Some(0) => true,
        Some(2) => {
            assert_missing_edition_usage_error(&out, stage, "on redstone-door.crn");
            false
        }
        other => panic!(
            "--stage {stage} without --edition exited {other:?}: {}",
            String::from_utf8_lossy(&out.stderr),
        ),
    }
}

/// Assert that `out` is the missing-`--edition` usage error for
/// `--stage <stage>` and nothing else: exit 2, no stdout (no partial IR
/// dump escaped), and one stderr line, naming `--stage <stage>` and
/// `--edition`. `context` says which run it was, after the stage.
fn assert_missing_edition_usage_error(out: &Output, stage: &str, context: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "--stage {stage} {context} without --edition should exit 2, got stderr: {stderr}",
    );
    assert!(
        out.stdout.is_empty(),
        "--stage {stage} {context} must print no IR before the usage gate, got: {}",
        String::from_utf8_lossy(&out.stdout),
    );
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "--stage {stage} {context}: stderr should be the usage error alone, got: {stderr}",
    );
    assert!(
        lines[0].contains(&format!("--stage {stage}")),
        "--stage {stage} {context}: the usage error should name the stage as typed, got: {stderr}",
    );
    assert!(
        lines[0].contains("--edition"),
        "--stage {stage} {context}: the usage error should name --edition, got: {stderr}",
    );
}

/// The `--stage` values [`stage_is_edition_neutral`] finds refused without
/// `--edition`, asked of the binary once per test process and shared by
/// the sweeps over that side of the partition.
fn edition_tagged_stages() -> &'static [String] {
    static TAGGED: OnceLock<Vec<String>> = OnceLock::new();
    TAGGED.get_or_init(|| {
        let tagged: Vec<String> = stage_values()
            .into_iter()
            .filter(|stage| !stage_is_edition_neutral(stage))
            .collect();
        assert!(
            !tagged.is_empty(),
            "no --stage value required --edition, so a sweep over those stages asserts nothing",
        );
        tagged
    })
}

/// Sources that each carry a finding, for the tests that pin a usage
/// refusal ahead of whatever the file has to say: a file name, the text,
/// and the code of the finding the text carries.
///
/// The errors come from different passes, a parse error and a `check`
/// error in a file that parses. The last source carries a warning alone,
/// which does not stop a run.
const FINDING_SOURCES: [(&str, &str, &str); 3] = [
    (
        "parse.crn",
        "@cairn 2026.06\nstruct s size=3x3 size=\n",
        "E_PARSE",
    ),
    (
        "check.crn",
        "@cairn 2026.06\nstruct s size=3x3\n  bogus a=1\n",
        "E_UNKNOWN_KEYWORD",
    ),
    (
        "warning.crn",
        "@cairn 2026.06\nstruct s size=3x3\n  \
         pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "W_LOGIC_UNUSED_SIGNAL",
    ),
];

/// Write each of [`FINDING_SOURCES`] into `dir`, as its name, the path it
/// was written to, and its code.
fn write_finding_sources(dir: &Path) -> Vec<(&'static str, PathBuf, &'static str)> {
    FINDING_SOURCES
        .into_iter()
        .map(|(name, text, code)| (name, write_source(dir, name, text), code))
        .collect()
}

#[test]
fn cli_synth_missing_edition_reports_only_the_usage_error() {
    // The per-stage tests above each pin their own exit code and hint
    // text; what this one pins is that a refused stage printed nothing
    // and said that one line. The fixture has no findings, so whether
    // the gate stands ahead of the passes that could print one is
    // `cli_synth_missing_edition_is_reported_ahead_of_the_sources_findings`'s
    // to pin.
    //
    // Driven off every `--stage` value rather than the edition-tagged
    // ones: which side a stage falls on is the binary's own business
    // (and is pinned there), and running the neutral ones through the
    // same loop says the gate stays out of their way.
    let gated = stage_values()
        .iter()
        .filter(|stage| !stage_is_edition_neutral(stage))
        .count();
    assert!(
        gated > 0,
        "no --stage value required --edition, so this test asserted nothing about the gate",
    );
}

#[test]
fn cli_synth_missing_edition_is_reported_ahead_of_the_sources_findings() {
    // `stage_is_edition_neutral` runs on a fixture with no findings, so an
    // earlier pass has nothing to print there and the ordering it checks
    // holds whichever runs first. Here each source has a finding (see
    // `FINDING_SOURCES`), and the usage error still has to be the one
    // line, with the usage exit code. That holds for the warning-only
    // source too, which does not stop a run: its warning is not printed
    // ahead of the usage error.
    //
    // Each source is also run with `--edition java`, the CONTROL: without
    // the usage error in the way, its finding is what surfaces, with the
    // exit code its severity gives. A source that stopped carrying its
    // finding would still pass the usage-error assertions, and this test
    // would be a copy of `cli_synth_missing_edition_reports_only_the_usage_error`
    // without saying so.
    let dir = tempfile::tempdir().expect("temp dir");
    let sources = write_finding_sources(dir.path());
    for stage in edition_tagged_stages() {
        for (name, path, code) in &sources {
            let out = cairn(
                "synth",
                &[
                    "--experimental-logic-synth",
                    "--stage",
                    stage,
                    path.to_str().unwrap(),
                ],
            );
            assert_missing_edition_usage_error(&out, stage, &format!("on {name}"));

            let out = cairn(
                "synth",
                &[
                    "--experimental-logic-synth",
                    "--stage",
                    stage,
                    "--edition",
                    "java",
                    path.to_str().unwrap(),
                ],
            );
            assert_reports_finding(
                &out,
                code,
                &format!("--stage {stage} --edition java on {name}"),
            );
        }
    }
}

#[test]
fn cli_synth_missing_edition_is_reported_ahead_of_a_missing_file() {
    // A path that names nothing exits 2, as the usage error does, so the
    // exit code cannot tell the two apart and only stderr can. The usage
    // error is decided from argv alone, before the file is read, so it is
    // the one a caller who got both wrong hears first, and the read is
    // never attempted.
    //
    // The CONTROL runs the same path with `--edition java`, where the read
    // is what fails. Without it, the first half would pass just as well on
    // a path that exists.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("missing.crn");
    for stage in edition_tagged_stages() {
        let out = cairn(
            "synth",
            &[
                "--experimental-logic-synth",
                "--stage",
                stage,
                path.to_str().unwrap(),
            ],
        );
        assert_missing_edition_usage_error(&out, stage, "on a missing file");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("cannot read"),
            "--stage {stage} on a missing file without --edition read the file before \
             refusing the flag, got: {stderr}",
        );

        let out = cairn(
            "synth",
            &[
                "--experimental-logic-synth",
                "--stage",
                stage,
                "--edition",
                "java",
                path.to_str().unwrap(),
            ],
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(2),
            "--stage {stage} --edition java on a missing file: {stderr}",
        );
        assert!(
            stderr.contains("cannot read"),
            "--stage {stage} --edition java on a missing file should report the read, \
             got: {stderr}",
        );
    }
}

/// Assert that `out` reported the finding `code` with the exit code its
/// severity gives: `error[E_…]` and exit 1, or `warning[W_…]` and exit 0,
/// since a warning leaves the run its product.
fn assert_reports_finding(out: &Output, code: &str, context: &str) {
    let (severity, exit) = match code.split_once('_') {
        Some(("E", _)) => ("error", 1),
        Some(("W", _)) => ("warning", 0),
        _ => panic!("`{code}` is neither an E_ nor a W_ code"),
    };
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(exit),
        "{context}: a source carrying {code} should exit {exit}, got stderr: {stderr}",
    );
    assert!(
        stderr.contains(&format!("{severity}[{code}]")),
        "{context}: stderr should carry {severity}[{code}], got: {stderr}",
    );
}

#[test]
fn cli_synth_stray_edition_is_refused_on_exactly_the_edition_neutral_stages() {
    // The refusing half of `--edition`'s contract, which
    // `cli_synth_missing_edition_reports_only_the_usage_error` leaves
    // out; why the flag is refused rather than ignored is `synth_edition`'s
    // to say. Walks every `--stage` value against every `--edition`
    // value, so a stage or edition landing later is covered the day it
    // lands.
    //
    // Which side a stage falls on is pinned inside the binary; what this
    // pins from the outside is that the two gates agree about it: a
    // stage refuses `--edition` exactly when it runs clean without one,
    // so no stage is refused both ways or accepted both ways.
    //
    // An accepted run is held to exit 0 only, not to a silent stderr:
    // a warning-severity finding on the fixture would still exit 0 with
    // the flag accepted, and that is a lint's business rather than this
    // gate's.
    let path = examples_dir().join("redstone-door.crn");
    let editions = edition_values();
    let mut refused = 0;
    let mut accepted = 0;
    for stage in stage_values() {
        let neutral = stage_is_edition_neutral(&stage);
        if neutral {
            refused += 1;
        } else {
            accepted += 1;
        }
        for edition in &editions {
            let out = cairn(
                "synth",
                &[
                    "--experimental-logic-synth",
                    "--stage",
                    &stage,
                    "--edition",
                    edition,
                    path.to_str().unwrap(),
                ],
            );
            let stderr = String::from_utf8(out.stderr).expect("utf-8");
            if neutral {
                assert_eq!(
                    out.status.code(),
                    Some(2),
                    "edition-neutral --stage {stage} should refuse --edition {edition}, \
                     got stderr: {stderr}",
                );
                assert!(
                    out.stdout.is_empty(),
                    "--stage {stage} --edition {edition} must print no IR, got: {}",
                    String::from_utf8_lossy(&out.stdout),
                );
                // The message lists the edition-neutral stages by bare
                // name; the stage just refused has to be among them, or
                // the caller is told a set the gate does not enforce.
                assert!(
                    stderr.contains("`--edition`")
                        && stderr.contains("edition-neutral")
                        && stderr.contains(&format!("`{stage}`")),
                    "--stage {stage} --edition {edition} should be refused as a stray flag \
                     naming `{stage}` among the edition-neutral stages, got: {stderr}",
                );
            } else {
                assert_eq!(
                    out.status.code(),
                    Some(0),
                    "edition-tagged --stage {stage} should accept --edition {edition}, \
                     got stderr: {stderr}",
                );
            }
        }
    }
    assert!(
        refused > 0 && accepted > 0,
        "every --stage value fell on one side of the partition ({refused} stages refused \
         --edition, {accepted} accepted it), so this test pinned only half of it",
    );
}
