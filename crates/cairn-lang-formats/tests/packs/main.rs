//! What the built-in registry packs hold: ids that exist in their targets, working aliases, and the
//! states and versions the game gives each id.

#[path = "../common/mod.rs"]
pub mod common;

mod absent;
mod aliases;
mod blockstate_members;
mod canonical_spelling;
mod conductors;
mod connections;
mod constraints;
mod declared_elsewhere;
mod eave_stair_shapes;
mod entities;
mod furniture;
mod ids_exist;
mod plates;
mod respelling;
mod signs;
mod spelling;
mod state_domain;
mod states;
mod waterlogged;
