//! Core of the Cairn language.
//!
//! This crate hosts the parser, IR, and the compiler that resolves intent into a block-array IR.
//! It is the dependency root of every other Cairn crate.

/// The Cairn release version, in date-based versioning (`YYYY.0M[.PATCH]`).
pub const CAIRN_VERSION: &str = "2026.06";
