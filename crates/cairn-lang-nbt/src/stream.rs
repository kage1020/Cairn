//! Streaming NBT encoder: writes a root entry by entry, in the bytes the tree writers produce for
//! the same tags. A list declares its length up front, so a miscount panics, and the caller keeps
//! compound keys unique.

use std::io::Write;

use crate::error::NbtIoError;
use crate::tag::{Compound, Tag};
use crate::writer::{
    Endian, check_item_type, write_compound_body, write_list_header, write_payload, write_string,
    write_tag_id,
};

const LIST_ID: u8 = 9;
const COMPOUND_ID: u8 = 10;

/// A compound being written, terminated when the closure that received it returns `Ok`.
pub struct CompoundStream<'w, W: Write> {
    writer: &'w mut W,
    endian: Endian,
}

/// A list being written after its header; each item must carry the declared element type.
pub struct ListStream<'w, W: Write> {
    writer: &'w mut W,
    endian: Endian,
    element_type_id: u8,
    len: usize,
    written: usize,
}

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

/// `body` runs even when `len` is 0, so an item written into an empty list panics as a miscount.
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
    /// Appends one named tag.
    ///
    /// # Errors
    ///
    /// A `name` that is not a valid NBT string, a tag the encoder rejects, or an I/O failure.
    pub fn tag(&mut self, name: &str, tag: &Tag) -> Result<(), NbtIoError> {
        write_tag_id(self.writer, tag.type_id())?;
        write_string(self.writer, self.endian, name)?;
        write_payload(self.writer, self.endian, tag)
    }

    /// Appends a named compound without cloning it into a [`Tag`].
    ///
    /// # Errors
    ///
    /// A `name` that is not a valid NBT string, a tag the encoder rejects, or an I/O failure.
    pub fn compound_tag(&mut self, name: &str, value: &Compound) -> Result<(), NbtIoError> {
        write_tag_id(self.writer, COMPOUND_ID)?;
        write_string(self.writer, self.endian, name)?;
        write_compound_body(self.writer, self.endian, value)
    }

    /// Appends a named compound whose entries `body` streams.
    ///
    /// # Errors
    ///
    /// A `name` that is not a valid NBT string, `body`'s error, or an I/O failure.
    ///
    /// # Panics
    ///
    /// When a list `body` streams gets a different number of items than it declared.
    pub fn compound<F>(&mut self, name: &str, body: F) -> Result<(), NbtIoError>
    where
        F: FnOnce(&mut CompoundStream<'_, W>) -> Result<(), NbtIoError>,
    {
        write_tag_id(self.writer, COMPOUND_ID)?;
        write_string(self.writer, self.endian, name)?;
        stream_compound_body(self.writer, self.endian, body)
    }

    /// Appends a named list of `len` items that `body` streams; an empty one declares `TAG_End`.
    ///
    /// # Errors
    ///
    /// An invalid `name`, a `len` past `i32::MAX`, `body`'s error, or an I/O failure.
    ///
    /// # Panics
    ///
    /// When `body` writes a number of items other than `len`.
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
    /// Counts one more item, refusing a wrong type before the caller writes any of its bytes.
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

    /// Appends one item.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::HeterogeneousList`] for a wrong type, a tag the encoder rejects, or I/O.
    ///
    /// # Panics
    ///
    /// When the list already holds the items it declared.
    pub fn item(&mut self, tag: &Tag) -> Result<(), NbtIoError> {
        self.claim(tag.type_id())?;
        write_payload(self.writer, self.endian, tag)
    }

    /// Appends one compound item whose entries `body` streams.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::HeterogeneousList`] for a list not of compounds, `body`'s error, or I/O.
    ///
    /// # Panics
    ///
    /// When the list is already full, or a list `body` streams gets the wrong number of items.
    pub fn compound<F>(&mut self, body: F) -> Result<(), NbtIoError>
    where
        F: FnOnce(&mut CompoundStream<'_, W>) -> Result<(), NbtIoError>,
    {
        self.claim(COMPOUND_ID)?;
        stream_compound_body(self.writer, self.endian, body)
    }

    /// Appends one list item of `len` items that `body` streams; an empty one declares `TAG_End`.
    ///
    /// # Errors
    ///
    /// [`NbtIoError::HeterogeneousList`] or a `len` past `i32::MAX`, `body`'s error, or I/O.
    ///
    /// # Panics
    ///
    /// When this list is already full, or `body` writes a number of items other than `len`.
    pub fn list<F>(&mut self, element_type_id: u8, len: usize, body: F) -> Result<(), NbtIoError>
    where
        F: FnOnce(&mut ListStream<'_, W>) -> Result<(), NbtIoError>,
    {
        self.claim(LIST_ID)?;
        stream_list_body(self.writer, self.endian, element_type_id, len, body)
    }
}
