//! What the bindings answer for a source, as the Rust values each export turns into JavaScript.

#![cfg(not(target_arch = "wasm32"))]

mod common;
mod compile;
mod info;
mod targets;
