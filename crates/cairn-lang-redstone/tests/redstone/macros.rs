//! Each sequential macro synthesizes to one gate carrying its stage count, which lowers to one
//! cell on each edition and keeps its feedback inside it (`spec/redstone` "Time model", "Edition
//! differences").

use std::fmt::Write as _;

use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::{
    CellPortDriver, EditionCell, GateKind, LogicalCell, NetRef, PortName, SignalRef,
    compile_netlist, verify_latency,
};

use crate::common::{cells, legalized, synth_source};

/// A struct with a lever on each of `inputs` and a door that `expression` opens, then `tail`.
fn source(inputs: &[&str], expression: &str, tail: &str) -> String {
    let mut levers = String::new();
    for (input, offset) in inputs.iter().zip((2..).step_by(4)) {
        writeln!(
            levers,
            "  lever id=l{offset} at=inside.back offset={offset} y=0 -> {input}"
        )
        .expect("writing to a String");
    }
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct s size=21x15\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door id=d side=front at=center\n\
         {levers}  logic sig.out = {expression}\n  door[id=d] opened_by=sig.out\n  \
         circuit region=floor void=4\n{tail}"
    )
}

struct Case {
    expression: &'static str,
    inputs: &'static [&'static str],
    gate: GateKind,
    logical: LogicalCell,
    java: EditionCell,
    bedrock: EditionCell,
}

const A: SignalRef = SignalRef::Input(0);

fn cases() -> [Case; 6] {
    let one = &["sig.a"][..];
    [
        Case {
            expression: "delay(sig.a, 3)",
            inputs: one,
            gate: GateKind::Delay { a: A, stages: 3 },
            logical: LogicalCell::Delay { stages: 3 },
            java: EditionCell::JavaRepeaterDelay { stages: 3 },
            bedrock: EditionCell::BedrockRepeaterDelay { stages: 3 },
        },
        Case {
            expression: "pulse(sig.a, 4)",
            inputs: one,
            gate: GateKind::Pulse { a: A, stages: 4 },
            logical: LogicalCell::Pulse { stages: 4 },
            java: EditionCell::JavaTorchPulse { stages: 4 },
            bedrock: EditionCell::BedrockTorchPulse { stages: 4 },
        },
        Case {
            expression: "edge_rising(sig.a)",
            inputs: one,
            gate: GateKind::EdgeRising { a: A },
            logical: LogicalCell::EdgeRising,
            java: EditionCell::JavaTorchEdgeRising,
            bedrock: EditionCell::BedrockTorchEdgeRising,
        },
        Case {
            expression: "edge_falling(sig.a)",
            inputs: one,
            gate: GateKind::EdgeFalling { a: A },
            logical: LogicalCell::EdgeFalling,
            java: EditionCell::JavaTorchEdgeFalling,
            bedrock: EditionCell::BedrockTorchEdgeFalling,
        },
        Case {
            expression: "latch(reset=sig.r, set=sig.s)",
            inputs: &["sig.s", "sig.r"],
            gate: GateKind::Latch {
                set: SignalRef::Input(0),
                reset: SignalRef::Input(1),
            },
            logical: LogicalCell::Latch,
            java: EditionCell::JavaTorchLatch,
            bedrock: EditionCell::BedrockTorchLatch,
        },
        Case {
            expression: "counter(sig.a, 2)",
            inputs: one,
            gate: GateKind::Counter { a: A, stages: 2 },
            logical: LogicalCell::Counter { stages: 2 },
            java: EditionCell::JavaRepeaterCounter { stages: 2 },
            bedrock: EditionCell::BedrockRepeaterCounter { stages: 2 },
        },
    ]
}

#[test]
fn each_macro_is_one_gate_and_one_cell_on_each_edition() {
    for case in cases() {
        let src = source(case.inputs, case.expression, "");
        let out = synth_source(&src);
        assert_eq!(out.diagnostics, [], "{}", case.expression);
        let gates: Vec<GateKind> = out.scoped.scopes[0]
            .ir
            .nodes
            .iter()
            .map(|node| node.kind)
            .collect();
        assert_eq!(gates, [case.gate], "{}", case.expression);

        let netlist = compile_netlist(&out.scoped);
        let logical: Vec<LogicalCell> = netlist.scopes[0]
            .ir
            .cells
            .iter()
            .map(|cell| cell.cell)
            .collect();
        assert_eq!(logical, [case.logical], "{}", case.expression);

        let ports: &[PortName] = if case.inputs.len() == 2 {
            &[PortName::Set, PortName::Reset]
        } else {
            &[PortName::A]
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
                cells(&legalized(&src, edition).scopes[0].ir),
                [(cell, drivers.clone())],
                "{} on {edition:?}",
                case.expression,
            );
        }
    }
}

#[test]
fn a_macro_shares_its_gate_only_with_one_of_the_same_operand_and_stage_count() {
    let tail = "  logic sig.same = delay(sig.a, 2)\n  logic sig.more = delay(sig.a, 3)\n  \
                logic sig.both = sig.same and sig.more\n  lamp id=lp at=inside.back offset=5 \
                y=1 lit_by=sig.both\n";
    let src = source(&["sig.a"], "delay(sig.a, 2)", tail);
    let out = synth_source(&src);
    assert_eq!(out.diagnostics, [], "{src}");
    let gates: Vec<GateKind> = out.scoped.scopes[0]
        .ir
        .nodes
        .iter()
        .map(|node| node.kind)
        .collect();
    assert_eq!(
        gates,
        [
            GateKind::Delay { a: A, stages: 2 },
            GateKind::Delay { a: A, stages: 3 },
            GateKind::And2 {
                a: SignalRef::Gate(0),
                b: SignalRef::Gate(1),
            },
        ],
    );
}

#[test]
fn a_signal_read_back_through_a_macro_is_still_a_cycle() {
    let src = source(&["sig.a"], "latch(set=sig.a, reset=sig.out)", "");
    let out = synth_source(&src);
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["E_LOGIC_CYCLE"], "{:#?}", out.diagnostics);
    assert_eq!(
        &src[out.diagnostics[0].span.clone()],
        "logic sig.out = latch(set=sig.a, reset=sig.out)",
    );
}

#[test]
fn a_latency_bound_counts_a_macro_by_the_port_it_enters() {
    let tail = "  assert latency(sig.s -> sig.out) <= 3\n  \
                assert latency(sig.r -> sig.out) <= 4\n";
    let src = source(&["sig.s", "sig.r"], "latch(set=sig.s, reset=sig.r)", tail);
    let intent = lower(&parse(&src).expect("parse"));
    for edition in [Edition::Java, Edition::Bedrock] {
        let found: Vec<String> = verify_latency(&intent, &legalized(&src, edition))
            .into_iter()
            .map(|d| d.primary)
            .collect();
        let edition = edition.as_str();
        assert_eq!(
            found,
            [
                format!(
                    "struct=s: `assert latency(sig.s -> sig.out) <= 3` fails on {edition}: its \
                     slowest path from sig.s to sig.out takes 6 ticks, 2 in the torches up to the \
                     actuator"
                ),
                format!(
                    "struct=s: `assert latency(sig.r -> sig.out) <= 4` fails on {edition}: its \
                     slowest path from sig.r to sig.out takes 7 ticks, 2 in the torches up to the \
                     actuator"
                ),
            ],
        );
    }
}
