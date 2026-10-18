//! Lowering to the intent IR and to the block-array IR.

#[path = "../common/mod.rs"]
pub mod common;

mod at_side_walkway;
mod conditional_arguments;
mod cottage;
mod door_wall_fit;
mod intent;
mod l_walkway;
mod level_scoped_members;
mod no_theme_bound;
mod palette_capacity;
mod palette_is_referenced;
mod phase_order;
mod port_reads_the_wall_that_paints;
mod port_rejection_reason;
mod roof_seats_on_the_wall;
mod structure_volume;
mod the_volume_is_what_gets_painted;
mod unread_mat_slot;
mod unreadable_argument_values;
mod village;
mod window_mirror;
mod window_walkway;
mod window_wall_fit;
mod without_target;
