//! Integration tests for `cairn_lang_redstone::synthesize`.
//!
//! Locks the observable behaviours of the combinational synth slice:
//! happy path over `examples/redstone-door.crn`, unbound / duplicate /
//! cyclic / unused signal diagnostics, nested-body traversal,
//! common-subexpression sharing (including commutativity), gate arity
//! per primitive, topological ordering across out-of-order declarations,
//! and cascade-suppression so a single root cause fires one diagnostic.

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

fn count_code(out: &cairn_lang_redstone::SynthOutput, code: DiagnosticCode) -> usize {
    out.diagnostics.iter().filter(|d| d.code == code).count()
}

#[test]
fn ac1_redstone_door_synthesises_two_inputs_one_or_gate_one_output() {
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
    assert!(
        matches!(ir.nodes[0].kind, GateKind::Or2 { .. }),
        "gate 0 should be Or2, got {:?}",
        ir.nodes[0].kind,
    );

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
    let entry = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "s")
        .expect("scope entry present");
    assert_eq!(entry.ir.nodes.len(), 1, "single OR2 gate after CSE");
    assert!(matches!(entry.ir.nodes[0].kind, GateKind::Or2 { .. }));
}

#[test]
fn commutative_cse_shares_a_gate_when_operand_order_flips() {
    // `sig.a or sig.b` and `sig.b or sig.a` denote the same
    // combinational function; CSE must recognise it or downstream
    // placement pays for a redundant gate.
    let source = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  logic sig.x = sig.a or sig.b
  logic sig.y = sig.b or sig.a
  door id=front side=front at=center
  door[id=front] opened_by=sig.x
  door[id=front] opened_by=sig.y
";
    let out = synth_source(source);
    let entry = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "s")
        .expect("scope entry present");
    assert_eq!(
        entry.ir.nodes.len(),
        1,
        "commutative CSE should collapse to a single OR node, got {:?}",
        entry.ir.nodes,
    );
}

#[test]
fn and_gate_lowers_with_two_inputs() {
    // happy-path coverage for `and`. Also asserts the operand shape
    // — `And2 { a, b }` — so a future refactor that swaps to a
    // vec-based encoding gets caught here.
    let source = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  logic sig.out = sig.a and sig.b
  door id=front side=front at=center
  door[id=front] opened_by=sig.out
";
    let out = synth_source(source);
    let entry = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "s")
        .expect("scope entry present");
    assert_eq!(entry.ir.nodes.len(), 1);
    let GateKind::And2 { a, b } = entry.ir.nodes[0].kind else {
        panic!("expected And2, got {:?}", entry.ir.nodes[0].kind);
    };
    assert!(matches!(a, SignalRef::Input(_)));
    assert!(matches!(b, SignalRef::Input(_)));
}

#[test]
fn not_gate_lowers_with_one_input() {
    // happy-path coverage for `not`. Also asserts the arity-1
    // encoding at the type layer.
    let source = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  logic sig.out = not sig.a
  door id=front side=front at=center
  door[id=front] opened_by=sig.out
";
    let out = synth_source(source);
    let entry = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "s")
        .expect("scope entry present");
    assert_eq!(entry.ir.nodes.len(), 1);
    let GateKind::Not { a } = entry.ir.nodes[0].kind else {
        panic!("expected Not, got {:?}", entry.ir.nodes[0].kind);
    };
    assert!(matches!(a, SignalRef::Input(_)));
}

#[test]
fn topologically_ordered_multi_gate_dag() {
    // `sig.out = sig.mid and sig.tail` where `mid` and `tail` are
    // both driven by earlier `logic` lines. Every gate's operands must
    // reference an earlier index or an input; the DAG is a strict
    // topological order regardless of source declaration order.
    let source = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b
  logic sig.out = sig.mid and sig.tail
  logic sig.mid = sig.a or sig.b
  logic sig.tail = not sig.a
  door id=front side=front at=center
  door[id=front] opened_by=sig.out
";
    let out = synth_source(source);
    let entry = out
        .scoped
        .scopes
        .iter()
        .find(|e| e.name == "s")
        .expect("scope entry present");
    let ir = &entry.ir;
    assert_eq!(ir.nodes.len(), 3, "three gates: OR, NOT, AND");

    for (idx, node) in ir.nodes.iter().enumerate() {
        let this_idx = u32::try_from(idx).expect("small index");
        let mut seen = Vec::new();
        node.kind.each_input(|r| seen.push(r));
        for r in seen {
            match r {
                SignalRef::Input(i) => assert!(
                    (i as usize) < ir.inputs.len(),
                    "input index out of range at gate {idx}",
                ),
                SignalRef::Gate(j) => assert!(
                    j < this_idx,
                    "gate {idx} operand references gate {j} which is not earlier",
                ),
            }
        }
    }
}

