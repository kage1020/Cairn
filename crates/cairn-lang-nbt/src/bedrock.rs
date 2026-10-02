//! Bedrock-flavoured NBT writer (little-endian, uncompressed).
//!
//! Bedrock's on-disk structure format (`.mcstructure`) stores the same tag
//! vocabulary as Java but flips every multi-byte scalar to little-endian
//! and skips the gzip envelope entirely. There is no compressed entry
//! point on purpose: emitting a gzip-wrapped `.mcstructure` would produce
//! a file the game silently fails to load, so the API surface should not
//! make that mistake expressible.
//!
//! The byte-level encoding lives in the crate-internal `writer` module,
//! shared with the Java writer; this module only pins the byte order.
//! [`stream_bedrock_uncompressed`] is the streaming twin of
//! [`write_bedrock_uncompressed`] (see [`crate::stream`]).

use std::io::Write;

use crate::stream::{CompoundStream, stream_named_root};
use crate::tag::Compound;
use crate::writer::{Endian, NbtIoError, write_named_root};

/// Write a root-level named compound as a Bedrock-format NBT byte stream.
/// `.mcstructure` files use an empty `root_name`.
///
/// # Errors
///
/// Propagates any I/O failure on `writer` and any encoding error raised by
/// the tag tree (`InvalidString`, `HeterogeneousList`, `LengthOverflow`).
pub fn write_bedrock_uncompressed<W: Write>(
    writer: &mut W,
    root_name: &str,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_named_root(writer, Endian::Little, root_name, root)
}

/// Streaming twin of [`write_bedrock_uncompressed`]: write a root-level
/// named compound whose entries `body` writes one at a time. The bytes are
/// the ones [`write_bedrock_uncompressed`] writes for a root holding the
/// same entries in the same order.
///
/// # Errors
///
/// [`NbtIoError::InvalidString`] or [`NbtIoError::LengthOverflow`] when
/// `root_name` cannot be written as an NBT string, whatever `body`
/// returns, and I/O failure on `writer`.
///
/// # Panics
///
/// Panics when a list `body` streams is given a number of items other than
/// it declared (see [`crate::stream`]).
pub fn stream_bedrock_uncompressed<W, F>(
    writer: &mut W,
    root_name: &str,
    body: F,
) -> Result<(), NbtIoError>
where
    W: Write,
    F: FnOnce(&mut CompoundStream<'_, W>) -> Result<(), NbtIoError>,
{
    stream_named_root(writer, Endian::Little, root_name, body)
}
