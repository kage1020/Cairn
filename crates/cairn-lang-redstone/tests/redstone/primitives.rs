//! Each primitive of the built-in logic library synthesizes to one gate that computes its truth
//! table, and that gate's cell lays out on both editions (`spec/redstone` "Two tiers, and the v1
//! boundary", "Edition differences").

use std::fmt::Write as _;

use cairn_lang_core::Edition;
use cairn_lang_redstone::{
    CellPortDriver, EditionCell, GateKind, LogicIr, NetRef, PortName, SignalRef,
};

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

/// `signal`'s value in `ir` while each input is on as `inputs` says, by input index.
fn evaluate(ir: &LogicIr, inputs: &[bool], signal: SignalRef) -> bool {
    let at = |index: u32| usize::try_from(index).expect("an index fits in usize");
    let value = |signal| evaluate(ir, inputs, signal);
    match signal {
        SignalRef::Input(index) => inputs[at(index)],
        SignalRef::Gate(index) => match ir.nodes[at(index)].kind {
            GateKind::And2 { a, b } => value(a) && value(b),
            GateKind::Or2 { a, b } => value(a) || value(b),
            GateKind::Not { a } => !value(a),
            GateKind::Xor2 { a, b } => value(a) != value(b),
            GateKind::Nand2 { a, b } => !(value(a) && value(b)),
            GateKind::Nor2 { a, b } => !(value(a) || value(b)),
            GateKind::Mux { sel, a, b } => {
                if value(sel) {
                    value(b)
                } else {
                    value(a)
                }
            }
            other => panic!("no truth table for {other:?}"),
        },
    }
}

struct Case {
    expression: &'static str,
    inputs: &'static [&'static str],
    /// The output for each row, the first input the most significant bit.
    truth: &'static [u8],
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
            truth: &[0, 0, 0, 1],
            gate: GateKind::And2 { a: A, b: B },
            java: EditionCell::JavaComparatorAnd,
            bedrock: EditionCell::BedrockTorchAnd,
        },
        Case {
            expression: "sig.a or sig.b",
            inputs: two,
            truth: &[0, 1, 1, 1],
            gate: GateKind::Or2 { a: A, b: B },
            java: EditionCell::JavaRepeaterOr,
            bedrock: EditionCell::BedrockTorchOr,
        },
        Case {
            expression: "not sig.a",
            inputs: &["sig.a"],
            truth: &[1, 0],
            gate: GateKind::Not { a: A },
            java: EditionCell::JavaInverterTorch,
            bedrock: EditionCell::BedrockInverterTorch,
        },
        Case {
            expression: "sig.a xor sig.b",
            inputs: two,
            truth: &[0, 1, 1, 0],
            gate: GateKind::Xor2 { a: A, b: B },
            java: EditionCell::JavaTorchXor,
            bedrock: EditionCell::BedrockTorchXor,
        },
        Case {
            expression: "sig.a nand sig.b",
            inputs: two,
            truth: &[1, 1, 1, 0],
            gate: GateKind::Nand2 { a: A, b: B },
            java: EditionCell::JavaTorchNand,
            bedrock: EditionCell::BedrockTorchNand,
        },
        Case {
            expression: "sig.a nor sig.b",
            inputs: two,
            truth: &[1, 0, 0, 0],
            gate: GateKind::Nor2 { a: A, b: B },
            java: EditionCell::JavaTorchNor,
            bedrock: EditionCell::BedrockTorchNor,
        },
        Case {
            expression: "mux(b=sig.b, sel=sig.s, a=sig.a)",
            inputs: &["sig.s", "sig.a", "sig.b"],
            truth: &[0, 0, 1, 1, 0, 1, 0, 1],
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
fn each_primitive_synthesizes_to_one_gate_that_computes_its_truth_table() {
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

        let width = case.inputs.len();
        let computed: Vec<u8> = (0..1_usize << width)
            .map(|row| {
                let inputs: Vec<bool> = (0..width)
                    .map(|bit| row >> (width - 1 - bit) & 1 == 1)
                    .collect();
                u8::from(evaluate(ir, &inputs, ir.outputs[0].driver))
            })
            .collect();
        assert_eq!(computed, case.truth, "{}", case.expression);
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
            let legalized = legalized_from_source(&source, edition);
            let cells: Vec<(EditionCell, &[CellPortDriver])> = legalized.scopes[0]
                .ir
                .cells
                .iter()
                .map(|placed| (placed.cell, placed.drivers.as_slice()))
                .collect();
            assert_eq!(
                cells,
                [(cell, drivers.as_slice())],
                "{} on {edition:?}",
                case.expression,
            );
        }
    }
}

#[test]
fn a_commutative_primitive_shares_its_gate_whatever_order_its_operands_are_written_in() {
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
