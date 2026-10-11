//! Redstone for Cairn.
//!
//! Synthesizes a signal-graph (Logic IR) into a netlist, places and routes the netlist into a
//! voxel circuit, and verifies it with a headless per-tick simulator. The cell library is
//! three-tier (logical cell → edition cell → physical tile) so an edition difference is
//! confined to the library.
//!
//! The passes, in the order of `spec/redstone` "Connection to the IR and phases" and
//! "Place-and-route":
//!
//! - [`synth::synthesize`] lowers the Intent IR's `logic` bindings, sensors and actuators into an
//!   edition-neutral Logic IR;
//! - [`netlist::compile_netlist`] rewrites that DAG into a Netlist IR of cells and nets;
//! - [`edition_netlist::compile_edition_netlist`] picks each cell's realisation for a
//!   [`cairn_lang_core::Edition`], the edition-cell tier of
//!   `spec/redstone` "Edition differences", as an [`edition_netlist_ir::EditionNetlistIr`];
//! - [`placement::compile_placement`], stage 1, lays the cells out inside each scope's
//!   `circuit region=` reservation;
//! - [`routing::compile_routing`], stage 2, lays a Steiner tree per net and fills each cell's
//!   `wire_length`;
//! - [`delay::compile_delay`], stage 3, fills each cell's `local_delay_ticks`, a local wire cost
//!   rather than an arrival time;
//! - [`crossing::compile_crossing`], stage 4, fills each cell's `buffer_coords` with the coord
//!   of every implicit buffer repeater. Crossings are prevented rather than legalized: stage 2
//!   lays each net around the ones before it, climbing to a
//!   [`placement_ir::RouteLayer::Bridge`] layer where the plane has no way round.
//!
//! Each [`placement_ir::PlacedCellNode`] names the last pass to touch it with a
//! [`placement_ir::PlacementStage`], which the JSON dump carries as a `stage` key in the
//! vocabulary of `cairn synth --stage <s>`. The physical-tile layer and the simulator are not
//! implemented here.

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

/// Saturating `usize -> u32` for every index space in the crate. A `.crn` that overflows `u32`
/// is far past any build the compiler would finish, so this clamps rather than panics on
/// adversarial input.
pub(crate) fn saturating_index(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}
