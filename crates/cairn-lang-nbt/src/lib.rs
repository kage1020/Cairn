//! NBT codec: Java big-endian (gzip-wrapped) and Bedrock little-endian (uncompressed), each with
//! a streaming form for output too large to build as a tree first, and a pull reader for both.

pub mod bedrock;
mod error;
pub mod java;
pub mod read;
pub mod stream;
pub mod tag;
mod writer;

pub use bedrock::{stream_bedrock_uncompressed, write_bedrock_uncompressed};
pub use java::{NbtIoError, stream_java_uncompressed, write_java_gzip, write_java_uncompressed};
pub use read::{
    Event, MAX_DEPTH, NbtReadError, NbtReader, read_bedrock_uncompressed, read_java_gzip,
    read_java_uncompressed,
};
pub use stream::{CompoundStream, ListStream};
pub use tag::{Compound, List, Tag, check_string};
