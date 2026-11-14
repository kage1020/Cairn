//! Language Server Protocol implementation for Cairn editors: diagnostics from `cairn-lang-core`,
//! completion of its closed vocabularies, and hover from the keyword reference and the IR.

pub mod completion;
pub mod diagnostics;
pub mod hover;
pub mod line_index;
pub mod server;
pub mod store;
#[cfg(test)]
mod test_fixtures;
