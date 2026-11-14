//! This crate's Rust API is in the
//! [Internal tier](https://cairn.kage1020.com/spec/compatibility/#internal) and carries no
//! promise: any release may change it.
//!
//! Language Server Protocol implementation for Cairn editors: diagnostics from `cairn-lang-core`
//! and completion of its closed vocabularies.

pub mod completion;
pub mod diagnostics;
pub mod line_index;
pub mod server;
pub mod store;
#[cfg(test)]
mod test_fixtures;
