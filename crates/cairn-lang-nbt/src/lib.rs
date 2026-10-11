//! NBT codec for the Cairn language.
//!
//! Encodes Minecraft NBT in both on-disk dialects: Java big-endian
//! (gzip-wrapped) and Bedrock little-endian (uncompressed). The tag tree
//! in [`tag`] is endian-neutral, and both writers share one
//! endian-parameterised encoder. Each uncompressed writer also has a
//! streaming form ([`stream`]) that encodes a root entry by entry, for
//! output too large to build as a tree first.
//!
//! `cairn-lang-formats` is the only caller; the CLI reaches NBT through the
//! structure writers there.

pub mod bedrock;
pub mod java;
pub mod stream;
pub mod tag;
mod writer;

pub use bedrock::{stream_bedrock_uncompressed, write_bedrock_uncompressed};
pub use java::{NbtIoError, stream_java_uncompressed, write_java_gzip, write_java_uncompressed};
pub use stream::{CompoundStream, ListStream};
pub use tag::{Compound, List, Tag, check_string};
