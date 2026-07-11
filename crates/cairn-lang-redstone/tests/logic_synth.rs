//! Integration tests for `cairn_lang_redstone::synthesize`.
//!
//! Locks the AC-1..AC-7 behaviours of the M6-PR1 slice: happy path over
//! `examples/redstone-door.crn`, unbound / duplicate / cyclic / unused
//! signal diagnostics, nested-body traversal, and common-subexpression
//! sharing.

use std::path::PathBuf;

use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{DiagnosticCode, GateKind, ScopeKind, SignalRef, synthesize};

/// Load `examples/<name>` relative to the workspace root.
fn load_example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn synth_source(source: &str) -> cairn_lang_redstone::SynthOutput {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    synthesize(&intent)
}

#[test]
fn ac1_redstone_door_synthesises_two_inputs_one_or_gate_one_output() {
    // Happy path: the canonical `redstone-door.crn` example lowers to a
    // single OR gate wired between two sensor inputs and one actuator
    // output. This locks the shape of the DAG so a follow-up refactor
    // that reorders signal collection cannot silently drop a port.
    let source = load_example("redstone-door.crn");
    let out = synth_source(&source);

    assert!(
        out.diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "clean example must not raise errors: {:?}",
        out.diagnostics,
    );

    let entry = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.kind == ScopeKind::Struct && e.name == "gatehouse")
        .expect("gatehouse struct produced a Logic IR entry");
    let ir = &entry.ir;

    assert_eq!(ir.inputs.len(), 2, "sig.step + sig.exit → 2 input ports");
    let input_names: Vec<String> = ir.inputs.iter().map(|p| p.name.to_string()).collect();
    assert!(
        input_names.contains(&"sig.step".to_string())
            && input_names.contains(&"sig.exit".to_string()),
        "expected sig.step and sig.exit, got {input_names:?}",
    );

    assert_eq!(
        ir.outputs.len(),
        1,
        "one actuator (`door opened_by=sig.open`)"
    );
    assert_eq!(ir.outputs[0].name.to_string(), "sig.open");

    assert_eq!(ir.nodes.len(), 1, "one combinational OR gate");
    assert_eq!(ir.nodes[0].kind, GateKind::Or2);

    // Actuator's driver must be the OR gate.
    let SignalRef::Gate(driver_idx) = ir.outputs[0].driver else {
        panic!(
            "actuator driver should be a gate, got {:?}",
            ir.outputs[0].driver
        );
    };
    assert_eq!(driver_idx, 0);
}

#[test]
fn ac2_unbound_signal_reports_e_logic_unbound_signal() {
    // A `logic` binding whose RHS names a signal that no sensor emits and
    // no other `logic` line defines must fail-loud rather than silently
    // wire the actuator to air.
    let source = "\
@cairn 2026.06

struct s size=1x1
  door id=front side=front at=center
  logic sig.open = sig.undefined
  door[id=front] opened_by=sig.open
";
    let out = synth_source(source);
    let unbound: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicUnboundSignal)
        .collect();
    assert_eq!(
        unbound.len(),
        1,
        "expected exactly one E_LOGIC_UNBOUND_SIGNAL, got: {:?}",
        out.diagnostics,
    );
    assert!(
        unbound[0].primary.contains("sig.undefined"),
        "diagnostic should name the missing signal, got: {}",
        unbound[0].primary,
    );
}

#[test]
fn ac3_multiple_drivers_report_e_logic_multiple_drivers() {
    // Two `logic sig.X = ...` lines with the same LHS is a fail-loud
    // ambiguity — a downstream reference cannot pick without dropping
    // one silently.
    let source = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  logic sig.dup = sig.a
  logic sig.dup = sig.b
  door id=front side=front at=center
  door[id=front] opened_by=sig.dup
";
    let out = synth_source(source);
    let duplicates: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicMultipleDrivers)
        .collect();
    assert_eq!(
        duplicates.len(),
        1,
        "expected one E_LOGIC_MULTIPLE_DRIVERS, got: {:?}",
        out.diagnostics,
    );
    assert!(
        duplicates[0]
            .notes
            .iter()
            .any(|n| n.message == "first declared here"),
        "duplicate diagnostic should include a `first declared here` note",
    );
}

#[test]
fn ac4_cyclic_bindings_report_e_logic_cycle() {
    // `logic sig.a = sig.b` and `logic sig.b = sig.a` form a
    // combinational cycle. The synth pass rejects it — a latch macro
    // (out of scope for M6-PR1) would be required.
    let source = "\
@cairn 2026.06

struct s size=1x1
  logic sig.a = sig.b
  logic sig.b = sig.a
  door id=front side=front at=center
  door[id=front] opened_by=sig.a
";
    let out = synth_source(source);
    let cycles: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicCycle)
        .collect();
    assert!(
        !cycles.is_empty(),
        "expected at least one E_LOGIC_CYCLE, got: {:?}",
        out.diagnostics,
    );
}

#[test]
fn ac5_unused_logic_binding_warns_and_still_synthesises() {
    // An unreachable `logic` line is a warning, not an error. The scope
    // still produces a Logic IR entry so the reachable half round-trips.
    let source = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.step
  logic sig.dead = sig.step
  door id=front side=front at=center
  door[id=front] opened_by=sig.step
";
    let out = synth_source(source);
    let warns: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicUnusedSignal)
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected one W_LOGIC_UNUSED_SIGNAL, got: {:?}",
        out.diagnostics,
    );

    let has_error = out
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error);
    assert!(!has_error, "warning must not block IR construction");

    assert!(
        out.scoped.scopes.iter().any(|e| e.name == "s"),
        "the scope should still round-trip a Logic IR when only warnings fire",
    );
}

#[test]
fn ac6_nested_level_body_collects_signals() {
    // Sensors, actuators, and `logic` lines nested inside a `level y=0`
    // block are recursively collected, so a struct that splits its
    // circuit across levels still lowers.
    let source = "\
@cairn 2026.06

struct s size=1x1
  level y=0
    pressure_plate id=p at=front.outside offset=0 y=0 -> sig.step
    logic sig.open = sig.step
    door id=front side=front at=center
    door[id=front] opened_by=sig.open
";
    let out = synth_source(source);
    let entry = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "s")
        .expect("nested body still produces a scope entry");
    assert_eq!(entry.ir.inputs.len(), 1);
    assert_eq!(entry.ir.outputs.len(), 1);
    assert!(
        out.diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "nested lowering must not raise errors: {:?}",
        out.diagnostics,
    );
}

#[test]
fn ac7_common_subexpressions_share_a_single_gate() {
    // Two `logic` lines whose RHS builds the same `sig.a or sig.b` collapse
    // to one OR gate. CSE prevents the follow-up placement PR from paying
    // for structurally redundant fanout the source never asked for.
    let source = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  logic sig.x = sig.a or sig.b
  logic sig.y = sig.a or sig.b
  door id=front side=front at=center
  door[id=front] opened_by=sig.x
  door[id=front] opened_by=sig.y
";
    let out = synth_source(source);
    // Both LHS names should map to the same underlying gate (CSE).
    let entry = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "s")
        .expect("scope entry present");
    assert_eq!(entry.ir.nodes.len(), 1, "single OR2 gate after CSE");
    assert_eq!(entry.ir.nodes[0].kind, GateKind::Or2);
}
