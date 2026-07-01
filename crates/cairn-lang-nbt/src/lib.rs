//! NBT codec for the Cairn language.
//!
//! Encodes Minecraft NBT in Java big-endian form (gzip-wrapped on disk).
//! Bedrock little-endian is a follow-up: the tag tree in [`tag`] is endian-
//! neutral so the Bedrock writer can grow alongside `write_java_*` without
//! a second tree type.
//!
//! Used by `cairn-lang-formats` for the various schematic/structure file
//! formats. The CLI never reaches in directly — it talks to format helpers
//! which talk to this crate.

pub mod java;
pub mod tag;

pub use java::{NbtIoError, write_java_gzip, write_java_uncompressed};
pub use tag::{Compound, List, Tag};
