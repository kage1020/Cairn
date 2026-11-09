//! Each primitive of the built-in logic library synthesizes to its one gate, and that gate's cell
//! lays out on both editions, alone and beside another (`spec/redstone` "Two tiers, and the v1
//! boundary", "Edition differences").

use std::fmt::Write as _;

use cairn_lang_core::Edition;
use cairn_lang_redstone::{CellPortDriver, EditionCell, GateKind, NetRef, PortName, SignalRef};

use crate::common::{cells, legalized, sig, synth_source};

/// The plates a primitive's operands come from, in input order.
const PLATES: [&str; 3] = [
    "at=inside.back offset=1",
    "at=inside.back offset=9",
    "at=inside.back offset=17",
];

/// A struct with a plate driving each of `inputs` and a door that `expression` opens.
fn source(inputs: &[&str], expression: &str) -> String {
    sized_source("20x13", inputs, expression)
}

/// [`source`] in a struct of `size`.
fn sized_source(size: &str, inputs: &[&str], expression: &str) -> String {
    let mut plates = String::new();
    for (i, (input, at)) in inputs.iter().zip(PLATES).enumerate() {
        writeln!(plates, "  pressure_plate id=p{i} {at} y=0 -> {input}")
            .expect("a String never fails to take a write");
    }
    format!(
        "theme t:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n\
         struct s size={size}\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         door id=d side=front at=center mat_slot=door\n\
         {plates}  logic sig.out = {expression}\n  door[id=d] opened_by=sig.out\n  \
         circuit region=floor void=4\n"
    )
}

struct Case {
    expression: &'static str,
    inputs: &'static [&'static str],
    gate: GateKind,
    java: EditionCell,
    bedrock: EditionCell,
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
            java: EditionCell::JavaComparatorAnd,
            bedrock: EditionCell::BedrockTorchAnd,
        },
        Case {
            expression: "sig.a or sig.b",
            inputs: two,
            gate: GateKind::Or2 { a: A, b: B },
            java: EditionCell::JavaRepeaterOr,
            bedrock: EditionCell::BedrockRepeaterOr,
        },
        Case {
            expression: "not sig.a",
            inputs: &["sig.a"],
            gate: GateKind::Not { a: A },
            java: EditionCell::JavaInverterTorch,
            bedrock: EditionCell::BedrockInverterTorch,
        },
        Case {
            expression: "sig.a xor sig.b",
            inputs: two,
            gate: GateKind::Xor2 { a: A, b: B },
            java: EditionCell::JavaTorchXor,
            bedrock: EditionCell::BedrockTorchXor,
        },
        Case {
            expression: "sig.a nand sig.b",
            inputs: two,
            gate: GateKind::Nand2 { a: A, b: B },
            java: EditionCell::JavaTorchNand,
            bedrock: EditionCell::BedrockTorchNand,
        },
        Case {
            expression: "sig.a nor sig.b",
            inputs: two,
            gate: GateKind::Nor2 { a: A, b: B },
            java: EditionCell::JavaTorchNor,
            bedrock: EditionCell::BedrockTorchNor,
        },
        Case {
            expression: "mux(b=sig.b, sel=sig.s, a=sig.a)",
            inputs: &["sig.s", "sig.a", "sig.b"],
            gate: GateKind::Mux {
                sel: SignalRef::Input(0),
                a: SignalRef::Input(1),
                b: SignalRef::Input(2),
            },
            java: EditionCell::JavaTorchMux,
            bedrock: EditionCell::BedrockTorchMux,
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
        for (edition, cell) in [(Edition::Java, case.java), (Edition::Bedrock, case.bedrock)] {
            assert_eq!(
                cells(&legalized(&source, edition).scopes[0].ir),
                [(cell, drivers.clone())],
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

/// A `mux` and an `xor`, each feeding the other, from the plates [`sized_source`] lays.
#[test]
fn a_mux_lays_out_beside_another_cell_on_both_editions() {
    use PortName::{A, B, Sel};
    let (s, a, b, gate) = (
        NetRef::Input(0),
        NetRef::Input(1),
        NetRef::Input(2),
        NetRef::Cell(0),
    );
    let drivers = |ports: &[(PortName, NetRef)]| -> Vec<CellPortDriver> {
        ports
            .iter()
            .map(|&(port, net)| CellPortDriver { port, net })
            .collect()
    };
    // A tile lists its inputs in net order: the plates' nets, then the cells'.
    for (logic, wanted) in [
        (
            "mux(sel=sig.s, a=sig.a, b=sig.b) xor sig.s",
            [
                (true, drivers(&[(Sel, s), (A, a), (B, b)])),
                (false, drivers(&[(A, s), (B, gate)])),
            ],
        ),
        (
            "mux(sel=sig.s xor sig.a, a=sig.a, b=sig.b)",
            [
                (false, drivers(&[(A, s), (B, a)])),
                (true, drivers(&[(A, a), (B, b), (Sel, gate)])),
            ],
        ),
    ] {
        // Two torch tiles do not wire on the 20x13 floor the other cases share.
        let source = sized_source("24x13", &["sig.s", "sig.a", "sig.b"], logic);
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
            let expected: Vec<(EditionCell, Vec<CellPortDriver>)> = wanted
                .iter()
                .map(|(is_mux, drivers)| {
                    (if *is_mux { mux_cell } else { xor_cell }, drivers.clone())
                })
                .collect();
            let laid = cells(&legalized(&source, edition).scopes[0].ir);
            assert_eq!(laid, expected, "{logic} on {edition:?}");
        }
    }
}
