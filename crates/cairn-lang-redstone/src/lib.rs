//! Redstone for Cairn: lowers the Intent IR's `logic` to a Logic IR and a netlist, then places,
//! routes, buffers and legalizes it under the struct, in the order of `spec/redstone`
//! "Place-and-route".

pub mod circuit;
pub(crate) mod coupling;
pub mod delay;
pub mod diagnostic;
pub mod edition_netlist;
pub mod edition_netlist_ir;
#[cfg(test)]
pub(crate) mod fixtures;
pub mod geometry;
pub(crate) mod grid;
pub mod legalize;
pub mod logic_ir;
pub mod netlist;
pub mod netlist_ir;
pub mod part;
pub mod placement;
pub mod placement_ir;
pub mod portable;
pub mod routing;
pub(crate) mod sim;
pub mod synth;
pub mod tile;
pub mod verify;

pub use circuit::build_circuits;
pub use delay::{BUFFER_REPEATER_TICKS, DUST_ATTENUATION_LIMIT, DelayOutput, compile_delay};
pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
pub use edition_netlist::compile_edition_netlist;
pub use edition_netlist_ir::{
    EditionCell, EditionCellNode, EditionNetlistIr, ScopedEditionNetlistIr,
    ScopedEditionNetlistIrEntry,
};
pub use legalize::{LegalizeOutput, compile_legalize};
pub use logic_ir::{
    GateKind, GateNode, InputPort, LogicIr, OutputPort, ScopeKind, Scoped, ScopedEntry,
    ScopedLogicIr, ScopedLogicIrEntry, SignalRef,
};
pub use netlist::compile_netlist;
pub use netlist_ir::{
    CellNode, CellPortDriver, LogicalCell, NetRef, NetlistInput, NetlistOutput, PortName,
    ScopedNetlistIr, ScopedNetlistIrEntry,
};
pub use placement::{MIN_COUNTER_VOID, MIN_VOID, PlacementOutput, compile_placement};
pub use placement_ir::{
    Buffer, CircuitBlock, CircuitEntity, CircuitLayout, CircuitNet, LayoutStage, NetSink,
    PlacedTile, Reservation, ScopedCircuitLayout, ScopedCircuitLayoutEntry, TileInput, TileKind,
    Wire,
};
pub use portable::verify_portable;
pub use routing::{RoutingOutput, compile_routing};
pub use synth::{SynthOutput, synthesize};
pub use tile::{PadKind, TilePort};
pub use verify::{MAX_ASSERTION_SENSORS, verify_latency, verify_truth_and_temporal};

pub(crate) fn saturating_index(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}
