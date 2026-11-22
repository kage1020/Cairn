//! The library half of `cairn import` and `cairn diff-blocks`: what an import writes for structure
//! files, what compiling that back builds, what a lift compared with its file reports, and what
//! each refuses.

#[path = "../common/mod.rs"]
pub mod common;

mod bedrock;
mod diff_blocks;
mod java;
mod round_trip;
