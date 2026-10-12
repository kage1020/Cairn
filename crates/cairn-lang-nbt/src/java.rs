//! Java NBT writer: big-endian, optionally gzip-wrapped. The default gzip level matches the
//! game's own output, which keeps snapshots byte-stable against its samples.

use std::io::Write;

pub use crate::error::NbtIoError;
use crate::stream::{CompoundStream, stream_named_root};
use crate::tag::Compound;
use crate::writer::{Endian, write_named_root};

/// Writes `root` as a named Java NBT root without gzip.
///
/// # Errors
///
/// An I/O failure on `writer`, or a tag the encoder rejects.
pub fn write_java_uncompressed<W: Write>(
    writer: &mut W,
    root_name: &str,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_named_root(writer, Endian::Big, root_name, root)
}

/// Writes `root` as a named Java NBT root wrapped in gzip, as `.nbt` structure files are.
///
/// # Errors
///
/// Those of [`write_java_uncompressed`], or an I/O failure finishing the gzip stream.
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

/// Streaming form of [`write_java_uncompressed`] for a root whose entries `body` writes.
///
/// # Errors
///
/// A `root_name` that is not a valid NBT string, `body`'s error, or an I/O failure on `writer`.
///
/// # Panics
///
/// When a list `body` streams gets a different number of items than it declared.
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
