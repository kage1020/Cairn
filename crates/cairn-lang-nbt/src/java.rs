//! Java-flavoured NBT writer (big-endian, optionally gzip-wrapped).
//!
//! Two entry points: [`write_java_uncompressed`] for tests and
//! [`write_java_gzip`] for the on-disk `.nbt` format Minecraft Java itself
//! emits. [`stream_java_uncompressed`] is the streaming twin of the first,
//! writing the root's body as it goes rather than from a built tree (see
//! [`crate::stream`]); a caller wanting the gzip form wraps its writer in
//! its own encoder, so this crate's signatures name no `flate2` type. The
//! gzip wrapper uses [`flate2::Compression::default`] (level 6), which
//! matches Mojang's output — important so binary snapshots of small
//! structures stay byte-stable when checked against samples from the game.
//!
//! The byte-level encoding lives in the crate-internal `writer` module,
//! shared with the Bedrock writer; this module only pins the byte order
//! and the gzip envelope.

use std::io::Write;

use crate::stream::{CompoundStream, stream_named_root};
use crate::tag::Compound;
pub use crate::writer::NbtIoError;
use crate::writer::{Endian, write_named_root};

/// Write a root-level named compound as a Java-format NBT byte stream,
/// **without** gzip wrapping. Useful for tests where eyeballing the bytes
/// matters; production callers should use [`write_java_gzip`].
///
/// # Errors
///
/// Propagates any I/O failure on `writer` and any encoding error raised by
/// the tag tree (`InvalidString`, `HeterogeneousList`, `LengthOverflow`).
pub fn write_java_uncompressed<W: Write>(
    writer: &mut W,
    root_name: &str,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_named_root(writer, Endian::Big, root_name, root)
}

/// Write a root-level named compound as Minecraft Java structure files want
/// it: big-endian payload wrapped in gzip at the default compression level.
///
/// # Errors
///
/// Same set as [`write_java_uncompressed`] plus any I/O the gzip encoder
/// raises when flushing.
pub fn write_java_gzip<W: Write>(
    writer: &mut W,
    root_name: &str,
    root: &Compound,
) -> Result<(), NbtIoError> {
    let mut gz = flate2::write::GzEncoder::new(writer, flate2::Compression::default());
    write_java_uncompressed(&mut gz, root_name, root)?;
    gz.finish()?;
    Ok(())
}

/// Streaming twin of [`write_java_uncompressed`]: write a root-level named
/// compound whose entries `body` writes one at a time. The bytes are the
/// ones [`write_java_uncompressed`] writes for a root holding the same
/// entries in the same order.
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
pub fn stream_java_uncompressed<W, F>(
    writer: &mut W,
    root_name: &str,
    body: F,
) -> Result<(), NbtIoError>
where
    W: Write,
    F: FnOnce(&mut CompoundStream<'_, W>) -> Result<(), NbtIoError>,
{
    stream_named_root(writer, Endian::Big, root_name, body)
}
