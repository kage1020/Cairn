//! Endian-parameterised NBT writer core shared by [`crate::java`] and
//! [`crate::bedrock`].
//!
//! The two on-disk dialects are byte-identical in structure — tag ids,
//! nesting, length-prefix widths, validation rules — and differ only in
//! the byte order of multi-byte scalars (numeric payloads and the u16 /
//! i32 length prefixes). Keeping one implementation here means a
//! validation fix (string encoding, list homogeneity, length overflow)
//! can never land on one edition and silently miss the other.

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
    /// A [`List`] with no items declared an element type other than
    /// `TAG_End`.
    ///
    /// The spec fixes the id at `0` for an empty list, and the sibling
    /// check above cannot see this one: it runs inside the item loop,
    /// which an empty list never enters. `List`'s fields are public, so a
    /// constructor is a convenience and not a funnel — this is the point
    /// every byte passes through.
    #[error("nbt: empty list declared element type {declared}, expected 0 (TAG_End)")]
    EmptyListWithElementType {
        /// Declared element type id.
        declared: u8,
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

/// Byte order of every multi-byte scalar in the output stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Endian {
    /// Java on-disk NBT (big-endian).
    Big,
    /// Bedrock on-disk NBT (little-endian).
    Little,
}

impl Endian {
    fn write_u16<W: Write>(self, w: &mut W, v: u16) -> std::io::Result<()> {
        match self {
            Endian::Big => w.write_all(&v.to_be_bytes()),
            Endian::Little => w.write_all(&v.to_le_bytes()),
        }
    }

    fn write_i16<W: Write>(self, w: &mut W, v: i16) -> std::io::Result<()> {
        match self {
            Endian::Big => w.write_all(&v.to_be_bytes()),
            Endian::Little => w.write_all(&v.to_le_bytes()),
        }
    }

    fn write_i32<W: Write>(self, w: &mut W, v: i32) -> std::io::Result<()> {
        match self {
            Endian::Big => w.write_all(&v.to_be_bytes()),
            Endian::Little => w.write_all(&v.to_le_bytes()),
        }
    }

    fn write_i64<W: Write>(self, w: &mut W, v: i64) -> std::io::Result<()> {
        match self {
            Endian::Big => w.write_all(&v.to_be_bytes()),
            Endian::Little => w.write_all(&v.to_le_bytes()),
        }
    }

    fn write_f32<W: Write>(self, w: &mut W, v: f32) -> std::io::Result<()> {
        match self {
            Endian::Big => w.write_all(&v.to_be_bytes()),
            Endian::Little => w.write_all(&v.to_le_bytes()),
        }
    }

    fn write_f64<W: Write>(self, w: &mut W, v: f64) -> std::io::Result<()> {
        match self {
            Endian::Big => w.write_all(&v.to_be_bytes()),
            Endian::Little => w.write_all(&v.to_le_bytes()),
        }
    }
}

/// Write a root-level named compound in the given byte order.
pub(crate) fn write_named_root<W: Write>(
    writer: &mut W,
    endian: Endian,
    root_name: &str,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_tag_id(writer, 10)?;
    write_string(writer, endian, root_name)?;
    write_compound_body(writer, endian, root)?;
    Ok(())
}

fn write_tag_id<W: Write>(w: &mut W, id: u8) -> Result<(), NbtIoError> {
    w.write_all(&[id])?;
    Ok(())
}

fn write_string<W: Write>(w: &mut W, endian: Endian, s: &str) -> Result<(), NbtIoError> {
    // Java NBT String uses Modified UTF-8; Bedrock uses plain UTF-8. Both
    // carry a u16 length prefix. The accepted byte set matches what
    // `NbtIoError::InvalidString` documents — ASCII and non-NUL only — a
    // range on which the two encodings coincide, so one validator covers
    // both dialects.
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
    endian.write_u16(w, len)?;
    w.write_all(bytes)?;
    Ok(())
}

fn write_payload<W: Write>(w: &mut W, endian: Endian, tag: &Tag) -> Result<(), NbtIoError> {
    match tag {
        Tag::Byte(v) => w.write_all(&v.to_ne_bytes())?,
        Tag::Short(v) => endian.write_i16(w, *v)?,
        Tag::Int(v) => endian.write_i32(w, *v)?,
        Tag::Long(v) => endian.write_i64(w, *v)?,
        Tag::Float(v) => endian.write_f32(w, *v)?,
        Tag::Double(v) => endian.write_f64(w, *v)?,
        Tag::ByteArray(bytes) => {
            write_array_len(w, endian, "byte_array", bytes.len())?;
            // i8 → u8 with the same bit pattern via `to_ne_bytes`; the NBT
            // wire treats both as raw bytes so the in-memory sign tag is
            // not part of the payload.
            for &b in bytes {
                w.write_all(&b.to_ne_bytes())?;
            }
        }
        Tag::String(s) => write_string(w, endian, s)?,
        Tag::List(list) => write_list(w, endian, list)?,
        Tag::Compound(c) => write_compound_body(w, endian, c)?,
        Tag::IntArray(values) => {
            write_array_len(w, endian, "int_array", values.len())?;
            for v in values {
                endian.write_i32(w, *v)?;
            }
        }
        Tag::LongArray(values) => {
            write_array_len(w, endian, "long_array", values.len())?;
            for v in values {
                endian.write_i64(w, *v)?;
            }
        }
    }
    Ok(())
}

fn write_array_len<W: Write>(
    w: &mut W,
    endian: Endian,
    context: &'static str,
    len: usize,
) -> Result<(), NbtIoError> {
    let len = i32::try_from(len).map_err(|_| NbtIoError::LengthOverflow {
        context,
        len,
        limit: i32::MAX as usize,
    })?;
    endian.write_i32(w, len)?;
    Ok(())
}

fn write_list<W: Write>(w: &mut W, endian: Endian, list: &List) -> Result<(), NbtIoError> {
    if list.items.is_empty() && list.element_type_id != 0 {
        return Err(NbtIoError::EmptyListWithElementType {
            declared: list.element_type_id,
        });
    }
    w.write_all(&[list.element_type_id])?;
    write_array_len(w, endian, "list", list.items.len())?;
    for (index, item) in list.items.iter().enumerate() {
        let actual = item.type_id();
        if actual != list.element_type_id {
            return Err(NbtIoError::HeterogeneousList {
                declared: list.element_type_id,
                index,
                actual,
            });
        }
        write_payload(w, endian, item)?;
    }
    Ok(())
}

fn write_compound_body<W: Write>(
    w: &mut W,
    endian: Endian,
    c: &Compound,
) -> Result<(), NbtIoError> {
    for (name, tag) in &c.entries {
        write_tag_id(w, tag.type_id())?;
        write_string(w, endian, name)?;
        write_payload(w, endian, tag)?;
    }
    // TAG_End terminator.
    write_tag_id(w, 0)?;
    Ok(())
}
