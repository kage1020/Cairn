//! Bedrock-flavoured NBT writer (little-endian, uncompressed).
//!
//! Bedrock's on-disk structure format (`.mcstructure`) stores the same tag
//! vocabulary as Java but flips every multi-byte scalar to little-endian
//! and skips the gzip envelope entirely. There is no compressed entry
//! point on purpose: emitting a gzip-wrapped `.mcstructure` would produce
//! a file the game silently fails to load, so the API surface should not
//! make that mistake expressible.
//!
//! The byte-level encoding lives in [`crate::writer`], shared with the
//! Java writer; this module only pins the byte order.

use std::io::Write;

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
