//! Each primitive of the built-in logic library synthesizes to its one gate, and that gate's cell
//! lays out on both editions, alone and beside another (`spec/redstone` "Two tiers, and the v1
//! boundary", "Edition differences").

use std::fmt::Write as _;

use cairn_lang_core::Edition;
use cairn_lang_redstone::{CellPortDriver, EditionCell, GateKind, NetRef, PortName, SignalRef};

use crate::common::{legalized_from_source, sig, synth_source};

/// The plates a primitive's operands come from, in input order.
const PLATES: [&str; 3] = [
    "at=front.outside offset=0",
    "at=inside.front offset=1",
    "at=front.outside offset=2",
];

/// A struct with a plate driving each of `inputs` and a door that `expression` opens.
fn source(inputs: &[&str], expression: &str) -> String {
    let mut plates = String::new();
    for (i, (input, at)) in inputs.iter().zip(PLATES).enumerate() {
        writeln!(plates, "  pressure_plate id=p{i} {at} y=0 -> {input}")
            .expect("a String never fails to take a write");
    }
    format!(
        "theme t:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n\
         struct s size=20x5\n  floor mat_slot=wall\n  \
         door id=d side=front at=center mat_slot=door\n\
         {plates}  logic sig.out = {expression}\n  door[id=d] opened_by=sig.out\n  \
         circuit region=floor void=2\n"
    )
}

struct Case {
    expression: &'static str,
    inputs: &'static [&'static str],
    gate: GateKind,
    /// The cell and its slowest port's ticks, from the table in `spec/redstone` "Edition
    /// differences".
    java: (EditionCell, u64),
    bedrock: (EditionCell, u64),
}

const A: SignalRef = SignalRef::Input(0);
const B: SignalRef = SignalRef::Input(1);

fn cases() -> [Case; 7] {
    let two = &["sig.a", "sig.b"][..];
    [
        Case {
            expression: "sig.a and sig.b",
            inputs: two,
            gate: GateKind::And2 { a: A, b: B },
            java: (EditionCell::JavaComparatorAnd, 2),
            bedrock: (EditionCell::BedrockTorchAnd, 2),
        },
        Case {
            expression: "sig.a or sig.b",
            inputs: two,
            gate: GateKind::Or2 { a: A, b: B },
            java: (EditionCell::JavaRepeaterOr, 1),
            bedrock: (EditionCell::BedrockTorchOr, 0),
        },
        Case {
            expression: "not sig.a",
            inputs: &["sig.a"],
            gate: GateKind::Not { a: A },
            java: (EditionCell::JavaInverterTorch, 1),
            bedrock: (EditionCell::BedrockInverterTorch, 1),
        },
        Case {
            expression: "sig.a xor sig.b",
            inputs: two,
            gate: GateKind::Xor2 { a: A, b: B },
            java: (EditionCell::JavaTorchXor, 2),
            bedrock: (EditionCell::BedrockTorchXor, 2),
        },
        Case {
            expression: "sig.a nand sig.b",
            inputs: two,
            gate: GateKind::Nand2 { a: A, b: B },
            java: (EditionCell::JavaTorchNand, 1),
            bedrock: (EditionCell::BedrockTorchNand, 1),
        },
        Case {
            expression: "sig.a nor sig.b",
            inputs: two,
            gate: GateKind::Nor2 { a: A, b: B },
            java: (EditionCell::JavaTorchNor, 1),
            bedrock: (EditionCell::BedrockTorchNor, 1),
        },
        Case {
            expression: "mux(b=sig.b, sel=sig.s, a=sig.a)",
            inputs: &["sig.s", "sig.a", "sig.b"],
            gate: GateKind::Mux {
                sel: SignalRef::Input(0),
                a: SignalRef::Input(1),
                b: SignalRef::Input(2),
            },
            java: (EditionCell::JavaTorchMux, 2),
            bedrock: (EditionCell::BedrockTorchMux, 2),
        },
    ]
}

#[test]
fn each_primitive_synthesizes_to_its_one_gate() {
    for case in cases() {
        let out = synth_source(&source(case.inputs, case.expression));
        assert_eq!(out.diagnostics, [], "{}", case.expression);
        let ir = &out.scoped.scopes[0].ir;
        let gates: Vec<GateKind> = ir.nodes.iter().map(|node| node.kind).collect();
        assert_eq!(gates, [case.gate], "{}", case.expression);
        let names: Vec<String> = ir
            .inputs
            .iter()
            .map(|input| input.name.to_string())
            .collect();
        assert_eq!(names, case.inputs, "{}", case.expression);
        assert_eq!(ir.outputs[0].name, sig("sig.out"), "{}", case.expression);
    }
}

