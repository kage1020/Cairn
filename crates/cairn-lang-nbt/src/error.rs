//! The errors the NBT writers and the reader return.

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

/// Why bytes could not be read as NBT, with the offset into the uncompressed stream where the
/// fault starts.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NbtReadError {
    /// The root tag is not a compound.
    #[error("nbt: the root tag has type {id}, expected 10 (TAG_Compound)")]
    RootNotCompound {
        /// The root's type id.
        id: u8,
    },
    /// A type id past `TAG_Long_Array` (12), or `TAG_End` where a value belongs.
    #[error("nbt: tag type {id} at byte {offset} is not a value type (1 to 12)")]
    UnknownTagType {
        /// The type id.
        id: u8,
        /// Where its byte sits.
        offset: u64,
    },
    /// A list or array whose `i32` length prefix is negative.
    #[error("nbt: the {context} at byte {offset} declares the negative length {len}")]
    NegativeLength {
        /// `"list"`, `"byte_array"`, `"int_array"` or `"long_array"`.
        context: &'static str,
        /// The declared length.
        len: i32,
        /// Where the prefix starts.
        offset: u64,
    },
    /// A non-empty list that declares `TAG_End` as its element type.
    #[error("nbt: the list at byte {offset} holds {len} items of type 0 (TAG_End)")]
    ListOfEnd {
        /// The declared length.
        len: i32,
        /// Where the list's element type sits.
        offset: u64,
    },
    /// A list or array longer than the reader was told to accept.
    #[error(
        "nbt: the {context} at byte {offset} declares {len} items, past this reader's limit of \
         {limit}"
    )]
    TooLong {
        /// `"list"`, `"byte_array"`, `"int_array"` or `"long_array"`.
        context: &'static str,
        /// The declared length.
        len: usize,
        /// The most items the reader accepts.
        limit: usize,
        /// Where the prefix starts.
        offset: u64,
    },
    /// Compounds and lists nested deeper than the game itself reads.
    #[error("nbt: the tag at byte {offset} nests past {limit} compounds and lists")]
    TooDeep {
        /// The deepest nesting accepted.
        limit: usize,
        /// Where the tag that goes past it starts.
        offset: u64,
    },
    /// A string that is not Modified UTF-8 (Java) or UTF-8 (Bedrock), or holds a lone surrogate.
    #[error("nbt: the string at byte {offset} is not valid {encoding}")]
    InvalidString {
        /// `"Modified UTF-8"` or `"UTF-8"`.
        encoding: &'static str,
        /// Where the string's length prefix starts.
        offset: u64,
    },
    /// An I/O failure, or input that ends inside a tag.
    #[error("nbt I/O: {0}")]
    Io(#[from] std::io::Error),
}
