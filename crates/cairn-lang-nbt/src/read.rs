//! NBT reader: a pull parser that yields a root's tags one at a time, so a list with an entry per
//! voxel never has to be in memory, and tree readers built on it.

use std::io::{ErrorKind, Read};

pub use crate::error::NbtReadError;
use crate::tag::{Compound, List, Tag};
use crate::writer::Endian;

/// The deepest nesting of compounds and lists the reader accepts, the game's own limit.
pub const MAX_DEPTH: usize = 512;

/// The most items or bytes a declared length reserves memory for before any of them is read.
const PREALLOCATE_AT_MOST: usize = 1 << 16;

macro_rules! scalar_readers {
    ($($name:ident: $ty:ty),* $(,)?) => {
        impl Endian {
            $(
                fn $name(self, bytes: [u8; size_of::<$ty>()]) -> $ty {
                    match self {
                        Endian::Big => <$ty>::from_be_bytes(bytes),
                        Endian::Little => <$ty>::from_le_bytes(bytes),
                    }
                }
            )*
        }
    };
}

scalar_readers! {
    u16: u16,
    i16: i16,
    i32: i32,
    i64: i64,
    f32: f32,
    f64: f64,
}

const END_ID: u8 = 0;
const LIST_ID: u8 = 9;
const COMPOUND_ID: u8 = 10;

/// One step through a root, in the order its bytes run.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A compound opens; its entries follow, then its [`Event::End`].
    Compound {
        /// The entry's name, or `None` for a list item.
        name: Option<String>,
    },
    /// A list opens; its `len` items follow, then its [`Event::End`].
    List {
        /// The entry's name, or `None` for a list item.
        name: Option<String>,
        /// The type id every item carries, `0` for an empty list.
        element_type_id: u8,
        /// How many items follow.
        len: usize,
    },
    /// A tag that is neither a compound nor a list, arrays included.
    Value {
        /// The entry's name, or `None` for a list item.
        name: Option<String>,
        /// The tag.
        tag: Tag,
    },
    /// The innermost open compound or list closes.
    End,
}

/// A compound or list the reader is inside.
#[derive(Debug, Clone, Copy)]
enum Frame {
    Compound,
    List {
        element_type_id: u8,
        remaining: usize,
    },
}

/// Where a reader is in its one root.
#[derive(Debug, Clone, Copy)]
enum State {
    Start,
    Open,
    Closed,
    /// A call failed with the reader at `offset`, so the bytes after it are misaligned.
    Failed {
        offset: u64,
    },
}

/// A pull parser over one root: [`Self::next_event`] yields its [`Event`]s,
/// [`Self::read_rest`] or [`Self::skip_rest`] take what is left of the innermost open compound or
/// list at once, and [`Self::finish`] checks that nothing follows the root.
#[derive(Debug)]
pub struct NbtReader<R: Read> {
    reader: R,
    endian: Endian,
    dialect: Dialect,
    offset: u64,
    stack: Vec<Frame>,
    state: State,
    max_list_len: usize,
    max_array_len: usize,
    /// The memory left for the call of [`Self::read_rest_within`] under way, if one is.
    holding: Option<Held>,
}

/// The memory a call of [`NbtReader::read_rest_within`] may still take, and has taken.
#[derive(Debug, Clone, Copy)]
struct Held {
    limit: usize,
    taken: usize,
}

/// How strings are encoded, which is where the two dialects differ beyond byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    /// Big-endian, Modified UTF-8.
    Java,
    /// Little-endian, UTF-8.
    Bedrock,
}

/// What a payload read produced: the tag, or nothing when it was skipped.
enum Payload {
    Tag(Tag),
    Skipped,
}

impl<R: Read> NbtReader<R> {
    /// A reader of Java NBT, uncompressed; wrap a gzip-wrapped file in a decoder first.
    pub fn java(reader: R) -> Self {
        Self::new(reader, Dialect::Java)
    }

    /// A reader of Bedrock NBT, the `.mcstructure` form.
    pub fn bedrock(reader: R) -> Self {
        Self::new(reader, Dialect::Bedrock)
    }

    fn new(reader: R, dialect: Dialect) -> Self {
        let endian = match dialect {
            Dialect::Java => Endian::Big,
            Dialect::Bedrock => Endian::Little,
        };
        Self {
            reader,
            endian,
            dialect,
            offset: 0,
            stack: Vec::new(),
            state: State::Start,
            max_list_len: usize::MAX,
            max_array_len: usize::MAX,
            holding: None,
        }
    }

