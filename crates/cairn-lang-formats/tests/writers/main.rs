//! The Java `.nbt` and Bedrock `.mcstructure` writers: the tag trees they
//! build, and the streamed bytes those trees must match.

#[path = "../common/mod.rs"]
mod common;

mod bedrock_structure;
mod java_structure;
mod streaming;
