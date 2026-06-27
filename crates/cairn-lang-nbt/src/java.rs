//! Java-flavoured NBT writer (big-endian, optionally gzip-wrapped).
//!
//! Two entry points: [`write_java_uncompressed`] for tests and
//! [`write_java_gzip`] for the on-disk `.nbt` format Minecraft Java itself
//! emits. The gzip wrapper uses [`flate2::Compression::default`] (level 6),
//! which matches Mojang's output — important so binary snapshots of small
//! structures stay byte-stable when checked against samples from the game.

use std::io::Write;

use thiserror::Error;

use crate::tag::{Compound, List, Tag};

/// Errors raised while encoding an NBT tag tree.
#[derive(Debug, Error)]
pub enum NbtIoError {
    /// A [`Tag::String`] contained a byte that cannot be carried by Java
    /// Modified UTF-8. The encoder currently declines the NUL byte and
    /// non-ASCII (a supplementary-pair encoder is reserved for a future
    /// extension once a registry-pack id is observed outside that range).
    /// The position is the byte offset within the offending string so a
    /// caller can point at it.
    #[error("nbt: string contains byte 0x{byte:02x} at index {index}, not encodable here")]
    InvalidString {
        /// Offending byte.
        byte: u8,
        /// Byte index within the string.
        index: usize,
    },
    /// A [`List`] declared one element type but an item carried another.
    #[error("nbt: list declared element type {declared} but item {index} has type {actual}")]
    HeterogeneousList {
        /// Declared element type id.
        declared: u8,
        /// Index of the mismatching item.
        index: usize,
        /// Actual element type id of the mismatching item.
        actual: u8,
    },
    /// An NBT length prefix (i32 / u16) overflowed the on-wire width.
    #[error("nbt: {context} length {len} exceeds wire limit {limit}")]
    LengthOverflow {
        /// Which tag's length overflowed (`"string"`, `"byte_array"`,
        /// `"int_array"`, `"long_array"`, `"list"`).
        context: &'static str,
        /// Requested length.
        len: usize,
        /// Hard wire-level limit (`i32::MAX` or `u16::MAX`).
        limit: usize,
    },
    /// Underlying I/O failure (file write, gzip flush, ...).
    #[error("nbt I/O: {0}")]
    Io(#[from] std::io::Error),
}

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
    write_tag_id(writer, 10)?;
    write_string(writer, root_name)?;
    write_compound_body(writer, root)?;
    Ok(())
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

fn write_tag_id<W: Write>(w: &mut W, id: u8) -> Result<(), NbtIoError> {
    w.write_all(&[id])?;
    Ok(())
}

fn write_string<W: Write>(w: &mut W, s: &str) -> Result<(), NbtIoError> {
    // Java NBT String uses Modified UTF-8 with a u16 length prefix. The
    // encoder currently accepts ASCII only: every byte must be ASCII and
    // non-NUL. A supplementary-pair encoder is reserved for a future
    // extension once a registry-pack id is observed outside that range.
    for (index, &byte) in s.as_bytes().iter().enumerate() {
        if byte == 0 || byte >= 0x80 {
            return Err(NbtIoError::InvalidString { byte, index });
        }
    }
    let bytes = s.as_bytes();
    let len = u16::try_from(bytes.len()).map_err(|_| NbtIoError::LengthOverflow {
        context: "string",
        len: bytes.len(),
        limit: u16::MAX as usize,
    })?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(bytes)?;
    Ok(())
}

fn write_payload<W: Write>(w: &mut W, tag: &Tag) -> Result<(), NbtIoError> {
    match tag {
        Tag::Byte(v) => w.write_all(&v.to_be_bytes())?,
        Tag::Short(v) => w.write_all(&v.to_be_bytes())?,
        Tag::Int(v) => w.write_all(&v.to_be_bytes())?,
        Tag::Long(v) => w.write_all(&v.to_be_bytes())?,
        Tag::Float(v) => w.write_all(&v.to_be_bytes())?,
        Tag::Double(v) => w.write_all(&v.to_be_bytes())?,
        Tag::ByteArray(bytes) => {
            write_array_len(w, "byte_array", bytes.len())?;
            // i8 → u8 with the same bit pattern via `to_ne_bytes`; the NBT
            // wire treats both as raw bytes so the in-memory sign tag is
            // not part of the payload.
            for &b in bytes {
                w.write_all(&b.to_ne_bytes())?;
            }
        }
        Tag::String(s) => write_string(w, s)?,
        Tag::List(list) => write_list(w, list)?,
        Tag::Compound(c) => write_compound_body(w, c)?,
        Tag::IntArray(values) => {
            write_array_len(w, "int_array", values.len())?;
            for v in values {
                w.write_all(&v.to_be_bytes())?;
            }
        }
        Tag::LongArray(values) => {
            write_array_len(w, "long_array", values.len())?;
            for v in values {
                w.write_all(&v.to_be_bytes())?;
            }
        }
    }
    Ok(())
}

fn write_array_len<W: Write>(
    w: &mut W,
    context: &'static str,
    len: usize,
) -> Result<(), NbtIoError> {
    let len = i32::try_from(len).map_err(|_| NbtIoError::LengthOverflow {
        context,
        len,
        limit: i32::MAX as usize,
    })?;
    w.write_all(&len.to_be_bytes())?;
    Ok(())
}

fn write_list<W: Write>(w: &mut W, list: &List) -> Result<(), NbtIoError> {
    w.write_all(&[list.element_type_id])?;
    write_array_len(w, "list", list.items.len())?;
    for (index, item) in list.items.iter().enumerate() {
        let actual = item.type_id();
        if actual != list.element_type_id {
            return Err(NbtIoError::HeterogeneousList {
                declared: list.element_type_id,
                index,
                actual,
            });
        }
        write_payload(w, item)?;
    }
    Ok(())
}

fn write_compound_body<W: Write>(w: &mut W, c: &Compound) -> Result<(), NbtIoError> {
    for (name, tag) in &c.entries {
        write_tag_id(w, tag.type_id())?;
        write_string(w, name)?;
        write_payload(w, tag)?;
    }
    // TAG_End terminator.
    write_tag_id(w, 0)?;
    Ok(())
}