    /// This reader, refusing any list or array that declares more than `max_len` items.
    #[must_use]
    pub fn with_max_len(mut self, max_len: usize) -> Self {
        self.max_list_len = max_len;
        self.max_array_len = max_len;
        self
    }

    /// This reader, refusing any array that declares more than `max_len` elements, while lists
    /// keep the limit [`Self::with_max_len`] gave them.
    #[must_use]
    pub fn with_max_array_len(mut self, max_len: usize) -> Self {
        self.max_array_len = max_len;
        self
    }

    /// How many compounds and lists are open.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// How many bytes of the uncompressed stream the reader has consumed.
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// The next event, or `None` once the root has closed; the root is a named compound.
    ///
    /// # Errors
    ///
    /// An [`NbtReadError`] for bytes that are not NBT or end inside the root, and
    /// [`NbtReadError::AfterFailure`] for every call after one that failed.
    pub fn next_event(&mut self) -> Result<Option<Event>, NbtReadError> {
        self.step(false)
    }

    /// What is left of the innermost open compound or list, read to its end as one tag.
    ///
    /// # Errors
    ///
    /// Those of [`Self::next_event`].
    ///
    /// # Panics
    ///
    /// When no compound or list is open.
    pub fn read_rest(&mut self) -> Result<Tag, NbtReadError> {
        self.read_rest_within(usize::MAX).map(|(tag, _)| tag)
    }

    /// [`Self::read_rest`], refused once the tags it builds would take more than `max_bytes` of
    /// memory; returns the tag and the memory, about, that it takes.
    ///
    /// # Errors
    ///
    /// Those of [`Self::next_event`], and [`NbtReadError::HeldTooMuch`] past `max_bytes`.
    ///
    /// # Panics
    ///
    /// When no compound or list is open.
    pub fn read_rest_within(&mut self, max_bytes: usize) -> Result<(Tag, usize), NbtReadError> {
        self.holding = Some(Held {
            limit: max_bytes,
            taken: 0,
        });
        let tag = self.build_rest();
        let held = self.holding.take().expect("held until the build ends");
        Ok((tag?, held.taken))
    }

    /// What is left of the innermost open compound or list, built as one tag.
    fn build_rest(&mut self) -> Result<Tag, NbtReadError> {
        let mut open = vec![
            match self.stack.last().expect("a compound or list is open") {
                Frame::Compound => Partial::Compound(None, Compound::new()),
                Frame::List {
                    element_type_id, ..
                } => Partial::List(None, *element_type_id, Vec::new()),
            },
        ];
        loop {
            let event = self
                .next_event()?
                .expect("an open compound or list ends before the root does");
            let finished = match event {
                Event::Compound { name } => {
                    open.push(Partial::Compound(name, Compound::new()));
                    continue;
                }
                Event::List {
                    name,
                    element_type_id,
                    len,
                } => {
                    let reserved = len.min(PREALLOCATE_AT_MOST);
                    self.hold(reserved.saturating_mul(size_of::<Tag>()))?;
                    let items = Vec::with_capacity(reserved);
                    open.push(Partial::List(name, element_type_id, items));
                    continue;
                }
                Event::Value { name, tag } => {
                    if let Tag::String(text) = &tag {
                        self.hold(text.len())?;
                    }
                    (name, tag)
                }
                Event::End => {
                    let done = open.pop().expect("one partial per open container");
                    let finished = done.finish();
                    if open.is_empty() {
                        return Ok(finished.1);
                    }
                    finished
                }
            };
            let into = open
                .last_mut()
                .expect("a value belongs to an open container");
            self.hold(into.room_for(&finished))?;
            into.push(finished);
        }
    }

    /// Takes `bytes` more of the memory the call of [`Self::read_rest_within`] under way may take.
    fn hold(&mut self, bytes: usize) -> Result<(), NbtReadError> {
        let Some(held) = &mut self.holding else {
            return Ok(());
        };
        held.taken = held.taken.saturating_add(bytes);
        if held.taken <= held.limit {
            return Ok(());
        }
        let refusal = NbtReadError::HeldTooMuch {
            limit: held.limit,
            offset: self.offset,
        };
        self.state = State::Failed {
            offset: self.offset,
        };
        Err(refusal)
    }

    /// Skips what is left of the innermost open compound or list, holding none of its arrays.
    ///
    /// # Errors
    ///
    /// Those of [`Self::next_event`].
    ///
    /// # Panics
    ///
    /// When no compound or list is open.
    pub fn skip_rest(&mut self) -> Result<(), NbtReadError> {
        let depth = self.stack.len();
        assert!(depth > 0, "a compound or list is open");
        while self.stack.len() >= depth {
            self.step(true)?;
        }
        Ok(())
    }

