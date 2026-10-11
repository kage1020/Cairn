//! The redstone integration tests, one module per behaviour under test,
//! built as one binary so the helpers in [`common`] compile once and
//! every one of them is used.

mod common;

mod actuator_patch_targets;
mod actuator_schema;
mod binding_hosts;
mod crossing;
mod delay;
mod edition_netlist;
mod front_end_cascades;
mod logic_synth;
mod netlist;
mod placement;
mod placement_ir_roundtrip;
mod routing;
mod signal_drivers;
mod source_order;
