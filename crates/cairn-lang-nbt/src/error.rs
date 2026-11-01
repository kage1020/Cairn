//! The error every NBT writer returns.

use thiserror::Error;

/// Errors raised while encoding an NBT tag tree.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NbtIoError {
    /// A string held a NUL or non-ASCII byte, outside the range where Java's Modified UTF-8 and
    /// Bedrock's UTF-8 agree.
    #[error("nbt: string contains byte 0x{byte:02x} at index {index}, not encodable here")]
    InvalidString {
        /// The rejected byte.
        byte: u8,
        /// Its byte index in the string.
        index: usize,
    },
    /// A [`List`](crate::tag::List) declared one element type but an item carried another.
    #[error("nbt: list declared element type {declared} but item {index} has type {actual}")]
    HeterogeneousList {
        /// The declared element type id.
        declared: u8,
        /// The mismatching item's index.
        index: usize,
        /// The mismatching item's type id.
        actual: u8,
    },
    /// An empty [`List`](crate::tag::List) declared an element type other than `TAG_End`.
    #[error("nbt: empty list declared element type {declared}, expected 0 (TAG_End)")]
    EmptyListWithElementType {
        /// The declared element type id.
        declared: u8,
    },
    /// A length overflowed its wire prefix.
    #[error("nbt: {context} length {len} exceeds wire limit {limit}")]
    LengthOverflow {
        /// What the length measures, such as `"string"` or `"list"`.
        context: &'static str,
        /// The length.
        len: usize,
        /// The prefix's maximum.
        limit: usize,
    },
    /// An I/O failure.
    #[error("nbt I/O: {0}")]
    Io(#[from] std::io::Error),
}