    /// The input the reader was built over, once its root has closed and no byte follows it.
    ///
    /// # Errors
    ///
    /// [`NbtReadError::TrailingBytes`] for a byte past the root, the failure of the read that looks
    /// for one, which is where a gzip decoder checks its trailer, or [`NbtReadError::AfterFailure`].
    ///
    /// # Panics
    ///
    /// When the root has not closed.
    pub fn finish(mut self) -> Result<R, NbtReadError> {
        match self.state {
            State::Closed => {}
            State::Failed { offset } => return Err(NbtReadError::AfterFailure { offset }),
            State::Start | State::Open => panic!("`finish` is called once the root has closed"),
        }
        let mut byte = [0];
        loop {
            match self.reader.read(&mut byte) {
                Ok(0) => return Ok(self.reader),
                Ok(_) => {
                    return Err(NbtReadError::TrailingBytes {
                        offset: self.offset,
                    });
                }
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// One event, the reader refusing every call after one that failed.
    fn step(&mut self, skip: bool) -> Result<Option<Event>, NbtReadError> {
        match self.state {
            State::Failed { offset } => return Err(NbtReadError::AfterFailure { offset }),
            State::Closed => return Ok(None),
            State::Start | State::Open => {}
        }
        let event = self.advance(skip);
        self.state = match &event {
            Err(_) => State::Failed {
                offset: self.offset,
            },
            Ok(_) if self.stack.is_empty() => State::Closed,
            Ok(_) => State::Open,
        };
        event
    }

    /// The event the next bytes hold; `skip` reads a value's bytes without keeping them.
    fn advance(&mut self, skip: bool) -> Result<Option<Event>, NbtReadError> {
        if matches!(self.state, State::Start) {
            let id = self.read_u8()?;
            if id != COMPOUND_ID {
                return Err(NbtReadError::RootNotCompound { id });
            }
            let name = self.read_string()?;
            self.push(Frame::Compound, 0)?;
            return Ok(Some(Event::Compound { name: Some(name) }));
        }
        let tag_at = self.offset;
        let frame = self
            .stack
            .last_mut()
            .expect("an open root has a frame until it closes");
        let (name, type_id) = match frame {
            Frame::List { remaining: 0, .. } => {
                self.stack.pop();
                return Ok(Some(Event::End));
            }
            Frame::List {
                element_type_id,
                remaining,
            } => {
                *remaining -= 1;
                (None, *element_type_id)
            }
            Frame::Compound => {
                let id = self.read_u8()?;
                if id == END_ID {
                    self.stack.pop();
                    return Ok(Some(Event::End));
                }
                check_type(id, tag_at)?;
                (Some(self.read_string()?), id)
            }
        };
        match type_id {
            COMPOUND_ID => {
                self.push(Frame::Compound, tag_at)?;
                Ok(Some(Event::Compound { name }))
            }
            LIST_ID => {
                let type_at = self.offset;
                let element_type_id = self.read_u8()?;
                let len = self.read_len("list")?;
                if element_type_id == END_ID && len > 0 {
                    return Err(NbtReadError::ListOfEnd {
                        len: i32::try_from(len).expect("a length read from an i32"),
                        offset: type_at,
                    });
                }
                if len > 0 {
                    check_type(element_type_id, type_at)?;
                }
                let element_type_id = if len == 0 { END_ID } else { element_type_id };
                self.push(
                    Frame::List {
                        element_type_id,
                        remaining: len,
                    },
                    tag_at,
                )?;
                Ok(Some(Event::List {
                    name,
                    element_type_id,
                    len,
                }))
            }
            id => match self.read_payload(id, skip)? {
                Payload::Tag(tag) => Ok(Some(Event::Value { name, tag })),
                // Only `skip_rest` skips, and it drops the event unread.
                Payload::Skipped => Ok(Some(Event::Value {
                    name,
                    tag: Tag::Byte(0),
                })),
            },
        }
    }

    /// Opens `frame`, the tag starting at byte `tag_at`.
    fn push(&mut self, frame: Frame, tag_at: u64) -> Result<(), NbtReadError> {
        if self.stack.len() == MAX_DEPTH {
            return Err(NbtReadError::TooDeep {
                limit: MAX_DEPTH,
                offset: tag_at,
            });
        }
        self.stack.push(frame);
        Ok(())
    }

    /// The payload of a tag that is neither a compound nor a list.
    fn read_payload(&mut self, id: u8, skip: bool) -> Result<Payload, NbtReadError> {
        let tag = match id {
            1 => Tag::Byte(i8::from_ne_bytes(self.read_bytes::<1>()?)),
            2 => Tag::Short(self.endian.i16(self.read_bytes()?)),
            3 => Tag::Int(self.endian.i32(self.read_bytes()?)),
            4 => Tag::Long(self.endian.i64(self.read_bytes()?)),
            5 => Tag::Float(self.endian.f32(self.read_bytes()?)),
            6 => Tag::Double(self.endian.f64(self.read_bytes()?)),
            7 => {
                let len = self.read_len("byte_array")?;
                if skip {
                    self.discard(len)?;
                    return Ok(Payload::Skipped);
                }
                self.hold(len)?;
                let bytes = self.read_growing(len)?;
                Tag::ByteArray(bytes.into_iter().map(|b| i8::from_ne_bytes([b])).collect())
            }
            8 if skip => {
                let len = usize::from(self.endian.u16(self.read_bytes()?));
                self.discard(len)?;
                return Ok(Payload::Skipped);
            }
            8 => Tag::String(self.read_string()?),
            11 => {
                let len = self.read_len("int_array")?.saturating_mul(4);
                if skip {
                    self.discard(len)?;
                    return Ok(Payload::Skipped);
                }
                self.hold(len)?;
                let bytes = self.read_growing(len)?;
                let endian = self.endian;
                Tag::IntArray(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| endian.i32(*c))
                        .collect(),
                )
            }
            12 => {
                let len = self.read_len("long_array")?.saturating_mul(8);
                if skip {
                    self.discard(len)?;
                    return Ok(Payload::Skipped);
                }
                self.hold(len)?;
                let bytes = self.read_growing(len)?;
                let endian = self.endian;
                Tag::LongArray(
                    bytes
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|c| endian.i64(*c))
                        .collect(),
                )
            }
            _ => unreachable!("type ids are checked before their payload is read"),
        };
        Ok(Payload::Tag(tag))
    }

    fn read_bytes<const N: usize>(&mut self) -> Result<[u8; N], NbtReadError> {
        let mut bytes = [0; N];
        self.fill(&mut bytes)?;
        Ok(bytes)
    }

    fn read_u8(&mut self) -> Result<u8, NbtReadError> {
        Ok(self.read_bytes::<1>()?[0])
    }

    /// An `i32` length prefix, refused when negative or past the limit for `context`.
    fn read_len(&mut self, context: &'static str) -> Result<usize, NbtReadError> {
        let offset = self.offset;
        let len = self.endian.i32(self.read_bytes()?);
        let len = usize::try_from(len).map_err(|_| NbtReadError::NegativeLength {
            context,
            len,
            offset,
        })?;
        let limit = if context == "list" {
            self.max_list_len
        } else {
            self.max_array_len
        };
        if len > limit {
            return Err(NbtReadError::TooLong {
                context,
                len,
                limit,
                offset,
            });
        }
        Ok(len)
    }

    /// Fills `buf`, counting what arrives, so input that ends early says where it ends.
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), NbtReadError> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.reader.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(err) => return Err(err.into()),
            }
        }
        self.offset += filled as u64;
        if filled < buf.len() {
            return Err(NbtReadError::Truncated {
                offset: self.offset,
            });
        }
        Ok(())
    }

    fn discard(&mut self, len: usize) -> Result<(), NbtReadError> {
        let len = u64::try_from(len).expect("a usize fits u64");
        let copied = std::io::copy(&mut (&mut self.reader).take(len), &mut std::io::sink())?;
        self.offset += copied;
        if copied < len {
            return Err(NbtReadError::Truncated {
                offset: self.offset,
            });
        }
        Ok(())
    }

    /// `len` bytes, the buffer growing as they arrive so a false length cannot reserve memory.
    fn read_growing(&mut self, len: usize) -> Result<Vec<u8>, NbtReadError> {
        let mut bytes = Vec::with_capacity(len.min(PREALLOCATE_AT_MOST));
        let len = u64::try_from(len).expect("a usize fits u64");
        let read = (&mut self.reader).take(len).read_to_end(&mut bytes)?;
        self.offset += read as u64;
        if bytes.len() as u64 != len {
            return Err(NbtReadError::Truncated {
                offset: self.offset,
            });
        }
        Ok(bytes)
    }

    fn read_string(&mut self) -> Result<String, NbtReadError> {
        let offset = self.offset;
        let len = usize::from(self.endian.u16(self.read_bytes()?));
        let mut bytes = vec![0; len];
        self.fill(&mut bytes)?;
        let decoded = match self.dialect {
            Dialect::Java => decode_modified_utf8(&bytes),
            Dialect::Bedrock => String::from_utf8(bytes).ok(),
        };
        decoded.ok_or(NbtReadError::InvalidString {
            encoding: match self.dialect {
                Dialect::Java => "Modified UTF-8",
                Dialect::Bedrock => "UTF-8",
            },
            offset,
        })
    }
}