#[test]
fn each_primitive_lays_out_as_its_cell_on_both_editions() {
    for case in cases() {
        let source = source(case.inputs, case.expression);
        let ports: &[PortName] = match case.inputs.len() {
            1 => &[PortName::A],
            2 => &[PortName::A, PortName::B],
            _ => &[PortName::Sel, PortName::A, PortName::B],
        };
        let drivers: Vec<CellPortDriver> = ports
            .iter()
            .zip(0..)
            .map(|(&port, input)| CellPortDriver {
                port,
                net: NetRef::Input(input),
            })
            .collect();
        for (edition, (cell, ticks)) in
            [(Edition::Java, case.java), (Edition::Bedrock, case.bedrock)]
        {
            let legalized = legalized_from_source(&source, edition);
            let cells: Vec<(EditionCell, &[CellPortDriver], Option<u64>)> = legalized.scopes[0]
                .ir
                .cells
                .iter()
                .map(|placed| {
                    (
                        placed.cell,
                        placed.drivers.as_slice(),
                        placed.local_delay_ticks(),
                    )
                })
                .collect();
            assert_eq!(
                cells,
                [(cell, drivers.as_slice(), Some(ticks))],
                "{} on {edition:?}",
                case.expression,
            );
        }
    }
}

#[test]
fn only_a_commutative_primitive_shares_its_gate_across_operand_orders() {
    let (s, a, b) = (
        SignalRef::Input(0),
        SignalRef::Input(1),
        SignalRef::Input(2),
    );
    for (written, flipped, gates) in [
        (
            "sig.a xor sig.b",
            "sig.b xor sig.a",
            vec![GateKind::Xor2 { a, b }],
        ),
        (
            "sig.a nand sig.b",
            "sig.b nand sig.a",
            vec![GateKind::Nand2 { a, b }],
        ),
        (
            "sig.a nor sig.b",
            "sig.b nor sig.a",
            vec![GateKind::Nor2 { a, b }],
        ),
        (
            "mux(sel=sig.s, a=sig.a, b=sig.b)",
            "mux(a=sig.a, b=sig.b, sel=sig.s)",
            vec![GateKind::Mux { sel: s, a, b }],
        ),
        (
            "mux(sel=sig.s, a=sig.a, b=sig.b)",
            "mux(sel=sig.s, a=sig.b, b=sig.a)",
            vec![
                GateKind::Mux { sel: s, a, b },
                GateKind::Mux { sel: s, a: b, b: a },
            ],
        ),
    ] {
        let source = format!(
            "struct s size=1x1\n  \
             pressure_plate id=p0 at=front.outside offset=0 y=0 -> sig.s\n  \
             pressure_plate id=p1 at=inside.front offset=0 y=0 -> sig.a\n  \
             pressure_plate id=p2 at=inside.back offset=0 y=0 -> sig.b\n  \
             logic sig.x = {written}\n  logic sig.y = {flipped}\n  \
             door id=front side=front at=center\n  door id=back side=back at=center\n  \
             door[id=front] opened_by=sig.x\n  door[id=back] opened_by=sig.y\n"
        );
        let out = synth_source(&source);
        let found: Vec<GateKind> = out.scoped.scopes[0]
            .ir
            .nodes
            .iter()
            .map(|node| node.kind)
            .collect();
        assert_eq!(found, gates, "{written} / {flipped}");
    }
}

/// A `mux` and an `xor`, each feeding the other, from the plates [`source`] lays.
#[test]
fn a_mux_lays_out_beside_another_cell_on_both_editions() {
    let (s, a, b) = (NetRef::Input(0), NetRef::Input(1), NetRef::Input(2));
    let mux = |sel: NetRef| {
        [(PortName::Sel, sel), (PortName::A, a), (PortName::B, b)]
            .map(|(port, net)| CellPortDriver { port, net })
            .to_vec()
    };
    let xor = |a: NetRef, b: NetRef| {
        [(PortName::A, a), (PortName::B, b)]
            .map(|(port, net)| CellPortDriver { port, net })
            .to_vec()
    };
    for (logic, cells) in [
        (
            "mux(sel=sig.s, a=sig.a, b=sig.b) xor sig.s",
            [(true, mux(s)), (false, xor(s, NetRef::Cell(0)))],
        ),
        (
            "mux(sel=sig.s xor sig.a, a=sig.a, b=sig.b)",
            [(false, xor(s, a)), (true, mux(NetRef::Cell(0)))],
        ),
    ] {
        let source = source(&["sig.s", "sig.a", "sig.b"], logic);
        for (edition, mux_cell, xor_cell) in [
            (
                Edition::Java,
                EditionCell::JavaTorchMux,
                EditionCell::JavaTorchXor,
            ),
            (
                Edition::Bedrock,
                EditionCell::BedrockTorchMux,
                EditionCell::BedrockTorchXor,
            ),
        ] {
            let expected: Vec<(EditionCell, &[CellPortDriver])> = cells
                .iter()
                .map(|(is_mux, drivers)| {
                    (
                        if *is_mux { mux_cell } else { xor_cell },
                        drivers.as_slice(),
                    )
                })
                .collect();
            let legalized = legalized_from_source(&source, edition);
            let laid: Vec<(EditionCell, &[CellPortDriver])> = legalized.scopes[0]
                .ir
                .cells
                .iter()
                .map(|placed| (placed.cell, placed.drivers.as_slice()))
                .collect();
            assert_eq!(laid, expected, "{logic} on {edition:?}");
        }
    }
}
