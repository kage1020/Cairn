//! NBT codec for the Cairn language.
//!
//! Encodes Minecraft NBT in both on-disk dialects: Java big-endian
//! (gzip-wrapped) and Bedrock little-endian (uncompressed). The tag tree
//! in [`tag`] is endian-neutral, and the byte-level encoder is a single
//! endian-parameterised core, so the two writers cannot drift apart on
//! validation rules. Each writer also has a streaming form ([`stream`])
//! that encodes a root entry by entry, for output too large to build as
//! a tree first.
//!
//! Used by `cairn-lang-formats` for the various schematic/structure file
//! formats. The CLI never reaches in directly — it talks to format helpers
//! which talk to this crate.

pub mod bedrock;
pub mod java;
pub mod stream;
pub mod tag;
mod writer;

pub use bedrock::{stream_bedrock_uncompressed, write_bedrock_uncompressed};
pub use java::{
    NbtIoError, stream_java_gzip, stream_java_uncompressed, write_java_gzip,
    write_java_uncompressed,
};
pub use stream::{CompoundStream, ListStream};
pub use tag::{Compound, List, Tag};
