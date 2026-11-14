//! NBT codec: Java big-endian (gzip-wrapped) and Bedrock little-endian (uncompressed), each with
//! a streaming form for output too large to build as a tree first, and a reader that builds only
//! the root entries its caller keeps.

pub mod bedrock;
mod error;
pub mod java;
pub mod read;
mod reader;
pub mod stream;
pub mod tag;
mod writer;

pub use bedrock::{
    read_bedrock_uncompressed, stream_bedrock_uncompressed, write_bedrock_uncompressed,
};
pub use java::{
    NbtIoError, read_java_gzip, read_java_uncompressed, stream_java_uncompressed, write_java_gzip,
    write_java_uncompressed,
};
pub use read::{MAX_DEPTH, NbtReadError, NbtRoot, ReadOptions};
pub use stream::{CompoundStream, ListStream};
pub use tag::{Compound, List, Tag, check_string};
