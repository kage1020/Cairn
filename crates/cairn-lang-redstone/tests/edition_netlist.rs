//! Integration tests for `cairn_lang_redstone::compile_edition_netlist`.
//!
//! Locks the observable behaviours of the Edition Netlist IR slice
//! (`spec/redstone` §14.6 `Logical Cell → Edition Cell → Physical Tile`,
//! second tier): the `examples/redstone-door.crn` happy path, per-edition
//! mapping for every reachable `LogicalCell` (`And` / `Or` / `Not`), the
//! canonical port order carried through from the Netlist IR, the
//! per-edition `*Unpinned` placeholders that stand in for the
//! parser-unreachable cells (`Xor` / `Nand` / `Nor` / `Mux`), the
//! `edition` field on the JSON wire form, and empty-scope elision.

use std::path::PathBuf;

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::check::Severity;
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    EditionCell, EditionCellNode, GateKind, GateNode, InputPort, LogicIr, NetRef, OutputPort,
    PortName, ScopeKind, ScopedLogicIr, ScopedLogicIrEntry, SignalRef, compile_edition_netlist,
    compile_netlist, synthesize,
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

fn sig(name: &str) -> DottedRef {
    let mut parts = name.split('.').map(str::to_owned);
    let head = parts.next().expect("dotted ref has head");
    DottedRef::new(head, parts.collect())
}

fn find_cell_port(cell: &EditionCellNode, port: PortName) -> NetRef {
    cell.drivers
        .iter()
        .find(|d| d.port == port)
        .unwrap_or_else(|| panic!("port {port:?} missing from cell {:?}", cell.cell))
        .net
}

/// AC1 — `examples/redstone-door.crn` compiled for Java tags its lone
/// `Or` cell as `JavaRepeaterOr` and carries the same `[A, B]` driver
/// shape through from the Netlist IR.
#[test]
fn redstone_door_java_maps_or_to_java_repeater_or() {
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
    let edition_netlist = compile_edition_netlist(&netlist, Edition::Java);

    let entry = edition_netlist
        .scopes
        .iter()
        .find(|e| e.kind == ScopeKind::Struct && e.name == "gatehouse")
        .expect("gatehouse scope");
    let ir = &entry.ir;

    assert_eq!(ir.edition, Edition::Java);
    assert_eq!(ir.cells.len(), 1);
    assert_eq!(ir.cells[0].cell, EditionCell::JavaRepeaterOr);

    let a = find_cell_port(&ir.cells[0], PortName::A);
    let b = find_cell_port(&ir.cells[0], PortName::B);
    assert!(
        matches!(a, NetRef::Input(_)) && matches!(b, NetRef::Input(_)),
        "Or drivers should still be sensor inputs, got {a:?}, {b:?}",
    );

    assert_eq!(ir.outputs.len(), 1);
    assert_eq!(ir.outputs[0].name.to_string(), "sig.open");
    assert_eq!(ir.outputs[0].driver, NetRef::Cell(0));

    assert!(
        ir.signal_defs
            .get(&sig("sig.open"))
            .is_some_and(|net| matches!(net, NetRef::Cell(0))),
        "signal_defs should map sig.open to Cell(0)",
    );
}

/// AC2 — the same example compiled for Bedrock picks the Bedrock
/// realisation, `BedrockTorchOr`. The rest of the IR shape (inputs,
/// outputs, driver arity) is edition-independent and stays put.
#[test]
fn redstone_door_bedrock_maps_or_to_bedrock_torch_or() {
    let source = load_example("redstone-door.crn");
    let synth = synth_source(&source);
    let netlist = compile_netlist(&synth.scoped);
    let edition_netlist = compile_edition_netlist(&netlist, Edition::Bedrock);

    let entry = &edition_netlist.scopes[0];
    assert_eq!(entry.ir.edition, Edition::Bedrock);
    assert_eq!(entry.ir.cells.len(), 1);
    assert_eq!(entry.ir.cells[0].cell, EditionCell::BedrockTorchOr);
    assert_eq!(entry.ir.cells[0].drivers.len(), 2);
    assert_eq!(entry.ir.cells[0].drivers[0].port, PortName::A);
    assert_eq!(entry.ir.cells[0].drivers[1].port, PortName::B);
}

