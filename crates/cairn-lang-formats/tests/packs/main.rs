//! What the built-in registry packs hold: ids that exist in their targets, working aliases, and the
//! states and versions the game gives each id.

#[path = "../common/mod.rs"]
pub mod common;

mod absent;
mod aliases;
mod blockstate_members;
mod connections;
mod declared_elsewhere;
mod ids_exist;
mod spelling;
mod state_domain;
mod states;
mod waterlogged;
