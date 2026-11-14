//! Language Server Protocol implementation for Cairn editors: diagnostics from `cairn-lang-core`,
//! completion of its closed vocabularies, and the `cairn info` and `cairn diff-blocks` reports as
//! commands.

pub mod commands;
pub mod completion;
pub mod diagnostics;
pub mod line_index;
pub mod server;
pub mod store;
#[cfg(test)]
mod test_fixtures;
