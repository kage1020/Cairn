//! Logic IR → Netlist IR lowering.
//!
//! Rewrites each [`crate::logic_ir::ScopedLogicIrEntry`] into a
//! [`ScopedNetlistIrEntry`] by mapping every [`crate::logic_ir::GateNode`]
//! to a [`CellNode`] tagged with a [`LogicalCell`]. The pass is a single
//! forward walk: the Logic IR carries a topologically ordered DAG
//! (invariant enforced by [`crate::synth::synthesize`], which also
//! runs CSE + cycle detection + unbound-signal reporting), so this
//! stage carries no diagnostics of its own — it is a pure structural
//! rewrite.
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
    let inputs_len = safe_index(ir.inputs.len());

    for port in &ir.inputs {
        netlist.inputs.push(NetlistInput {
            name: port.name.clone(),
            span: port.span.clone(),
        });
    }

    for (idx, node) in ir.nodes.iter().enumerate() {
        let node_index = safe_index(idx);
        netlist.cells.push(gate_to_cell(
            node.kind,
            node.span.clone(),
            node_index,
            inputs_len,
        ));
    }

    let cells_len = safe_index(netlist.cells.len());
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

/// Saturating `usize -> u32` matching [`crate::synth`]'s helper: a
/// `.crn` big enough to overflow `u32` is well past any Cairn build the
/// compiler will practically finish, so clamp rather than panic on
/// adversarial input.
fn safe_index(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// Rewrite one Logic IR [`SignalRef`] into a Netlist IR [`NetRef`],
/// checking the arena bounds the synth pass is supposed to have
/// preserved. `max_gate` is the largest legal `Gate(j)` index +1 —
/// pass the current cells vector length when rewriting an output or
/// `signal_defs` reference (any `Gate` may be forward), and the current
/// node index when rewriting an operand inside a cell (`j < node_index`
/// per the topological invariant).
///
/// The checks are `debug_assert!` because the synth pass already
/// enforces them fail-loud — a broken invariant here would be a synth
/// regression, not user input, and release builds should keep zero cost.
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
