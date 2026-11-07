//! What the built-in registry packs hold: ids that exist in their targets, working aliases, and the
//! states and versions the game gives each id.

#[path = "../common/mod.rs"]
pub mod common;

mod aliases;
mod ids_exist;
mod state_domain;
mod states;