/// AC3 — every `NetRef` / `CellPortDriver` from the source Netlist IR is
/// copied verbatim into the Edition Netlist IR: driver arrays, inputs,
/// outputs, and `signal_defs` all round-trip byte-for-byte. Pins the
/// "structural rewrite only" contract.
#[test]
fn edition_pass_copies_nets_verbatim_from_netlist() {
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
    let netlist = compile_netlist(&synth.scoped);
    let edition_netlist = compile_edition_netlist(&netlist, Edition::Java);

    let src = &netlist.scopes[0].ir;
    let dst = &edition_netlist.scopes[0].ir;

    assert_eq!(src.cells.len(), dst.cells.len());
    assert_eq!(src.inputs, dst.inputs);
    assert_eq!(src.outputs, dst.outputs);
    assert_eq!(src.signal_defs, dst.signal_defs);

    for (i, (from, to)) in src.cells.iter().zip(dst.cells.iter()).enumerate() {
        assert_eq!(from.drivers, to.drivers, "drivers of cell[{i}] must match");
        assert_eq!(from.span, to.span, "span of cell[{i}] must match");
    }
}

/// AC4 — the reachable `LogicalCell` variants (`And`, `Or`, `Not`) each
/// pick their Java realisation, and the parser-unreachable cells
/// (`Xor` / `Nand` / `Nor` / `Mux`) fall through to their `Java*Unpinned`
/// placeholders so a future parser expansion cannot silently emit a
/// container-mismatched cell.
#[test]
fn java_mapping_is_exhaustive_over_logical_cells() {
    let two_input =
        |kind: fn(SignalRef, SignalRef) -> GateKind| kind(SignalRef::Input(0), SignalRef::Input(1));
    let and = two_input(|a, b| GateKind::And2 { a, b });
    let or = two_input(|a, b| GateKind::Or2 { a, b });
    let xor = two_input(|a, b| GateKind::Xor2 { a, b });
    let nand = two_input(|a, b| GateKind::Nand2 { a, b });
    let nor = two_input(|a, b| GateKind::Nor2 { a, b });
    let not_gate = GateKind::Not {
        a: SignalRef::Input(0),
    };
    let mux = GateKind::Mux {
        sel: SignalRef::Input(0),
        a: SignalRef::Input(1),
        b: SignalRef::Input(2),
    };

    assert_eq!(
        edition_cell_for(and, Edition::Java),
        EditionCell::JavaComparatorAnd,
    );
    assert_eq!(
        edition_cell_for(or, Edition::Java),
        EditionCell::JavaRepeaterOr,
    );
    assert_eq!(
        edition_cell_for(not_gate, Edition::Java),
        EditionCell::JavaInverterTorch,
    );
    assert_eq!(
        edition_cell_for(xor, Edition::Java),
        EditionCell::JavaXorUnpinned,
    );
    assert_eq!(
        edition_cell_for(nand, Edition::Java),
        EditionCell::JavaNandUnpinned,
    );
    assert_eq!(
        edition_cell_for(nor, Edition::Java),
        EditionCell::JavaNorUnpinned,
    );
    assert_eq!(
        edition_cell_for(mux, Edition::Java),
        EditionCell::JavaMuxUnpinned,
    );
}

/// AC5 — Bedrock realisations pick their own edition-tagged variants for
/// every reachable cell; the unreachable cells fall through to
/// `Bedrock*Unpinned` — the container edition tag is baked into the
/// variant name, so a downstream consumer cannot mistake a Java placeholder
/// for a Bedrock one.
#[test]
fn bedrock_mapping_is_exhaustive_over_logical_cells() {
    let two_input =
        |kind: fn(SignalRef, SignalRef) -> GateKind| kind(SignalRef::Input(0), SignalRef::Input(1));
    let and = two_input(|a, b| GateKind::And2 { a, b });
    let or = two_input(|a, b| GateKind::Or2 { a, b });
    let xor = two_input(|a, b| GateKind::Xor2 { a, b });
    let nand = two_input(|a, b| GateKind::Nand2 { a, b });
    let nor = two_input(|a, b| GateKind::Nor2 { a, b });
    let not_gate = GateKind::Not {
        a: SignalRef::Input(0),
    };
    let mux = GateKind::Mux {
        sel: SignalRef::Input(0),
        a: SignalRef::Input(1),
        b: SignalRef::Input(2),
    };

    assert_eq!(
        edition_cell_for(and, Edition::Bedrock),
        EditionCell::BedrockTorchAnd,
    );
    assert_eq!(
        edition_cell_for(or, Edition::Bedrock),
        EditionCell::BedrockTorchOr,
    );
    assert_eq!(
        edition_cell_for(not_gate, Edition::Bedrock),
        EditionCell::BedrockInverterTorch,
    );
    assert_eq!(
        edition_cell_for(xor, Edition::Bedrock),
        EditionCell::BedrockXorUnpinned,
    );
    assert_eq!(
        edition_cell_for(nand, Edition::Bedrock),
        EditionCell::BedrockNandUnpinned,
    );
    assert_eq!(
        edition_cell_for(nor, Edition::Bedrock),
        EditionCell::BedrockNorUnpinned,
    );
    assert_eq!(
        edition_cell_for(mux, Edition::Bedrock),
        EditionCell::BedrockMuxUnpinned,
    );
}

