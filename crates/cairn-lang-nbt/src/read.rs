//! What a read of a named root keeps, what it returns, and why it refuses.

use thiserror::Error;

use crate::tag::Compound;

/// The deepest a read follows compounds and lists, root included: the game's own limit.
pub const MAX_DEPTH: usize = 512;

/// Which root entries a read builds, and how large what it builds may grow.
#[derive(Debug, Clone, Copy)]
pub struct ReadOptions<'a> {
    /// The root entries to build, by name, or every one when `None`; the rest are skipped.
    pub keep: Option<&'a [&'a str]>,
    /// The most bytes the built tree may take: each tag counts its in-memory size, and each
    /// string, compound key and array its bytes.
    pub max_tree_bytes: usize,
}

/// A named root compound as read.
#[derive(Debug, Clone, PartialEq)]
pub struct NbtRoot {
    /// The root tag's name, empty in a structure file and in a `level.dat`.
    pub name: String,
    /// The entries the read built, in file order.
    pub compound: Compound,
    /// The name and tag id of each root entry it skipped, in file order.
    pub skipped: Vec<(String, u8)>,
}

/// Errors raised while decoding an NBT root.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NbtReadError {
    /// The first tag is not a compound.
    #[error("nbt: the root tag has type {id}, not a compound (10)")]
    RootNotCompound {
        /// The tag id read.
        id: u8,
    },
    /// A tag id outside `0..=12`.
    #[error("nbt: tag type {id} does not exist")]
    UnknownTagId {
        /// The tag id read.
        id: u8,
    },
    /// A list or array declared a negative length.
    #[error("nbt: a {context} declares the negative length {len}")]
    NegativeLength {
        /// `"list"`, `"byte_array"`, `"int_array"` or `"long_array"`.
        context: &'static str,
        /// The length read.
        len: i32,
    },
    /// A list of one or more items declared `TAG_End` as their type.
    #[error("nbt: a list of {len} items declares no element type")]
    UntypedList {
        /// The length read.
        len: i32,
    },
    /// Compounds and lists nested past [`MAX_DEPTH`].
    #[error("nbt: tags nest deeper than {MAX_DEPTH}")]
    TooDeep,
    /// The built tree would pass [`ReadOptions::max_tree_bytes`].
    #[error("nbt: the tags read would take more than {limit} bytes")]
    TooLarge {
        /// The limit.
        limit: usize,
    },
    /// A string that is not Java's Modified UTF-8, or Bedrock's UTF-8.
    #[error("nbt: a string is not valid {encoding}")]
    InvalidString {
        /// `"Modified UTF-8"` or `"UTF-8"`.
        encoding: &'static str,
    },
    /// The input ended inside a tag.
    #[error("nbt: the data ends inside a tag")]
    Truncated,
    /// Any other I/O failure, a gzip stream that does not decode among them.
    #[error("nbt I/O: {0}")]
    Io(std::io::Error),
}

impl From<std::io::Error> for NbtReadError {
    fn from(err: std::io::Error) -> Self {
        if err.kind() == std::io::ErrorKind::UnexpectedEof {
            Self::Truncated
        } else {
            Self::Io(err)
        }
    }
}
