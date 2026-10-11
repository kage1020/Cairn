//! Redstone for Cairn: lowers the Intent IR's `logic` to a Logic IR and a netlist, then places,
//! routes, times and buffers it, in the order of `spec/redstone` "Place-and-route".

pub mod crossing;
pub mod delay;
pub mod diagnostic;
pub mod edition_netlist;
pub mod edition_netlist_ir;
pub(crate) mod grid;
pub mod logic_ir;
pub mod netlist;
pub mod netlist_ir;
pub(crate) mod pass;
pub mod placement;
pub mod placement_ir;
pub(crate) mod repeater_sites;
pub mod routing;
pub(crate) mod routing_geometry;
pub mod synth;
#[cfg(test)]
pub(crate) mod test_fixtures;

pub use crossing::{CrossingOutput, compile_crossing};
pub use delay::{
    BUFFER_REPEATER_TICKS, DUST_ATTENUATION_LIMIT, DelayOutput, MAX_ATTENUATION_SEGMENT,
    compile_delay,
};
pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
pub use edition_netlist::compile_edition_netlist;
pub use edition_netlist_ir::{
    EditionCell, EditionCellNode, EditionNetlistIr, ScopedEditionNetlistIr,
    ScopedEditionNetlistIrEntry,
};
pub use logic_ir::{
    GateKind, GateNode, InputPort, LogicIr, OutputPort, ScopeKind, Scoped, ScopedEntry,
    ScopedLogicIr, ScopedLogicIrEntry, SignalRef,
};
pub use netlist::compile_netlist;
pub use netlist_ir::{
    CellNode, CellPortDriver, LogicalCell, NetRef, NetlistInput, NetlistOutput, PortName,
    ScopedNetlistIr, ScopedNetlistIrEntry,
};
pub use placement::{CELL_FOOTPRINT, PlacementOutput, compile_placement};
pub use placement_ir::{
    BufferCoord, BufferSegment, CellCoord, CircuitRegionReservation, PlacedCellNode,
    PlacedOutputNode, PlacementIr, PlacementPhase, PlacementPhaseTransitionError, PlacementStage,
    RouteLayer, ScopedPlacementIr, ScopedPlacementIrEntry,
};
pub use routing::{RoutingOutput, compile_routing};
pub use synth::{SynthOutput, synthesize};

pub(crate) fn saturating_index(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}
