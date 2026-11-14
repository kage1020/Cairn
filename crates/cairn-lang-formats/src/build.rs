//! The passes a front end runs over a source for a build: the gate `cairn check` applies, the
//! lowering at a target with each struct's circuit laid, what refuses the run, and the per-edition
//! walk `cairn info` reports.

pub mod artifact;
pub mod circuits;
pub mod finding;
pub mod floors;
pub mod gate;
pub mod info;
pub mod lowering;
pub mod run_level;
pub mod scopes;