#[test]
fn cascade_suppression_reports_single_root_cause_for_shared_unbound() {
    // `logic sig.x = sig.undef` and `door opened_by=sig.undef` both
    // reference the missing signal. After cascade suppression the author
    // sees exactly one E_LOGIC_UNBOUND_SIGNAL naming `sig.undef` — not
    // one per consumer.
    let source = "\
@cairn 2026.06

struct s size=1x1
  logic sig.x = sig.undef
  door id=front side=front at=center
  door[id=front] opened_by=sig.undef
";
    let out = synth_source(source);
    assert_eq!(
        count_code(&out, DiagnosticCode::LogicUnboundSignal),
        1,
        "cascade suppression should collapse to a single finding, got: {:?}",
        out.diagnostics,
    );
}

#[test]
fn actuator_side_unbound_signal_flags_the_driver_arg() {
    // coverage for the actuator-only path — no `logic` line names the
    // signal at all; `opened_by=sig.ghost` should fail loud.
    let source = "\
@cairn 2026.06

struct s size=1x1
  door id=front side=front at=center
  door[id=front] opened_by=sig.ghost
";
    let out = synth_source(source);
    let unbound: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicUnboundSignal)
        .collect();
    assert_eq!(unbound.len(), 1);
    assert!(
        unbound[0].primary.contains("sig.ghost"),
        "expected the signal name in the primary, got: {}",
        unbound[0].primary,
    );
    assert!(
        unbound[0].primary.contains("actuator argument"),
        "primary should distinguish the actuator source, got: {}",
        unbound[0].primary,
    );
}

#[test]
fn sensor_and_logic_lhs_collision_reports_e_logic_multiple_drivers() {
    // a sensor already drives sig.step; a `logic sig.step = ...`
    // line trying to redefine it collides. The finding must fire the same
    // E_LOGIC_MULTIPLE_DRIVERS code with a `sensor emits this signal
    // here` note, not silently drop the `logic` line.
    let source = "\
@cairn 2026.06

struct s size=1x1
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.step
  logic sig.step = sig.step
  door id=front side=front at=center
  door[id=front] opened_by=sig.step
";
    let out = synth_source(source);
    let dups: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicMultipleDrivers)
        .collect();
    assert_eq!(dups.len(), 1);
    assert!(
        dups[0]
            .notes
            .iter()
            .any(|n| n.message == "sensor emits this signal here"),
        "sensor-collision diagnostic should note the sensor source, got: {:?}",
        dups[0].notes,
    );
}

#[test]
fn self_referential_binding_reports_e_logic_cycle() {
    // `logic sig.a = sig.a` — the shortest cycle. Distinct code
    // path from the mutual-recursion test (`resolve_ref` sees `dr` in
    // `in_progress` immediately, without a second `lower_binding` call).
    let source = "\
@cairn 2026.06

struct s size=1x1
  logic sig.a = sig.a
  door id=front side=front at=center
  door[id=front] opened_by=sig.a
";
    let out = synth_source(source);
    assert!(
        count_code(&out, DiagnosticCode::LogicCycle) >= 1,
        "self-reference should be flagged as E_LOGIC_CYCLE, got: {:?}",
        out.diagnostics,
    );
}

#[test]
fn multiple_independent_unbound_refs_in_one_rhs_all_reported() {
    // `sig.undef1 or sig.undef2` — both operands are independent
    // root causes. A short-circuit on the first would leave the second
    // hidden until the author fixed the first and re-ran.
    let source = "\
@cairn 2026.06

struct s size=1x1
  logic sig.out = sig.undef1 or sig.undef2
  door id=front side=front at=center
  door[id=front] opened_by=sig.out
";
    let out = synth_source(source);
    let unbound: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicUnboundSignal)
        .collect();
    assert_eq!(
        unbound.len(),
        2,
        "expected two independent unbound findings, got: {:?}",
        out.diagnostics,
    );
    let primaries: String = unbound.iter().map(|d| d.primary.as_str()).collect();
    assert!(primaries.contains("sig.undef1"), "sig.undef1 must be named");
    assert!(primaries.contains("sig.undef2"), "sig.undef2 must be named");
}
