//! Bedrock NBT writer: little-endian and uncompressed. It has no gzip form because the game fails
//! to load a gzip-wrapped `.mcstructure`.

use std::io::Write;

use crate::stream::{CompoundStream, stream_named_root};
use crate::tag::Compound;
use crate::writer::{Endian, NbtIoError, write_named_root};

/// Writes `root` as a named Bedrock NBT root; `.mcstructure` files use an empty `root_name`.
///
/// # Errors
///
/// An I/O failure on `writer`, or a tag the encoder rejects.
pub fn write_bedrock_uncompressed<W: Write>(
    writer: &mut W,
    root_name: &str,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_named_root(writer, Endian::Little, root_name, root)
}

/// Streaming form of [`write_bedrock_uncompressed`] for a root whose entries `body` writes.
///
/// # Errors
///
/// A `root_name` that is not a valid NBT string, `body`'s error, or an I/O failure on `writer`.
///
/// # Panics
///
/// When a list `body` streams gets a different number of items than it declared.
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