/// A compound or list [`NbtReader::read_rest`] is building, with the name it will be filed under.
enum Partial {
    Compound(Option<String>, Compound),
    List(Option<String>, u8, Vec<Tag>),
}

impl Partial {
    /// The memory, about, that pushing `entry` takes beyond what it holds itself: a compound's
    /// slot for it and its name, or a full list's next block of slots, which it reserves.
    fn room_for(&mut self, (name, _): &(Option<String>, Tag)) -> usize {
        match self {
            Partial::Compound(..) => {
                size_of::<(u64, String, Tag)>()
                    + size_of::<usize>()
                    + name.as_ref().map_or(0, String::len)
            }
            Partial::List(_, _, items) if items.len() == items.capacity() => {
                let more = items.capacity().max(4);
                items.reserve_exact(more);
                more.saturating_mul(size_of::<Tag>())
            }
            Partial::List(..) => 0,
        }
    }

    fn push(&mut self, (name, tag): (Option<String>, Tag)) {
        match self {
            Partial::Compound(_, compound) => {
                compound.insert(name.expect("a compound entry is named"), tag);
            }
            Partial::List(_, _, items) => items.push(tag),
        }
    }

    fn finish(self) -> (Option<String>, Tag) {
        match self {
            Partial::Compound(name, compound) => (name, Tag::Compound(compound)),
            Partial::List(name, element_type_id, items) => {
                (name, Tag::List(List::of_tags(element_type_id, items)))
            }
        }
    }
}

