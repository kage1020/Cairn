//! This crate's Rust API is in the
//! [Internal tier](https://cairn.kage1020.com/spec/compatibility/#internal) and carries no
//! promise: any release may change it.
//!
//! Language Server Protocol implementation for Cairn editors: diagnostics from `cairn-lang-core`
//! against the target the client pins, completion of its closed vocabularies, hover from the
//! keyword reference, the IR and the registry packs, quick-fixes from each diagnostic's payload,
//! and the `cairn info` and `cairn diff-blocks` reports as commands.

pub mod code_action;
pub mod commands;
pub mod completion;
pub mod diagnostics;
pub mod hover;
pub mod line_index;
pub mod pin;
pub mod server;
pub mod store;
#[cfg(test)]
mod test_fixtures;
