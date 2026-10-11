//! NBT codec: Java big-endian (gzip-wrapped) and Bedrock little-endian (uncompressed), each with
//! a streaming form for output too large to build as a tree first.

pub mod bedrock;
pub mod java;
pub mod stream;
pub mod tag;
mod writer;

pub use bedrock::{stream_bedrock_uncompressed, write_bedrock_uncompressed};
pub use java::{NbtIoError, stream_java_uncompressed, write_java_gzip, write_java_uncompressed};
pub use stream::{CompoundStream, ListStream};
pub use tag::{Compound, List, Tag, check_string};
