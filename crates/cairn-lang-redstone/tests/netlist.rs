//! Integration tests for `cairn_lang_redstone::compile_netlist`.
//!
//! Locks the observable behaviours of the Netlist IR slice: the
//! `examples/redstone-door.crn` happy path, the Logic-IR-to-Netlist-IR
//! 1-to-1 rewrite, canonical port ordering per logical cell (including
//! the parser-unreachable `Mux`), the topological invariant carried
//! across from the Logic IR, and empty-scope elision.

use std::path::PathBuf;

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    CellNode, GateKind, GateNode, InputPort, LogicIr, LogicalCell, NetRef, OutputPort, PortName,
    ScopeKind, ScopedLogicIr, ScopedLogicIrEntry, SignalRef, compile_netlist, synthesize,
};

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

fn find_cell_port(cell: &CellNode, port: PortName) -> NetRef {
    cell.drivers
        .iter()
        .find(|d| d.port == port)
        .unwrap_or_else(|| panic!("port {port:?} missing from cell {:?}", cell.cell))
        .net
}

fn sig(name: &str) -> DottedRef {
    let mut parts = name.split('.').map(str::to_owned);
    let head = parts.next().expect("dotted ref has head");
    DottedRef::new(head, parts.collect())
}

/// AC1 — `examples/redstone-door.crn` produces one `OR` cell with two
/// sensor-driven `A`/`B` inputs and one actuator output driven by that
/// cell.
#[test]
fn redstone_door_lowers_to_a_single_or_cell() {
    let source = load_example("redstone-door.crn");
    let synth = synth_source(&source);
    assert!(
        synth
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "clean example must not raise errors: {:?}",
        synth.diagnostics,
    );

    let netlist = compile_netlist(&synth.scoped);
    let entry = netlist
        .scopes
        .iter()
        .find(|e| e.kind == ScopeKind::Struct && e.name == "gatehouse")
        .expect("gatehouse struct produced a Netlist IR entry");
    let ir = &entry.ir;

    let input_names: Vec<String> = ir.inputs.iter().map(|p| p.name.to_string()).collect();
    assert!(
        input_names.contains(&"sig.step".to_string())
            && input_names.contains(&"sig.exit".to_string()),
        "expected sig.step and sig.exit, got {input_names:?}",
    );

    assert_eq!(ir.cells.len(), 1, "one Or cell for `sig.step or sig.exit`");
    assert_eq!(ir.cells[0].cell, LogicalCell::Or);

    let a = find_cell_port(&ir.cells[0], PortName::A);
    let b = find_cell_port(&ir.cells[0], PortName::B);
    assert!(
        matches!(a, NetRef::Input(_)) && matches!(b, NetRef::Input(_)),
        "both Or drivers should be sensor inputs, got {a:?}, {b:?}",
    );

    assert_eq!(
        ir.outputs.len(),
        1,
        "one actuator (door opened_by=sig.open)"
    );
    assert_eq!(ir.outputs[0].name.to_string(), "sig.open");
    assert_eq!(
        ir.outputs[0].driver,
        NetRef::Cell(0),
        "actuator should be driven by the sole Or cell",
    );

    assert!(
        ir.signal_defs
            .get(&sig("sig.open"))
            .is_some_and(|net| matches!(net, NetRef::Cell(0))),
        "signal_defs should map sig.open to Cell(0)",
    );
}

/// AC2 — the Netlist IR keeps a strict 1-to-1 mapping with the source
/// Logic IR (no CSE, no fusion in this pass).
#[test]
fn netlist_preserves_node_count_and_index_mapping() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct sim size=5x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b

  logic sig.and_ab   = sig.a and sig.b
  logic sig.or_ab    = sig.a or sig.b
  logic sig.combined = sig.and_ab and sig.or_ab

  door id=d side=front at=center mat_slot=wall opened_by=sig.combined
";
    let synth = synth_source(source);
    let logic_entry = synth
        .scoped
        .scopes
        .iter()
        .find(|e| e.kind == ScopeKind::Struct && e.name == "sim")
        .expect("sim struct has a Logic IR");
    let logic_len = logic_entry.ir.nodes.len();

    let netlist = compile_netlist(&synth.scoped);
    let entry = netlist
        .scopes
        .iter()
        .find(|e| e.kind == ScopeKind::Struct && e.name == "sim")
        .expect("sim struct has a Netlist IR");
    assert_eq!(
        entry.ir.cells.len(),
        logic_len,
        "netlist cell count must match Logic IR node count",
    );
    assert_eq!(entry.ir.inputs.len(), logic_entry.ir.inputs.len());
    assert_eq!(entry.ir.outputs.len(), logic_entry.ir.outputs.len());
}

/// AC3 — And2 lowers to `LogicalCell::And` with a canonical `[A, B]` port
/// order; Not lowers to `LogicalCell::Not` with a single `[A]` port.
#[test]
fn and_and_not_land_with_canonical_port_order() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct sim size=5x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b

  logic sig.both = sig.a and sig.b
  logic sig.na   = not sig.a

  door id=d side=front at=center mat_slot=wall opened_by=sig.both
  door id=e side=back  at=center mat_slot=wall opened_by=sig.na
