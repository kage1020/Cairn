//! Streaming NBT encoder: write a root compound entry by entry instead of
//! building the whole [`Tag`] tree first.
//!
//! The tree writers in [`crate::java`] and [`crate::bedrock`] need every
//! tag in memory before the first byte goes out, which is the right shape
//! for a palette and the wrong one for a list with an entry per voxel. A
//! [`CompoundStream`] writes each named tag as it is handed one, and a
//! [`ListStream`] writes each item the same way, so a caller can encode a
//! list of millions of items while holding one at a time.
//!
//! The byte stream is the one the tree writers produce for the same tags in
//! the same order: both go through the crate-internal `writer` core, which
//! owns string validation, length prefixes and byte order.
//!
//! A list's length prefix is written before its first item, so the item
//! count is declared up front and checked when the list's closure returns.
//! Writing a different number of items than declared is a bug in the caller
//! rather than a property of the data, and it panics: the prefix is already
//! on the wire, and no error value could make the bytes behind it valid.

use std::io::Write;

use crate::tag::Tag;
use crate::writer::{
    Endian, NbtIoError, write_array_len, write_payload, write_string, write_tag_id,
};

/// `TAG_List`'s wire id.
const LIST_ID: u8 = 9;
/// `TAG_Compound`'s wire id.
const COMPOUND_ID: u8 = 10;

/// The body of a compound being written: each call appends one named tag,
/// and the `TAG_End` terminator is written when the closure that received
/// this stream returns.
pub struct CompoundStream<'w, W: Write> {
    writer: &'w mut W,
    endian: Endian,
}

/// The items of a list being written, after its element type and length
/// prefix. Each call appends one item, which must carry the declared element
/// type.
pub struct ListStream<'w, W: Write> {
    writer: &'w mut W,
    endian: Endian,
    element_type_id: u8,
    len: usize,
    written: usize,
}

