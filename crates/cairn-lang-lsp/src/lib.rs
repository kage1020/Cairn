//! Language Server Protocol implementation for Cairn editors: diagnostics from `cairn-lang-core`
//! against the target the client pins, completion of its closed vocabularies, hover from the
//! keyword reference and the IR, and quick-fixes from each diagnostic's payload.

pub mod code_action;
pub mod completion;
pub mod diagnostics;
pub mod hover;
pub mod line_index;
pub mod pin;
pub mod server;
pub mod store;
#[cfg(test)]
mod test_fixtures;