";
    let synth = synth_source(source);
    let netlist = compile_netlist(&synth.scoped);
    let entry = &netlist.scopes[0];

    let and_cell = entry
        .ir
        .cells
        .iter()
        .find(|c| c.cell == LogicalCell::And)
        .expect("expected an And cell");
    assert_eq!(
        and_cell.drivers.len(),
        2,
        "And is a two-input cell: {:?}",
        and_cell.drivers,
    );
    assert_eq!(
        and_cell.drivers[0].port,
        PortName::A,
        "first driver must be port A",
    );
    assert_eq!(
        and_cell.drivers[1].port,
        PortName::B,
        "second driver must be port B",
    );

    let not_cell = entry
        .ir
        .cells
        .iter()
        .find(|c| c.cell == LogicalCell::Not)
        .expect("expected a Not cell");
    assert_eq!(
        not_cell.drivers.len(),
        1,
        "Not is a single-input cell: {:?}",
        not_cell.drivers,
    );
    assert_eq!(not_cell.drivers[0].port, PortName::A);
}

/// AC4 — every `NetRef::Cell(j)` in `cells[i]` satisfies `j < i`; the
/// topological invariant survives the rewrite.
#[test]
fn cells_preserve_topological_order() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct sim size=5x5
  floor mat_slot=wall

  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=q at=inside.front  offset=0 y=0 -> sig.b

  logic sig.out = sig.mid and sig.a
  logic sig.mid = sig.a or sig.b

  door id=d side=front at=center mat_slot=wall opened_by=sig.out
";
    let synth = synth_source(source);
    let netlist = compile_netlist(&synth.scoped);
    let entry = &netlist.scopes[0];

    for (i, cell) in entry.ir.cells.iter().enumerate() {
        for driver in &cell.drivers {
            if let NetRef::Cell(j) = driver.net {
                assert!(
                    (j as usize) < i,
                    "cell {i} references Cell({j}) which is not strictly earlier: {cell:?}",
                );
            }
        }
    }
}

/// AC5 — the parser-unreachable `GateKind::Mux` variant lowers with the
/// canonical `[Sel, A, B]` port order when hand-built. Exercises the
/// enum path that a future parser change will make reachable.
#[test]
fn mux_gate_lowers_with_sel_a_b_port_order() {
    let mut ir = LogicIr::new();
    ir.inputs.push(InputPort {
        name: sig("sig.sel"),
        span: 0..0,
    });
    ir.inputs.push(InputPort {
        name: sig("sig.x"),
        span: 0..0,
    });
    ir.inputs.push(InputPort {
        name: sig("sig.y"),
        span: 0..0,
    });
    ir.nodes.push(GateNode {
        kind: GateKind::Mux {
            sel: SignalRef::Input(0),
            a: SignalRef::Input(1),
            b: SignalRef::Input(2),
        },
        span: 0..0,
    });
    ir.outputs.push(OutputPort {
        name: sig("sig.out"),
        driver: SignalRef::Gate(0),
        span: 0..0,
    });

    let mut scoped = ScopedLogicIr::new();
    scoped.scopes.push(ScopedLogicIrEntry {
        kind: ScopeKind::Struct,
        name: "mux_scope".into(),
        ir,
    });

    let netlist = compile_netlist(&scoped);
    let entry = &netlist.scopes[0];
    let cell = &entry.ir.cells[0];

    assert_eq!(cell.cell, LogicalCell::Mux);
    assert_eq!(cell.drivers.len(), 3);
    assert_eq!(cell.drivers[0].port, PortName::Sel);
    assert_eq!(cell.drivers[0].net, NetRef::Input(0));
    assert_eq!(cell.drivers[1].port, PortName::A);
    assert_eq!(cell.drivers[1].net, NetRef::Input(1));
    assert_eq!(cell.drivers[2].port, PortName::B);
    assert_eq!(cell.drivers[2].net, NetRef::Input(2));
}

/// AC6 — scopes whose Logic IR was empty produce no Netlist IR entry.
#[test]
fn empty_logic_scopes_produce_no_netlist_entry() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct plain size=3x3
  floor mat_slot=wall
";
    let synth = synth_source(source);
    assert!(synth.scoped.is_empty(), "sanity: no redstone in the source");
    let netlist = compile_netlist(&synth.scoped);
    assert!(
        netlist.is_empty(),
        "netlist should also be empty when there is no logic",
    );
}

/// AC7 — `signal_defs` is rewritten in place, keyed by the dotted signal
/// name and mapping to the corresponding [`NetRef`]. Locks the JSON
/// shape that the CLI's `synth --stage netlist` will emit.
#[test]
fn signal_defs_rewrites_signal_refs_to_net_refs() {
    let source = load_example("redstone-door.crn");
    let synth = synth_source(&source);
    let netlist = compile_netlist(&synth.scoped);
    let entry = &netlist.scopes[0];

    assert_eq!(
        entry.ir.signal_defs.get(&sig("sig.step")),
        Some(&NetRef::Input(0)).or(Some(&NetRef::Input(1))),
        "sig.step should map to some sensor input",
    );
    assert_eq!(
        entry.ir.signal_defs.get(&sig("sig.open")),
        Some(&NetRef::Cell(0)),
        "sig.open should map to the sole Or cell output",
    );

    let json = serde_json::to_string(&netlist).expect("serialise");
    assert!(
        json.contains("\"sig.open\""),
        "signal_defs should serialise dotted names as JSON string keys, got: {json}",
    );
    assert!(
        json.contains("\"cell\":\"or\""),
        "LogicalCell::Or should serialise as snake_case: {json}",
    );
}
