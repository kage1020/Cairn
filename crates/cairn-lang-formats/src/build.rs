//! The passes a front end runs over a source for a build: the gate `cairn check` applies, the
//! lowering at a target with each struct's circuit laid, the floors the registry contradicts, and
//! what refuses the run.

pub mod artifact;
pub mod circuits;
pub mod conflicts;
pub mod finding;
pub mod floors;
pub mod gate;
pub mod lowering;
pub mod run_level;
pub mod scopes;
