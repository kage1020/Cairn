//! The redstone integration tests, one module per behaviour.

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
