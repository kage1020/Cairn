//! NBT codec for the Cairn language.
//!
//! Encodes Minecraft NBT in both on-disk dialects: Java big-endian
//! (gzip-wrapped) and Bedrock little-endian (uncompressed). The tag tree
//! in [`tag`] is endian-neutral, and the byte-level encoder is a single
//! endian-parameterised core, so the two writers cannot drift apart on
//! validation rules.
//!
//! Used by `cairn-lang-formats` for the various schematic/structure file
//! formats. The CLI never reaches in directly — it talks to format helpers
//! which talk to this crate.

pub mod bedrock;
pub mod java;
pub mod tag;
mod writer;

pub use bedrock::write_bedrock_uncompressed;
pub use java::{NbtIoError, write_java_gzip, write_java_uncompressed};
pub use tag::{Compound, List, Tag};
