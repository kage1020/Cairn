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
//! count is declared up front and checked when the list's closure returns
//! successfully. Writing a different number of items than declared panics.
//! A wrong item type returns [`NbtIoError::HeterogeneousList`] instead, and
//! the difference is whether the stream can still be finished. A wrong type
//! is refused before the item is counted and before any byte of it is
//! written, so the bytes so far are still a valid prefix and a retry with
//! the right type continues the list. A short list is found only once its
//! closure has returned, when the prefix promises items that will not
//! follow. A long one is found at the extra item, before any of its bytes,
//! but it is the same bug: a declared length that the caller's own loop
//! disagrees with. Like any input the caller built wrong, both assert.
//!
//! An empty list always declares `TAG_End`, whatever element type the
//! caller names, as [`List::of_tags`](crate::tag::List::of_tags) does.
//!
//! A [`CompoundStream`] writes every entry it is given, so naming one key
//! twice writes it twice. A [`Compound`]'s map keeps only the last, so such
//! a stream has no tree that writes the same bytes; the caller keeps keys
//! unique.

use std::io::Write;

use crate::tag::{Compound, Tag};
use crate::writer::{
    Endian, NbtIoError, check_item_type, write_compound_body, write_list_header, write_payload,
    write_string, write_tag_id,
};

/// `TAG_List`'s wire id.
const LIST_ID: u8 = 9;
/// `TAG_Compound`'s wire id.
const COMPOUND_ID: u8 = 10;

/// The body of a compound being written: each call appends one named tag,
/// and the `TAG_End` terminator is written when the closure that received
/// this stream returns successfully.
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
/// check the count. An empty list declares `TAG_End` whatever
/// `element_type_id` says, as `List::of_tags` does. `body` runs even for
/// an empty list, so a caller that writes an item into one is caught the
/// same way as any other miscount.
///
/// The count check runs only after `body` returns `Ok`. An error from
/// `body` (a failed write, an unencodable item) returns before it, so the
/// caller gets that error rather than a panic about the count it left
/// short.
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
    let element_type_id = if len == 0 { 0 } else { element_type_id };
    write_list_header(writer, endian, element_type_id, len)?;
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
    /// [`NbtIoError::InvalidString`] or [`NbtIoError::LengthOverflow`] when
    /// `name` cannot be written as an NBT string, any encoding error the
    /// tag raises, and I/O failure on the writer.
    pub fn tag(&mut self, name: &str, tag: &Tag) -> Result<(), NbtIoError> {
        write_tag_id(self.writer, tag.type_id())?;
        write_string(self.writer, self.endian, name)?;
        write_payload(self.writer, self.endian, tag)
    }

    /// Append a named compound the caller already holds, without wrapping
    /// it in a [`Tag`]: the bytes [`Self::tag`] writes for
    /// `Tag::Compound(value.clone())`, without the clone.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::InvalidString`] or [`NbtIoError::LengthOverflow`] when
    /// `name` cannot be written as an NBT string, any encoding error the
    /// compound raises, and I/O failure on the writer.
    pub fn compound_tag(&mut self, name: &str, value: &Compound) -> Result<(), NbtIoError> {
        write_tag_id(self.writer, COMPOUND_ID)?;
        write_string(self.writer, self.endian, name)?;
        write_compound_body(self.writer, self.endian, value)
    }

    /// Append a named compound whose body `body` streams.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::InvalidString`] or [`NbtIoError::LengthOverflow`] when
    /// `name` cannot be written as an NBT string, whatever `body` returns,
    /// and I/O failure on the writer.
    ///
    /// # Panics
    ///
    /// Panics when a list `body` streams is given a number of items other
    /// than it declared.
    pub fn compound<F>(&mut self, name: &str, body: F) -> Result<(), NbtIoError>
    where
        F: FnOnce(&mut CompoundStream<'_, W>) -> Result<(), NbtIoError>,
    {
        write_tag_id(self.writer, COMPOUND_ID)?;
        write_string(self.writer, self.endian, name)?;
        stream_compound_body(self.writer, self.endian, body)
    }

    /// Append a named list of `len` items of type `element_type_id`, which
    /// `body` streams. When `len` is `0` the list declares `TAG_End`
    /// whatever `element_type_id` says, as `List::of_tags` does.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::InvalidString`] or [`NbtIoError::LengthOverflow`] when
    /// `name` cannot be written as an NBT string,
    /// [`NbtIoError::LengthOverflow`] for a `len` past the `i32` wire
    /// limit, whatever `body` returns, and I/O failure on the writer.
    ///
    /// # Panics
    ///
    /// Panics when `body` writes more than `len` items, or returns `Ok`
    /// having written fewer.
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
    /// Count one more item of type `actual`, refusing it when its type is
    /// not the declared one. A refused item is not counted, and the caller
    /// writes no byte of it.
    fn claim(&mut self, actual: u8) -> Result<(), NbtIoError> {
        assert!(
            self.written < self.len,
            "a streamed list declared {} items and was given more",
            self.len,
        );
        check_item_type(self.element_type_id, self.written, actual)?;
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
    /// Panics when the list already holds the `len` items it declared, or
    /// when a list `body` streams is given a number of items other than it
    /// declared.
    pub fn compound<F>(&mut self, body: F) -> Result<(), NbtIoError>
    where
        F: FnOnce(&mut CompoundStream<'_, W>) -> Result<(), NbtIoError>,
    {
        self.claim(COMPOUND_ID)?;
        stream_compound_body(self.writer, self.endian, body)
    }

    /// Append one list item of `len` items of type `element_type_id`, which
    /// `body` streams. When `len` is `0` the inner list declares `TAG_End`
    /// whatever `element_type_id` says, as `List::of_tags` does.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::HeterogeneousList`] when this list does not hold lists,
    /// [`NbtIoError::LengthOverflow`] for a `len` past the `i32` wire
    /// limit, whatever `body` returns, and I/O failure on the writer.
    ///
    /// # Panics
    ///
    /// Panics when this list already holds the items it declared, or when
    /// `body` writes more than `len` items or returns `Ok` having written
    /// fewer.
    pub fn list<F>(&mut self, element_type_id: u8, len: usize, body: F) -> Result<(), NbtIoError>
    where
        F: FnOnce(&mut ListStream<'_, W>) -> Result<(), NbtIoError>,
    {
        self.claim(LIST_ID)?;
        stream_list_body(self.writer, self.endian, element_type_id, len, body)
    }
}
