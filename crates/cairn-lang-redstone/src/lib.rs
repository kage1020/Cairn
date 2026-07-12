//! Redstone for Cairn.
//!
//! Synthesizes a signal-graph (Logic IR) into a netlist, places and routes the netlist into a
//! voxel circuit, and verifies it with a headless per-tick simulator. The cell library is
//! three-tier (logical cell → edition cell → physical tile) so an edition difference is
//! confined to the library.
//!
//! The crate currently exposes the first three pipeline stages:
//! [`synth::synthesize`] lowers the Intent IR's `logic` bindings, sensors,
//! and actuators into an edition-neutral Logic IR;
//! [`netlist::compile_netlist`] rewrites that DAG into a Netlist IR of
//! cells + nets (still edition-neutral); and
//! [`edition_netlist::compile_edition_netlist`] selects the target-edition
//! realisation of each cell against a [`cairn_lang_core::Edition`]. The
//! placement, route, and simulator layers below the Edition Netlist IR
//! are not yet public API — see `spec/redstone` for the complete pipeline
//! they will fill in.

pub mod diagnostic;
pub mod edition_netlist;
pub mod edition_netlist_ir;
pub mod logic_ir;
pub mod netlist;
pub mod netlist_ir;
pub mod synth;

pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
pub use edition_netlist::compile_edition_netlist;
pub use edition_netlist_ir::{
    EditionCell, EditionCellNode, EditionNetlistIr, ScopedEditionNetlistIr,
    ScopedEditionNetlistIrEntry,
};
pub use logic_ir::{
    GateKind, GateNode, InputPort, LogicIr, OutputPort, ScopeKind, ScopedLogicIr,
    ScopedLogicIrEntry, SignalRef,
};
pub use netlist::compile_netlist;
pub use netlist_ir::{
    CellNode, CellPortDriver, LogicalCell, NetRef, NetlistInput, NetlistOutput, PortName,
    ScopedNetlistIr, ScopedNetlistIrEntry,
};
pub use synth::{SynthOutput, synthesize};
