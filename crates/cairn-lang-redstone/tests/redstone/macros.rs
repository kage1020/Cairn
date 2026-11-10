//! Each sequential macro synthesizes to one gate, carrying the stage count of one that counts
//! stages, which lowers to one cell on each edition (`spec/redstone` "Time model", "Edition
//! differences").

use std::fmt::Write as _;

use cairn_lang_core::logic_library::StageCount;
use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::{
    CellPortDriver, EditionCell, GateKind, LogicalCell, NetRef, PortName, SignalRef,
    compile_netlist, verify_latency,
};

use crate::common::{legalized_from_source, synth_source};

/// A struct with a lever on each of `inputs` and a door that `expression` opens, then `tail`.
fn source(inputs: &[&str], expression: &str, tail: &str) -> String {
    let mut levers = String::new();
    for (input, offset) in inputs.iter().zip(1..) {
        writeln!(
            levers,
            "  lever id=l{offset} at=inside.back offset={offset} y=1 -> {input}"
        )
        .expect("writing to a String");
    }
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct s size=11x7\n  floor mat_slot=wall\n  door id=d side=front at=center\n\
         {levers}  logic sig.out = {expression}\n  door[id=d] opened_by=sig.out\n  \
         circuit region=floor void=2\n{tail}"
    )
}

struct Case {
    expression: &'static str,
    inputs: &'static [&'static str],
    /// Each port the cell takes, with the input that drives it.
    ports: &'static [(PortName, u32)],
    gate: GateKind,
    logical: LogicalCell,
    java: EditionCell,
    bedrock: EditionCell,
}

const A: SignalRef = SignalRef::Input(0);

/// `count` stages.
fn n(count: u32) -> StageCount {
    StageCount::new(count).expect("a stage count is at least 1")
}

fn cases() -> [Case; 6] {
    let one = &["sig.a"][..];
    let on_a = &[(PortName::A, 0)][..];
    [
        Case {
            expression: "delay(sig.a, 3)",
            inputs: one,
            ports: on_a,
            gate: GateKind::Delay { a: A, stages: n(3) },
            logical: LogicalCell::Delay { stages: n(3) },
            java: EditionCell::JavaRepeaterDelay { stages: n(3) },
            bedrock: EditionCell::BedrockRepeaterDelay { stages: n(3) },
        },
        Case {
            expression: "pulse(sig.a, 4)",
            inputs: one,
            ports: on_a,
            gate: GateKind::Pulse { a: A, stages: n(4) },
            logical: LogicalCell::Pulse { stages: n(4) },
            java: EditionCell::JavaTorchPulse { stages: n(4) },
            bedrock: EditionCell::BedrockTorchPulse { stages: n(4) },
        },
        Case {
            expression: "edge_rising(sig.a)",
            inputs: one,
            ports: on_a,
            gate: GateKind::EdgeRising { a: A },
            logical: LogicalCell::EdgeRising,
            java: EditionCell::JavaTorchEdgeRising,
            bedrock: EditionCell::BedrockTorchEdgeRising,
        },
        Case {
            expression: "edge_falling(sig.a)",
            inputs: one,
            ports: on_a,
            gate: GateKind::EdgeFalling { a: A },
            logical: LogicalCell::EdgeFalling,
            java: EditionCell::JavaTorchEdgeFalling,
            bedrock: EditionCell::BedrockTorchEdgeFalling,
        },
        Case {
            expression: "latch(reset=sig.r, set=sig.s)",
            // Declared against the operands' order, so a swap of the two shows.
            inputs: &["sig.r", "sig.s"],
            ports: &[(PortName::Set, 1), (PortName::Reset, 0)],
            gate: GateKind::Latch {
                set: SignalRef::Input(1),
                reset: SignalRef::Input(0),
            },
            logical: LogicalCell::Latch,
            java: EditionCell::JavaTorchLatch,
            bedrock: EditionCell::BedrockTorchLatch,
        },
        Case {
            expression: "counter(sig.a, 2)",
            inputs: one,
            ports: on_a,
            gate: GateKind::Counter { a: A, stages: n(2) },
            logical: LogicalCell::Counter { stages: n(2) },
            java: EditionCell::JavaRepeaterCounter { stages: n(2) },
            bedrock: EditionCell::BedrockRepeaterCounter { stages: n(2) },
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

        let drivers: Vec<CellPortDriver> = case
            .ports
            .iter()
            .map(|&(port, input)| CellPortDriver {
                port,
                net: NetRef::Input(input),
            })
            .collect();
        for (edition, cell) in [(Edition::Java, case.java), (Edition::Bedrock, case.bedrock)] {
            let legalized = legalized_from_source(&src, edition);
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
fn a_macro_shares_its_gate_only_with_one_of_the_same_operand_and_stage_count() {
    let tail = "  logic sig.same = delay(sig.a, 2)\n  logic sig.more = delay(sig.a, 3)\n  \
                logic sig.other = delay(sig.b, 2)\n  logic sig.wide = pulse(sig.a, 2)\n  \
                logic sig.all = sig.same and sig.more and sig.other and sig.wide\n  \
                lamp id=lp at=inside.back offset=5 y=2 lit_by=sig.all\n";
    let src = source(&["sig.a", "sig.b"], "delay(sig.a, 2)", tail);
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
            GateKind::Delay { a: A, stages: n(2) },
            GateKind::Delay { a: A, stages: n(3) },
            GateKind::Delay {
                a: SignalRef::Input(1),
                stages: n(2),
            },
            GateKind::Pulse { a: A, stages: n(2) },
            GateKind::And2 {
                a: SignalRef::Gate(0),
                b: SignalRef::Gate(1),
            },
            GateKind::And2 {
                a: SignalRef::Gate(2),
                b: SignalRef::Gate(4),
            },
            GateKind::And2 {
                a: SignalRef::Gate(3),
                b: SignalRef::Gate(5),
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
    let notes: Vec<&str> = out.diagnostics[0]
        .notes
        .iter()
        .map(|note| note.message.as_str())
        .collect();
    assert_eq!(
        notes,
        [
            "Fix: break the cycle by driving one of the signals from a sensor. A `latch` or \
             `counter` holds its state inside its own cell, so it stands in for a loop of `logic` \
             lines, and one that reads its own output back closes the loop again."
        ],
    );
}

#[test]
fn a_latency_bound_counts_a_macro_by_the_port_it_enters() {
    let tail = "  assert latency(sig.s -> sig.out) <= 3\n  \
                assert latency(sig.r -> sig.out) <= 4\n";
    let src = source(&["sig.r", "sig.s"], "latch(set=sig.s, reset=sig.r)", tail);
    let intent = lower(&parse(&src).expect("parse"));
    for edition in [Edition::Java, Edition::Bedrock] {
        let found: Vec<String> = verify_latency(&legalized_from_source(&src, edition), &intent)
            .into_iter()
            .map(|d| d.primary)
            .collect();
        let edition = edition.as_str();
        assert_eq!(
            found,
            [
                format!(
                    "struct=s: `assert latency(sig.s -> sig.out) <= 3` fails on {edition}: its \
                     slowest path from sig.s to sig.out takes 4 ticks, all in its cells"
                ),
                format!(
                    "struct=s: `assert latency(sig.r -> sig.out) <= 4` fails on {edition}: its \
                     slowest path from sig.r to sig.out takes 5 ticks, all in its cells"
                ),
            ],
        );
    }
}
