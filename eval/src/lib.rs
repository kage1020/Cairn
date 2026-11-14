//! The evaluation harness `spec/evaluation` calls for: each prompt of a set goes to a model that
//! sees only the specification, each program it replies with goes through `cairn check` and
//! `cairn compile`, up to three corrections answer what they report, and the run is written down
//! as a record of the primary metrics.
//!
//! `cargo eval run` runs it and `cargo eval compare` sets two records side by side.

pub mod compare;
pub mod compiler;
pub mod conversation;
mod json;
pub mod metrics;
pub mod model;
pub mod prompts;
pub mod record;
pub mod run;
pub mod spec;