/// Write a root-level named compound whose body `body` streams, in the
/// given byte order.
pub(crate) fn stream_named_root<W, F>(
    writer: &mut W,
    endian: Endian,
    root_name: &str,
    body: F,
) -> Result<(), NbtIoError>
where
    W: Write,
    F: FnOnce(&mut CompoundStream<'_, W>) -> Result<(), NbtIoError>,
{
    write_tag_id(writer, COMPOUND_ID)?;
    write_string(writer, endian, root_name)?;
    stream_compound_body(writer, endian, body)
}

fn stream_compound_body<W, F>(writer: &mut W, endian: Endian, body: F) -> Result<(), NbtIoError>
where
    W: Write,
    F: FnOnce(&mut CompoundStream<'_, W>) -> Result<(), NbtIoError>,
{
    let mut compound = CompoundStream { writer, endian };
    body(&mut compound)?;
    write_tag_id(compound.writer, 0)
}

/// Write a list's element type and length prefix, stream its items, and
/// check the count. `body` runs even for an empty list, so a caller that
/// writes an item into one is caught the same way as any other miscount.
fn stream_list_body<W, F>(
    writer: &mut W,
    endian: Endian,
    element_type_id: u8,
    len: usize,
    body: F,
) -> Result<(), NbtIoError>
where
    W: Write,
    F: FnOnce(&mut ListStream<'_, W>) -> Result<(), NbtIoError>,
{
    if len == 0 && element_type_id != 0 {
        return Err(NbtIoError::EmptyListWithElementType {
            declared: element_type_id,
        });
    }
    writer.write_all(&[element_type_id])?;
    write_array_len(writer, endian, "list", len)?;
    let mut list = ListStream {
        writer,
        endian,
        element_type_id,
        len,
        written: 0,
    };
    body(&mut list)?;
    assert_eq!(
        list.written, list.len,
        "a streamed list declared {} items and was given {}",
        list.len, list.written,
    );
    Ok(())
}

impl<W: Write> CompoundStream<'_, W> {
    /// Append one named tag, encoded exactly as the tree writers encode a
    /// compound entry.
    ///
    /// # Errors
    ///
    /// Any encoding error the tag raises, and I/O failure on the writer.
    pub fn tag(&mut self, name: &str, tag: &Tag) -> Result<(), NbtIoError> {
        write_tag_id(self.writer, tag.type_id())?;
        write_string(self.writer, self.endian, name)?;
        write_payload(self.writer, self.endian, tag)
    }

    /// Append a named compound whose body `body` streams.
    ///
    /// # Errors
    ///
    /// Whatever `body` returns, and I/O failure on the writer.
    pub fn compound<F>(&mut self, name: &str, body: F) -> Result<(), NbtIoError>
    where
        F: FnOnce(&mut CompoundStream<'_, W>) -> Result<(), NbtIoError>,
    {
        write_tag_id(self.writer, COMPOUND_ID)?;
        write_string(self.writer, self.endian, name)?;
        stream_compound_body(self.writer, self.endian, body)
    }

    /// Append a named list of `len` items of type `element_type_id`, which
    /// `body` streams.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::EmptyListWithElementType`] for an empty list declaring
    /// an element type other than `TAG_End`, a length past the wire limit,
    /// whatever `body` returns, and I/O failure on the writer.
    ///
    /// # Panics
    ///
    /// Panics when `body` writes a number of items other than `len`.
    pub fn list<F>(
        &mut self,
        name: &str,
        element_type_id: u8,
        len: usize,
        body: F,
    ) -> Result<(), NbtIoError>
    where
        F: FnOnce(&mut ListStream<'_, W>) -> Result<(), NbtIoError>,
    {
        write_tag_id(self.writer, LIST_ID)?;
        write_string(self.writer, self.endian, name)?;
        stream_list_body(self.writer, self.endian, element_type_id, len, body)
    }
}

impl<W: Write> ListStream<'_, W> {
    /// Count one more item, refusing it when the list is already full.
    fn claim(&mut self, actual: u8) -> Result<(), NbtIoError> {
        assert!(
            self.written < self.len,
            "a streamed list declared {} items and was given more",
            self.len,
        );
        if actual != self.element_type_id {
            return Err(NbtIoError::HeterogeneousList {
                declared: self.element_type_id,
                index: self.written,
                actual,
            });
        }
        self.written += 1;
        Ok(())
    }

    /// Append one item.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::HeterogeneousList`] when the item's type is not the
    /// declared element type, any encoding error the item raises, and I/O
    /// failure on the writer.
    ///
    /// # Panics
    ///
    /// Panics when the list already holds the `len` items it declared.
    pub fn item(&mut self, tag: &Tag) -> Result<(), NbtIoError> {
        self.claim(tag.type_id())?;
        write_payload(self.writer, self.endian, tag)
    }

    /// Append one compound item whose body `body` streams.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::HeterogeneousList`] when the list does not hold
    /// compounds, whatever `body` returns, and I/O failure on the writer.
    ///
    /// # Panics
    ///
    /// Panics when the list already holds the `len` items it declared.
    pub fn compound<F>(&mut self, body: F) -> Result<(), NbtIoError>
    where
        F: FnOnce(&mut CompoundStream<'_, W>) -> Result<(), NbtIoError>,
    {
        self.claim(COMPOUND_ID)?;
        stream_compound_body(self.writer, self.endian, body)
    }

    /// Append one list item of `len` items of type `element_type_id`, which
    /// `body` streams.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::HeterogeneousList`] when this list does not hold lists,
    /// [`NbtIoError::EmptyListWithElementType`] for an empty inner list
    /// declaring an element type other than `TAG_End`, whatever `body`
    /// returns, and I/O failure on the writer.
    ///
    /// # Panics
    ///
    /// Panics when this list already holds the items it declared, or when
    /// `body` writes a number of items other than `len`.
    pub fn list<F>(&mut self, element_type_id: u8, len: usize, body: F) -> Result<(), NbtIoError>
    where
        F: FnOnce(&mut ListStream<'_, W>) -> Result<(), NbtIoError>,
    {
        self.claim(LIST_ID)?;
        stream_list_body(self.writer, self.endian, element_type_id, len, body)
    }
}
