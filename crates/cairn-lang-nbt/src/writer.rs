//! The NBT encoder both dialects share; they differ only in the byte order of multi-byte scalars.

use std::io::Write;

use crate::error::NbtIoError;
use crate::tag::{Compound, List, Tag, string_len};

/// The byte order of every multi-byte scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Endian {
    /// Java.
    Big,
    /// Bedrock.
    Little,
}

macro_rules! scalar_writers {
    ($($name:ident: $ty:ty),* $(,)?) => {
        impl Endian {
            $(
                fn $name<W: Write>(self, w: &mut W, v: $ty) -> std::io::Result<()> {
                    match self {
                        Endian::Big => w.write_all(&v.to_be_bytes()),
                        Endian::Little => w.write_all(&v.to_le_bytes()),
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
    endian: Endian,
    root_name: &str,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_tag_id(writer, 10)?;
    write_string(writer, endian, root_name)?;
    write_compound_body(writer, endian, root)?;
    Ok(())
}

pub(crate) fn write_tag_id<W: Write>(w: &mut W, id: u8) -> Result<(), NbtIoError> {
    w.write_all(&[id])?;
    Ok(())
}

pub(crate) fn write_string<W: Write>(w: &mut W, endian: Endian, s: &str) -> Result<(), NbtIoError> {
    let len = string_len(s)?;
    endian.write_u16(w, len)?;
    w.write_all(s.as_bytes())?;
    Ok(())
}

pub(crate) fn write_payload<W: Write>(
    w: &mut W,
    endian: Endian,
    tag: &Tag,
) -> Result<(), NbtIoError> {
    match tag {
        Tag::Byte(v) => w.write_all(&v.to_ne_bytes())?,
        Tag::Short(v) => endian.write_i16(w, *v)?,
        Tag::Int(v) => endian.write_i32(w, *v)?,
        Tag::Long(v) => endian.write_i64(w, *v)?,
        Tag::Float(v) => endian.write_f32(w, *v)?,
        Tag::Double(v) => endian.write_f64(w, *v)?,
        Tag::ByteArray(bytes) => {
            write_array_len(w, endian, "byte_array", bytes.len())?;
            // A single byte has no byte order.
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
    write_list_header(w, endian, list.element_type_id, list.items.len())?;
    for (index, item) in list.items.iter().enumerate() {
        check_item_type(list.element_type_id, index, item.type_id())?;
        write_payload(w, endian, item)?;
    }
    Ok(())
}

pub(crate) fn write_list_header<W: Write>(
    w: &mut W,
    endian: Endian,
    element_type_id: u8,
    len: usize,
) -> Result<(), NbtIoError> {
    if len == 0 && element_type_id != 0 {
        return Err(NbtIoError::EmptyListWithElementType {
            declared: element_type_id,
        });
    }
    w.write_all(&[element_type_id])?;
    write_array_len(w, endian, "list", len)
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
    endian: Endian,
    compound: &Compound,
) -> Result<(), NbtIoError> {
    for (name, tag) in &compound.entries {
        write_tag_id(w, tag.type_id())?;
        write_string(w, endian, name)?;
        write_payload(w, endian, tag)?;
    }
    // TAG_End terminator.
    write_tag_id(w, 0)?;
    Ok(())
}
