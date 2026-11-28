//! The NBT encoder both dialects share; they differ in the byte order of multi-byte scalars and
//! in how a string's characters are encoded.

use std::borrow::Cow;
use std::io::Write;

use crate::error::NbtIoError;
use crate::tag::{Compound, List, Tag};

/// An NBT dialect: Java's, big-endian with Modified UTF-8 strings, or Bedrock's, little-endian
/// with UTF-8 strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    /// Java.
    Java,
    /// Bedrock.
    Bedrock,
}

impl Dialect {
    /// The name of the encoding the dialect writes a string in.
    pub(crate) const fn string_encoding(self) -> &'static str {
        match self {
            Dialect::Java => "Modified UTF-8",
            Dialect::Bedrock => "UTF-8",
        }
    }
}

macro_rules! scalar_writers {
    ($($name:ident: $ty:ty),* $(,)?) => {
        impl Dialect {
            $(
                fn $name<W: Write>(self, w: &mut W, v: $ty) -> std::io::Result<()> {
                    match self {
                        Dialect::Java => w.write_all(&v.to_be_bytes()),
                        Dialect::Bedrock => w.write_all(&v.to_le_bytes()),
                    }
                }
            )*
        }
    };
}

scalar_writers! {
    write_u16: u16,
    write_i16: i16,
    write_i32: i32,
    write_i64: i64,
    write_f32: f32,
    write_f64: f64,
}

pub(crate) fn write_named_root<W: Write>(
    writer: &mut W,
    dialect: Dialect,
    root_name: &str,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_tag_id(writer, 10)?;
    write_string(writer, dialect, root_name)?;
    write_compound_body(writer, dialect, root)?;
    Ok(())
}

pub(crate) fn write_tag_id<W: Write>(w: &mut W, id: u8) -> Result<(), NbtIoError> {
    w.write_all(&[id])?;
    Ok(())
}

pub(crate) fn write_string<W: Write>(
    w: &mut W,
    dialect: Dialect,
    s: &str,
) -> Result<(), NbtIoError> {
    let bytes = encode(s, dialect);
    dialect.write_u16(w, length_prefix(bytes.len(), dialect)?)?;
    w.write_all(&bytes)?;
    Ok(())
}

/// Whether `dialect`'s writers accept `s`, without writing it.
pub(crate) fn check_string(s: &str, dialect: Dialect) -> Result<(), NbtIoError> {
    length_prefix(encode(s, dialect).len(), dialect).map(drop)
}

/// `s` in `dialect`'s string encoding.
fn encode(s: &str, dialect: Dialect) -> Cow<'_, [u8]> {
    if dialect == Dialect::Bedrock || !needs_modified_utf8(s) {
        return Cow::Borrowed(s.as_bytes());
    }
    let low = |bits: u16| bits.to_le_bytes()[0];
    let mut bytes = Vec::with_capacity(s.len() + s.len() / 2);
    for unit in s.encode_utf16() {
        match unit {
            0x0001..=0x007F => bytes.push(low(unit)),
            0x0000 | 0x0080..=0x07FF => {
                bytes.extend_from_slice(&[0xC0 | low(unit >> 6), 0x80 | low(unit & 0x3F)]);
            }
            _ => bytes.extend_from_slice(&[
                0xE0 | low(unit >> 12),
                0x80 | low((unit >> 6) & 0x3F),
                0x80 | low(unit & 0x3F),
            ]),
        }
    }
    Cow::Owned(bytes)
}

/// The prefix of a string whose encoding in `dialect` takes `len` bytes.
fn length_prefix(len: usize, dialect: Dialect) -> Result<u16, NbtIoError> {
    u16::try_from(len).map_err(|_| NbtIoError::StringTooLong {
        len,
        encoding: dialect.string_encoding(),
    })
}

/// Whether `s` holds a character Java's Modified UTF-8 writes apart from UTF-8: NUL, or one past
/// U+FFFF, which UTF-8 opens with a byte of `0xF0` or more.
fn needs_modified_utf8(s: &str) -> bool {
    s.bytes().any(|byte| byte == 0 || byte >= 0xF0)
}