fn check_type(id: u8, offset: u64) -> Result<(), NbtReadError> {
    if (1..=12).contains(&id) {
        Ok(())
    } else {
        Err(NbtReadError::UnknownTagType { id, offset })
    }
}

/// Java's Modified UTF-8: UTF-8 whose NUL is two bytes and whose supplementary characters are
/// surrogate pairs of three bytes each.
fn decode_modified_utf8(bytes: &[u8]) -> Option<String> {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some((&first, tail)) = rest.split_first() {
        let continuation = |at: usize| {
            tail.get(at)
                .filter(|byte| *byte & 0xC0 == 0x80)
                .map(|byte| u16::from(byte & 0x3F))
        };
        let (unit, used) = match first {
            0x00..=0x7F => (u16::from(first), 1),
            0xC0..=0xDF => (u16::from(first & 0x1F) << 6 | continuation(0)?, 2),
            0xE0..=0xEF => (
                u16::from(first & 0x0F) << 12 | continuation(0)? << 6 | continuation(1)?,
                3,
            ),
            _ => return None,
        };
        units.push(unit);
        rest = &rest[used..];
    }
    String::from_utf16(&units).ok()
}

/// Reads one named root of uncompressed Java NBT as a tree, refusing input past it.
///
/// # Errors
///
/// Those of [`NbtReader::next_event`] and [`NbtReader::finish`].
pub fn read_java_uncompressed<R: Read>(reader: R) -> Result<(String, Compound), NbtReadError> {
    read_root(NbtReader::java(reader))
}

/// Reads one named root of gzip-wrapped Java NBT, the form `.nbt` structure files take, as a tree,
/// refusing input past it in the gzip stream or after it.
///
/// # Errors
///
/// Those of [`NbtReader::next_event`] and [`NbtReader::finish`], or [`NbtReadError::Io`] for
/// input that is not gzip or whose gzip trailer does not match it.
pub fn read_java_gzip<R: Read>(reader: R) -> Result<(String, Compound), NbtReadError> {
    // As Java's `GZIPInputStream` reads them, members after the first continue the stream.
    read_java_uncompressed(flate2::read::MultiGzDecoder::new(reader))
}

/// Reads one named root of Bedrock NBT, the `.mcstructure` form, as a tree, refusing input past it.
///
/// # Errors
///
/// Those of [`NbtReader::next_event`] and [`NbtReader::finish`].
pub fn read_bedrock_uncompressed<R: Read>(reader: R) -> Result<(String, Compound), NbtReadError> {
    read_root(NbtReader::bedrock(reader))
}

fn read_root<R: Read>(mut reader: NbtReader<R>) -> Result<(String, Compound), NbtReadError> {
    let Some(Event::Compound { name: Some(name) }) = reader.next_event()? else {
        unreachable!("a root opens as a named compound or fails to read");
    };
    let Tag::Compound(root) = reader.read_rest()? else {
        unreachable!("the rest of a compound reads as a compound");
    };
    reader.finish()?;
    Ok((name, root))
}
