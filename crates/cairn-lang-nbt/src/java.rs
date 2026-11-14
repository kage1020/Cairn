//! Java NBT writer and reader: big-endian, optionally gzip-wrapped. The default gzip level matches
//! the game's own output, which keeps snapshots byte-stable against its samples.

use std::io::{BufReader, Read, Write};

pub use crate::error::NbtIoError;
use crate::read::{NbtReadError, NbtRoot, ReadOptions};
use crate::reader::read_named_root;
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

/// Reads a named Java NBT root that is not gzip-wrapped, building the entries `options` keeps.
///
/// # Errors
///
/// An I/O failure on `reader`, or bytes that do not decode as an NBT root within `options`.
pub fn read_java_uncompressed<R: Read>(
    reader: R,
    options: &ReadOptions<'_>,
) -> Result<NbtRoot, NbtReadError> {
    read_named_root(reader, Endian::Big, options)
}

/// Reads a gzip-wrapped named Java NBT root, as `.nbt` structure files and `level.dat` hold one.
///
/// # Errors
///
/// Those of [`read_java_uncompressed`], a gzip stream that does not decode among the I/O failures.
pub fn read_java_gzip<R: Read>(
    reader: R,
    options: &ReadOptions<'_>,
) -> Result<NbtRoot, NbtReadError> {
    let gz = BufReader::new(flate2::read::MultiGzDecoder::new(reader));
    read_java_uncompressed(gz, options)
}