/// AC6 — scopes whose Netlist IR was empty produce no Edition Netlist IR
/// entry, matching the Logic IR / Netlist IR elision contract.
#[test]
fn empty_netlist_scopes_produce_no_edition_entry() {
    let scoped = cairn_lang_redstone::ScopedNetlistIr::new();
    let out = compile_edition_netlist(&scoped, Edition::Java);
    assert!(out.is_empty());
    assert!(out.scopes.is_empty());
}

/// AC7 — the JSON dump carries an `edition` key on every scope entry
/// (canonical lowercase name, `"java"` / `"bedrock"`) and the `cell`
/// values render as `snake_case` so downstream tooling has one stable
/// vocabulary to key off.
#[test]
fn json_dump_carries_edition_and_snake_case_cells() {
    let source = load_example("redstone-door.crn");
    let synth = synth_source(&source);
    let netlist = compile_netlist(&synth.scoped);
    let edition_netlist = compile_edition_netlist(&netlist, Edition::Java);

    let json = serde_json::to_string(&edition_netlist).expect("serialise");
    assert!(
        json.contains("\"edition\":\"java\""),
        "Java edition should render as \"java\": {json}",
    );
    assert!(
        json.contains("\"cell\":\"java_repeater_or\""),
        "EditionCell::JavaRepeaterOr should serialise as snake_case: {json}",
    );

    let bedrock = compile_edition_netlist(&netlist, Edition::Bedrock);
    let json = serde_json::to_string(&bedrock).expect("serialise");
    assert!(
        json.contains("\"edition\":\"bedrock\""),
        "Bedrock edition should render as \"bedrock\": {json}",
    );
    assert!(
        json.contains("\"cell\":\"bedrock_torch_or\""),
        "EditionCell::BedrockTorchOr should serialise as snake_case: {json}",
    );
}

/// Two non-empty scopes are compiled independently — their `edition`
/// tags all match the pass argument and their per-scope contents do not
/// bleed across.
#[test]
fn multiple_scopes_compile_independently() {
    let source = r"
theme t:
  slot wall -> @oak_planks

struct alpha size=5x5
  floor mat_slot=wall
  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
  logic sig.na = not sig.a
  door id=d side=front at=center mat_slot=wall opened_by=sig.na

struct beta size=5x5
  floor mat_slot=wall
  pressure_plate id=q at=front.outside offset=0 y=0 -> sig.b1
  pressure_plate id=r at=inside.front  offset=0 y=0 -> sig.b2
  logic sig.both = sig.b1 and sig.b2
  door id=e side=front at=center mat_slot=wall opened_by=sig.both
";
    let synth = synth_source(source);
    let netlist = compile_netlist(&synth.scoped);
    let out = compile_edition_netlist(&netlist, Edition::Bedrock);

    assert_eq!(out.scopes.len(), 2);
    let alpha = out.scopes.iter().find(|e| e.name == "alpha").unwrap();
    assert_eq!(alpha.ir.edition, Edition::Bedrock);
    assert_eq!(alpha.ir.cells[0].cell, EditionCell::BedrockInverterTorch);

    let beta = out.scopes.iter().find(|e| e.name == "beta").unwrap();
    assert_eq!(beta.ir.edition, Edition::Bedrock);
    assert_eq!(beta.ir.cells[0].cell, EditionCell::BedrockTorchAnd);

    assert!(alpha.ir.signal_defs.get(&sig("sig.b1")).is_none());
    assert!(beta.ir.signal_defs.get(&sig("sig.b1")).is_some());
}