pub(crate) fn write_payload<W: Write>(
    w: &mut W,
    dialect: Dialect,
    tag: &Tag,
) -> Result<(), NbtIoError> {
    match tag {
        Tag::Byte(v) => w.write_all(&v.to_ne_bytes())?,
        Tag::Short(v) => dialect.write_i16(w, *v)?,
        Tag::Int(v) => dialect.write_i32(w, *v)?,
        Tag::Long(v) => dialect.write_i64(w, *v)?,
        Tag::Float(v) => dialect.write_f32(w, *v)?,
        Tag::Double(v) => dialect.write_f64(w, *v)?,
        Tag::ByteArray(bytes) => {
            write_byte_array_payload(w, dialect, bytes.len(), bytes.iter().copied())?;
        }
        Tag::String(s) => write_string(w, dialect, s)?,
        Tag::List(list) => write_list(w, dialect, list)?,
        Tag::Compound(c) => write_compound_body(w, dialect, c)?,
        Tag::IntArray(values) => {
            write_array_len(w, dialect, "int_array", values.len())?;
            for v in values {
                dialect.write_i32(w, *v)?;
            }
        }
        Tag::LongArray(values) => {
            write_long_array_payload(w, dialect, values.len(), values.iter().copied())?;
        }
    }
    Ok(())
}

/// A `TAG_Long_Array` payload of the `len` values `values` yields.
///
/// # Panics
///
/// When `values` yields a number of values other than `len`.
pub(crate) fn write_long_array_payload<W: Write>(
    w: &mut W,
    dialect: Dialect,
    len: usize,
    values: impl IntoIterator<Item = i64>,
) -> Result<(), NbtIoError> {
    write_array_len(w, dialect, "long_array", len)?;
    write_counted(w, ("long array", len), values, |w, value| {
        dialect.write_i64(w, value)
    })
}

/// A `TAG_Byte_Array` payload of the `len` values `values` yields.
///
/// # Panics
///
/// When `values` yields a number of values other than `len`.
pub(crate) fn write_byte_array_payload<W: Write>(
    w: &mut W,
    dialect: Dialect,
    len: usize,
    values: impl IntoIterator<Item = i8>,
) -> Result<(), NbtIoError> {
    write_array_len(w, dialect, "byte_array", len)?;
    // A single byte has no byte order.
    write_counted(w, ("byte array", len), values, |w, value| {
        w.write_all(&value.to_ne_bytes())
    })
}

/// Writes exactly the `len` values of the array `kind` names, each with `write_one`.
fn write_counted<W: Write, T>(
    w: &mut W,
    (kind, len): (&str, usize),
    values: impl IntoIterator<Item = T>,
    mut write_one: impl FnMut(&mut W, T) -> std::io::Result<()>,
) -> Result<(), NbtIoError> {
    let mut values = values.into_iter();
    for written in 0..len {
        let value = values.next().unwrap_or_else(|| {
            panic!("a streamed {kind} declared {len} values and was given {written}")
        });
        write_one(w, value)?;
    }
    assert!(
        values.next().is_none(),
        "a streamed {kind} declared {len} values and was given more",
    );
    Ok(())
}

fn write_array_len<W: Write>(
    w: &mut W,
    dialect: Dialect,
    context: &'static str,
    len: usize,
) -> Result<(), NbtIoError> {
    let len = i32::try_from(len).map_err(|_| NbtIoError::LengthOverflow {
        context,
        len,
        limit: i32::MAX as usize,
    })?;
    dialect.write_i32(w, len)?;
    Ok(())
}

fn write_list<W: Write>(w: &mut W, dialect: Dialect, list: &List) -> Result<(), NbtIoError> {
    write_list_header(w, dialect, list.element_type_id, list.items.len())?;
    for (index, item) in list.items.iter().enumerate() {
        check_item_type(list.element_type_id, index, item.type_id())?;
        write_payload(w, dialect, item)?;
    }
    Ok(())
}

pub(crate) fn write_list_header<W: Write>(
    w: &mut W,
    dialect: Dialect,
    element_type_id: u8,
    len: usize,
) -> Result<(), NbtIoError> {
    if len == 0 && element_type_id != 0 {
        return Err(NbtIoError::EmptyListWithElementType {
            declared: element_type_id,
        });
    }
    w.write_all(&[element_type_id])?;
    write_array_len(w, dialect, "list", len)
}

pub(crate) fn check_item_type(declared: u8, index: usize, actual: u8) -> Result<(), NbtIoError> {
    if actual == declared {
        Ok(())
    } else {
        Err(NbtIoError::HeterogeneousList {
            declared,
            index,
            actual,
        })
    }
}

pub(crate) fn write_compound_body<W: Write>(
    w: &mut W,
    dialect: Dialect,
    compound: &Compound,
) -> Result<(), NbtIoError> {
    for (name, tag) in &compound.entries {
        write_tag_id(w, tag.type_id())?;
        write_string(w, dialect, name)?;
        write_payload(w, dialect, tag)?;
    }
    // TAG_End terminator.
    write_tag_id(w, 0)?;
    Ok(())
}
