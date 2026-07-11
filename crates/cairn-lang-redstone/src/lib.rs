//! Redstone for Cairn.
//!
//! Synthesizes a signal-graph (Logic IR) into a netlist, places and routes the netlist into a
//! voxel circuit, and verifies it with a headless per-tick simulator. The cell library is
//! three-tier (logical cell → edition cell → physical tile) so an edition difference is
//! confined to the library.
//!
//! The current build lands the first two pipeline stages:
//! [`synth::synthesize`] lowers the Intent IR's `logic` bindings, sensors,
//! and actuators into an edition-neutral Logic IR, and
//! [`netlist::compile_netlist`] rewrites that DAG into a Netlist IR of
//! cells + nets ready for the placement pass to consume.

pub mod diagnostic;
pub mod logic_ir;
pub mod netlist;
pub mod netlist_ir;
pub mod synth;

pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
pub use logic_ir::{
    GateKind, GateNode, InputPort, LogicIr, OutputPort, ScopeKind, ScopedLogicIr,
    ScopedLogicIrEntry, SignalRef,
};
pub use netlist::compile_netlist;
pub use netlist_ir::{
    CellNode, CellPortDriver, LogicalCell, NetRef, NetlistInput, NetlistIr, NetlistOutput,
    PortName, ScopedNetlistIr, ScopedNetlistIrEntry,
};
pub use synth::{SynthOutput, synthesize};
