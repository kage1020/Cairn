//! Redstone for Cairn.
//!
//! Synthesizes a signal-graph (Logic IR) into a netlist, places and routes the netlist into a
//! voxel circuit, and verifies it with a headless per-tick simulator. The cell library is
//! three-tier (logical cell → edition cell → physical tile) so an edition difference is
//! confined to the library.
//!
//! The crate currently exposes the first three of `spec/redstone` §14.8's
//! four IR layers plus the cell library's tier-2 selection pass:
//! [`synth::synthesize`] lowers the Intent IR's `logic` bindings,
//! sensors, and actuators into an edition-neutral Logic IR;
//! [`netlist::compile_netlist`] rewrites that DAG into an
//! edition-neutral Netlist IR of cells + nets;
//! [`edition_netlist::compile_edition_netlist`] picks the target-edition
//! realisation of each cell against a [`cairn_lang_core::Edition`] —
//! the second rung (`Edition Cell`) of the three-tier cell library
//! from §14.6, materialised as [`edition_netlist_ir::EditionNetlistIr`]
//! so the placement pass has one type to consume;
//! [`placement::compile_placement`] lays out those edition-tagged
//! cells inside each scope's `circuit region=` reservation — stage 1
//! of the five-stage place-and-route pipeline §14.5 describes;
//! [`routing::compile_routing`] runs stage 2 (Steiner routing) over
//! that layout, filling every [`placement_ir::PlacedCellNode`]'s
//! `wire_length` with the routed total of the driver→sink paths
//! through its Steiner tree and re-checking `E_ROUTE_CONGESTION`
//! against the actual post-routing occupancy; [`delay::compile_delay`] runs stage 3
//! (delay insertion) over the routed IR, promoting every cell's
//! `delay_ticks` from `None` to `Some(base delay + implicit buffer
//! repeater ticks)` and refusing with `E_ATTENUATION_LIMIT` whenever a
//! single driver segment exceeds the v1 sanity cap; and
//! [`crossing::compile_crossing`] runs stage 4 (crossing legalization)
//! over the delayed IR, filling every cell's `buffer_coords` with the
//! coord of each implicit buffer repeater the delay pass counted. The
//! crossing it is named for is prevented rather than legalized: stage
//! 2 lays each net around the dust of the nets before it, climbing to
//! a [`placement_ir::RouteLayer::Bridge`] layer where there is no way
//! round on the plane, and refusing the scope where there is no way at
//! all. Every
//! [`placement_ir::PlacedCellNode`] names the last of those four
//! passes to touch it via [`placement_ir::PlacementStage`], which the
//! JSON dump carries as a `stage` key in the same vocabulary
//! `cairn synth --stage <s>` accepts. The physical-tile
//! selection and simulator layers are not yet public API — see
//! `spec/redstone` for the complete pipeline they will fill in.

pub mod crossing;
pub mod delay;
pub mod diagnostic;
pub mod edition_netlist;
pub mod edition_netlist_ir;
pub mod logic_ir;
pub mod netlist;
pub mod netlist_ir;
pub mod placement;
pub mod placement_ir;
pub mod routing;
pub(crate) mod routing_geometry;
pub mod synth;

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
    GateKind, GateNode, InputPort, LogicIr, OutputPort, ScopeKind, ScopedLogicIr,
    ScopedLogicIrEntry, SignalRef,
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
