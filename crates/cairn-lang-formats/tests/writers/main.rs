//! The Java `.nbt` and Bedrock `.mcstructure` writers: their tag trees and streamed bytes.

#[path = "../common/mod.rs"]
pub mod common;

mod bedrock_structure;
mod java_structure;
mod streaming;