/// The Java and Bedrock runs of the same Netlist IR differ *only* in
/// the container's `edition` field and each cell's `cell` tag — every
/// other structural byte (inputs, outputs, driver arity + port order,
/// span, `signal_defs`) is edition-independent by contract, and this
/// test locks that so a future edition pass that accidentally reorders
/// or drops non-cell state (or forgets to copy `signal_defs`) fails
/// loud instead of silently drifting.
#[test]
fn java_and_bedrock_edition_netlists_differ_only_in_edition_and_cell_tags() {
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

    let java = compile_edition_netlist(&netlist, Edition::Java);
    let bedrock = compile_edition_netlist(&netlist, Edition::Bedrock);

    assert_eq!(java.scopes.len(), bedrock.scopes.len());
    for (j, b) in java.scopes.iter().zip(bedrock.scopes.iter()) {
        assert_eq!(j.kind, b.kind);
        assert_eq!(j.name, b.name);
        assert_eq!(j.ir.inputs, b.ir.inputs);
        assert_eq!(j.ir.outputs, b.ir.outputs);
        assert_eq!(j.ir.signal_defs, b.ir.signal_defs);
        assert_ne!(j.ir.edition, b.ir.edition);
        assert_eq!(j.ir.edition, Edition::Java);
        assert_eq!(b.ir.edition, Edition::Bedrock);
        assert_eq!(j.ir.cells.len(), b.ir.cells.len());
        for (jc, bc) in j.ir.cells.iter().zip(b.ir.cells.iter()) {
            assert_eq!(jc.drivers, bc.drivers);
            assert_eq!(jc.span, bc.span);
            assert_ne!(jc.cell, bc.cell, "cell tag must differ per edition");
            assert_eq!(jc.cell.edition(), Edition::Java);
            assert_eq!(bc.cell.edition(), Edition::Bedrock);
        }
    }
}

/// The Netlist IR's `outputs` list can carry more than one entry — a
/// scope with two actuators. Locks that every entry, not just the first,
/// is copied verbatim (the `.clone_from` on `out.outputs` in `compile_scope`
/// is a single call today, so this test would fail loud if that were
/// reduced to a `take(1)` mistake in a later refactor).
#[test]
fn compile_edition_netlist_copies_all_outputs_verbatim() {
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
    let edition = compile_edition_netlist(&netlist, Edition::Java);

    let src_outputs = &netlist.scopes[0].ir.outputs;
    let dst_outputs = &edition.scopes[0].ir.outputs;
    assert!(
        src_outputs.len() >= 2,
        "sanity: fixture wires two actuators"
    );
    assert_eq!(src_outputs, dst_outputs);
}

/// The `*Unpinned` placeholders serialise as their `snake_case` name —
/// no `serde` attribute drift can silently make a downstream consumer
/// think a `JavaXor` cell landed when the pass emitted a placeholder.
#[test]
fn unpinned_variants_serialise_as_snake_case() {
    let mut logic = LogicIr::new();
    logic.inputs.push(InputPort {
        name: sig("sig.a"),
        span: 0..0,
    });
    logic.inputs.push(InputPort {
        name: sig("sig.b"),
        span: 0..0,
    });
    logic.nodes.push(GateNode {
        kind: GateKind::Xor2 {
            a: SignalRef::Input(0),
            b: SignalRef::Input(1),
        },
        span: 0..0,
    });
    logic.outputs.push(OutputPort {
        name: sig("sig.out"),
        driver: SignalRef::Gate(0),
        span: 0..0,
    });

    let mut scoped = ScopedLogicIr::new();
    scoped.scopes.push(ScopedLogicIrEntry {
        kind: ScopeKind::Struct,
        name: "hand".into(),
        ir: logic,
    });

    let netlist = compile_netlist(&scoped);
    let java = compile_edition_netlist(&netlist, Edition::Java);
    let json = serde_json::to_string(&java).expect("serialise");
    assert!(
        json.contains("\"cell\":\"java_xor_unpinned\""),
        "Java XOR should serialise as its unpinned placeholder name: {json}",
    );

    let bedrock = compile_edition_netlist(&netlist, Edition::Bedrock);
    let json = serde_json::to_string(&bedrock).expect("serialise");
    assert!(
        json.contains("\"cell\":\"bedrock_xor_unpinned\""),
        "Bedrock XOR should serialise as its unpinned placeholder name: {json}",
    );
}

/// Build a single-gate Netlist IR around `kind`, run
/// `compile_edition_netlist` for `edition`, and return the resulting cell
/// tag. Lets AC4 / AC5 hit the `*Unpinned` variants without going through
/// the surface parser.
fn edition_cell_for(kind: GateKind, edition: Edition) -> EditionCell {
    let mut ir = LogicIr::new();
    let mut input_count = 0u32;
    kind.each_input(|_| input_count += 1);
    for i in 0..input_count {
        ir.inputs.push(InputPort {
            name: sig(&format!("sig.a{i}")),
            span: 0..0,
        });
    }
    ir.nodes.push(GateNode { kind, span: 0..0 });
    ir.outputs.push(OutputPort {
        name: sig("sig.out"),
        driver: SignalRef::Gate(0),
        span: 0..0,
    });

    let mut scoped = ScopedLogicIr::new();
    scoped.scopes.push(ScopedLogicIrEntry {
        kind: ScopeKind::Struct,
        name: "hand".into(),
        ir,
    });

    let netlist = compile_netlist(&scoped);
    let edition_netlist = compile_edition_netlist(&netlist, edition);
    edition_netlist.scopes[0].ir.cells[0].cell
}
