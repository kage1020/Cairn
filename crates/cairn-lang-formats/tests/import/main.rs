//! `cairn import`'s library half: what it writes for structure files, what compiling that back
//! builds, and what it refuses.

#[path = "../common/mod.rs"]
pub mod common;

mod bedrock;
mod java;
mod round_trip;
