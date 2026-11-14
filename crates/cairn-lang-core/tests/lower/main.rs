//! Lowering to the intent IR and to the block-array IR.

#[path = "../common/mod.rs"]
pub mod common;

mod at_side_walkway;
mod chest_block;
mod components;
mod conditional_arguments;
mod connections;
mod cottage;
mod door_block;
mod door_column;
mod door_wall_fit;
mod edition_guard;
mod fill;
mod intent;
mod l_walkway;
mod level_scoped_members;
mod member_geometry;
mod no_theme_bound;
mod own_line_first;
mod palette_capacity;
mod palette_is_referenced;
mod parity_unsupported;
mod phase_order;
mod placement_overlap;
mod port_reads_the_wall_that_paints;
mod port_rejection_reason;
mod raw;
mod resolved_state;
mod roof_seats_on_the_wall;
mod state_domain;
mod state_unmapped;
mod struct_as_written;
mod structure_volume;
mod the_volume_is_what_gets_painted;
mod tier0_parts;
mod unknown_id_versions;
mod unplaceable_plate;
mod unreached_keys;
mod unread_mat_slot;
mod unreadable_argument_values;
mod village;
mod window_mirror;
mod window_walkway;
mod window_wall_fit;
mod without_target;
