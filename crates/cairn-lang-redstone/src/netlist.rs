//! Logic IR → Netlist IR lowering.
//!
//! Maps every [`crate::logic_ir::GateNode`] to a [`CellNode`] tagged with a [`LogicalCell`], the
//! "Logical Cell selection" of `spec/redstone` "Connection to the IR and phases". The Logic IR is
//! already topologically ordered and checked by [`crate::synth::synthesize`], so this is one
//! forward walk with no diagnostics. The edition is chosen one step down, by
//! [`crate::edition_netlist::compile_edition_netlist`].

use crate::logic_ir::{GateKind, LogicIr, ScopedLogicIr, SignalRef};
use crate::netlist_ir::{
    CellNode, CellPortDriver, LogicalCell, NetRef, NetlistInput, NetlistIr, NetlistOutput,
    PortName, ScopedNetlistIr,
};
use crate::saturating_index;

/// Lowers a [`ScopedLogicIr`] to a [`ScopedNetlistIr`], one [`NetlistIr`] per scope.
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
    let inputs_len = saturating_index(ir.inputs.len());

    for port in &ir.inputs {
        netlist.inputs.push(NetlistInput {
            name: port.name.clone(),
            span: port.span.clone(),
        });
    }

    for (idx, node) in ir.nodes.iter().enumerate() {
        let node_index = saturating_index(idx);
        netlist.cells.push(gate_to_cell(
            node.kind,
            node.span.clone(),
            node_index,
            inputs_len,
        ));
    }

    let cells_len = saturating_index(netlist.cells.len());
    for port in &ir.outputs {
        netlist.outputs.push(NetlistOutput {
            name: port.name.clone(),
            driver: net_ref_from(port.driver, cells_len, inputs_len),
            span: port.span.clone(),
        });
    }

    for (name, sig) in &ir.signal_defs {
        netlist
            .signal_defs
            .insert(name.clone(), net_ref_from(*sig, cells_len, inputs_len));
    }

    netlist
}

/// Rewrites a Logic IR [`SignalRef`] as a [`NetRef`], debug-checking the arena bounds synth
/// guarantees. `max_gate` is one past the largest legal `Gate` index: the cell count for an output
/// or `signal_defs` entry, and the node's own index for an operand, which must point earlier.
fn net_ref_from(sig: SignalRef, max_gate: u32, inputs_len: u32) -> NetRef {
    match sig {
        SignalRef::Input(i) => {
            debug_assert!(
                i < inputs_len,
                "SignalRef::Input({i}) out of bounds; inputs_len={inputs_len}",
            );
            NetRef::Input(i)
        }
        SignalRef::Gate(j) => {
            debug_assert!(
                j < max_gate,
                "SignalRef::Gate({j}) out of bounds; max_gate={max_gate}",
            );
            NetRef::Cell(j)
        }
    }
}

fn gate_to_cell(
    kind: GateKind,
    span: cairn_lang_core::error::Span,
    node_index: u32,
    inputs_len: u32,
) -> CellNode {
    let ref_of = |sig| net_ref_from(sig, node_index, inputs_len);
    let (cell, drivers) = match kind {
        GateKind::And2 { a, b } => (LogicalCell::And, two_input(a, b, node_index, inputs_len)),
        GateKind::Or2 { a, b } => (LogicalCell::Or, two_input(a, b, node_index, inputs_len)),
        GateKind::Xor2 { a, b } => (LogicalCell::Xor, two_input(a, b, node_index, inputs_len)),
        GateKind::Nand2 { a, b } => (LogicalCell::Nand, two_input(a, b, node_index, inputs_len)),
        GateKind::Nor2 { a, b } => (LogicalCell::Nor, two_input(a, b, node_index, inputs_len)),
        GateKind::Not { a } => (
            LogicalCell::Not,
            vec![CellPortDriver {
                port: PortName::A,
                net: ref_of(a),
            }],
        ),
        GateKind::Mux { sel, a, b } => (
            LogicalCell::Mux,
            vec![
                CellPortDriver {
                    port: PortName::Sel,
                    net: ref_of(sel),
                },
                CellPortDriver {
                    port: PortName::A,
                    net: ref_of(a),
                },
                CellPortDriver {
                    port: PortName::B,
                    net: ref_of(b),
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

fn two_input(a: SignalRef, b: SignalRef, node_index: u32, inputs_len: u32) -> Vec<CellPortDriver> {
    vec![
        CellPortDriver {
            port: PortName::A,
            net: net_ref_from(a, node_index, inputs_len),
        },
        CellPortDriver {
            port: PortName::B,
            net: net_ref_from(b, node_index, inputs_len),
        },
    ]
}
