//! Logic IR → Netlist IR lowering.
//!
//! Rewrites each [`crate::logic_ir::ScopedLogicIrEntry`] into a
//! [`ScopedNetlistIrEntry`] by mapping every [`crate::logic_ir::GateNode`]
//! to a [`CellNode`] tagged with a [`LogicalCell`]. The pass is a single
//! forward walk: the Logic IR already guarantees topological order and
//! CSE, so this stage carries no diagnostics of its own — it is a pure
//! structural rewrite.
//!
//! Edition is intentionally *not* in scope. Selecting Java `ComparatorAND`
//! vs Bedrock `TorchAND` is the Edition Cell step per `spec/redstone`
//! §14.6, which runs against a target [`cairn_lang_core::Edition`] in a
//! later pass. The Netlist IR built here is the "Logical Cell selection"
//! stage described by §14.8's IR diagram.
//!
//! Port ordering is canonical per cell — two-input gates emit
//! `[A, B]`; `Not` emits `[A]`; `Mux` emits `[Sel, A, B]` — so a
//! consumer can index by position without inspecting [`PortName`].

use crate::logic_ir::{GateKind, LogicIr, ScopedLogicIr, SignalRef};
use crate::netlist_ir::{
    CellNode, CellPortDriver, LogicalCell, NetRef, NetlistInput, NetlistIr, NetlistOutput,
    PortName, ScopedNetlistIr,
};

/// Lower a [`ScopedLogicIr`] to a [`ScopedNetlistIr`].
///
/// One [`NetlistIr`] entry per non-empty [`crate::logic_ir::LogicIr`] in
/// the input; empty scopes remain elided (they are already dropped by
/// [`ScopedLogicIr::push`], so this pass simply walks whatever the synth
/// pass emitted).
#[must_use]
pub fn compile_netlist(scoped: &ScopedLogicIr) -> ScopedNetlistIr {
    let mut out = ScopedNetlistIr::new();
    for entry in &scoped.scopes {
        let netlist = compile_scope(&entry.ir);
        out.push(entry.kind, entry.name.clone(), netlist);
    }
    out
}

fn compile_scope(ir: &LogicIr) -> NetlistIr {
    let mut netlist = NetlistIr::new();

    for port in &ir.inputs {
        netlist.inputs.push(NetlistInput {
            name: port.name.clone(),
            span: port.span.clone(),
        });
    }

    for node in &ir.nodes {
        netlist
            .cells
            .push(gate_to_cell(node.kind, node.span.clone()));
    }

    for port in &ir.outputs {
        netlist.outputs.push(NetlistOutput {
            name: port.name.clone(),
            driver: net_ref_from(port.driver),
            span: port.span.clone(),
        });
    }

    for (name, sig) in &ir.signal_defs {
        netlist.signal_defs.insert(name.clone(), net_ref_from(*sig));
    }

    netlist
}

fn net_ref_from(sig: SignalRef) -> NetRef {
    match sig {
        SignalRef::Input(i) => NetRef::Input(i),
        SignalRef::Gate(j) => NetRef::Cell(j),
    }
}

fn gate_to_cell(kind: GateKind, span: cairn_lang_core::error::Span) -> CellNode {
    let (cell, drivers) = match kind {
        GateKind::And2 { a, b } => (LogicalCell::And, two_input(a, b)),
        GateKind::Or2 { a, b } => (LogicalCell::Or, two_input(a, b)),
        GateKind::Xor2 { a, b } => (LogicalCell::Xor, two_input(a, b)),
        GateKind::Nand2 { a, b } => (LogicalCell::Nand, two_input(a, b)),
        GateKind::Nor2 { a, b } => (LogicalCell::Nor, two_input(a, b)),
        GateKind::Not { a } => (
            LogicalCell::Not,
            vec![CellPortDriver {
                port: PortName::A,
                net: net_ref_from(a),
            }],
        ),
        GateKind::Mux { sel, a, b } => (
            LogicalCell::Mux,
            vec![
                CellPortDriver {
                    port: PortName::Sel,
                    net: net_ref_from(sel),
                },
                CellPortDriver {
                    port: PortName::A,
                    net: net_ref_from(a),
                },
                CellPortDriver {
                    port: PortName::B,
                    net: net_ref_from(b),
                },
            ],
        ),
    };
    CellNode {
        cell,
        drivers,
        span,
    }
}

fn two_input(a: SignalRef, b: SignalRef) -> Vec<CellPortDriver> {
    vec![
        CellPortDriver {
            port: PortName::A,
            net: net_ref_from(a),
        },
        CellPortDriver {
            port: PortName::B,
            net: net_ref_from(b),
        },
    ]
}
