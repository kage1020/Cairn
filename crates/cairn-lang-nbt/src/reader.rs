//! The NBT decoder both dialects share: byte order aside, they differ only in how a string's bytes
//! encode it, Modified UTF-8 on Java and UTF-8 on Bedrock. Nesting is followed on the heap, so the
//! deepest tree the game reads costs no stack.

use std::io::{self, Read};

use crate::read::{MAX_DEPTH, NbtReadError, NbtRoot, ReadOptions};
use crate::tag::{Compound, List, Tag};
use crate::writer::Endian;

/// What a tag counts against [`ReadOptions::max_tree_bytes`] before its strings and arrays.
const TAG_BYTES: usize = std::mem::size_of::<Tag>();

/// What a compound entry counts before its key's bytes and its tag.
const ENTRY_BYTES: usize = std::mem::size_of::<String>() + std::mem::size_of::<usize>();

macro_rules! scalar_readers {
    ($($name:ident: $ty:ty),* $(,)?) => {
        impl<R: Read> Decoder<'_, R> {
            $(
                fn $name(&mut self) -> Result<$ty, NbtReadError> {
                    let mut bytes = [0; std::mem::size_of::<$ty>()];
                    self.reader.read_exact(&mut bytes)?;
                    Ok(match self.endian {
                        Endian::Big => <$ty>::from_be_bytes(bytes),
                        Endian::Little => <$ty>::from_le_bytes(bytes),
                    })
                }
            )*
        }
    };
}

scalar_readers! {
    read_u16: u16,
    read_i16: i16,
    read_i32: i32,
    read_i64: i64,
    read_f32: f32,
    read_f64: f64,
}

struct Decoder<'a, R> {
    reader: R,
    endian: Endian,
    options: &'a ReadOptions<'a>,
    tree_bytes: usize,
}

/// A list or compound whose items are still being read.
enum Building {
    List {
        element_type_id: u8,
        remaining: usize,
        items: Vec<Tag>,
    },
    /// `key` names the entry being read.
    Compound { compound: Compound, key: String },
}

/// What reading one tag left: the tag, or the type of the first item of the container it opened.
enum Step {
    Value(Tag),
    Opened(u8),
}

/// A list or compound being skipped: a list's element type and how many of its items are left to
/// skip, or `None` for a compound.
type Skipping = Option<(u8, usize)>;

pub(crate) fn read_named_root<R: Read>(
    reader: R,
    endian: Endian,
    options: &ReadOptions<'_>,
) -> Result<NbtRoot, NbtReadError> {
    let mut decoder = Decoder {
        reader,
        endian,
        options,
        tree_bytes: 0,
    };
    let id = decoder.read_u8()?;
    if id != 10 {
        return Err(NbtReadError::RootNotCompound { id });
    }
    let name = decoder.read_string()?;
    let mut compound = Compound::new();
    let mut skipped = Vec::new();
    while let Some((id, key)) = decoder.read_entry_header()? {
        if options
            .keep
            .is_none_or(|names| names.contains(&key.as_str()))
        {
            let tag = decoder.read_payload(id, 2)?;
            compound.insert(key, tag);
        } else {
            decoder.skip_payload(id, 2)?;
            skipped.push((key, id));
        }
    }
    Ok(NbtRoot {
        name,
        compound,
        skipped,
    })
}

impl<R: Read> Decoder<'_, R> {
    fn charge(&mut self, bytes: usize) -> Result<(), NbtReadError> {
        let limit = self.options.max_tree_bytes;
        self.tree_bytes = self.tree_bytes.saturating_add(bytes);
        if self.tree_bytes > limit {
            return Err(NbtReadError::TooLarge { limit });
        }
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8, NbtReadError> {
        let mut byte = [0];
        self.reader.read_exact(&mut byte)?;
        Ok(byte[0])
    }

    fn read_len(&mut self, context: &'static str) -> Result<usize, NbtReadError> {
        let len = self.read_i32()?;
        usize::try_from(len).map_err(|_| NbtReadError::NegativeLength { context, len })
    }

    /// Exactly `len` bytes, grown as they arrive so a declared length allocates no more than
    /// the input holds.
    fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>, NbtReadError> {
        let mut bytes = Vec::new();
        (&mut self.reader)
            .take(as_u64(len))
            .read_to_end(&mut bytes)?;
        if bytes.len() == len {
            Ok(bytes)
        } else {
            Err(NbtReadError::Truncated)
        }
    }

    fn skip_bytes(&mut self, len: u64) -> Result<(), NbtReadError> {
        let skipped = io::copy(&mut (&mut self.reader).take(len), &mut io::sink())?;
        if skipped == len {
            Ok(())
        } else {
            Err(NbtReadError::Truncated)
        }
    }

    fn read_string(&mut self) -> Result<String, NbtReadError> {
        let len = usize::from(self.read_u16()?);
        let bytes = self.read_bytes(len)?;
        match self.endian {
            Endian::Big => decode_modified_utf8(&bytes).ok_or(NbtReadError::InvalidString {
                encoding: "Modified UTF-8",
            }),
            Endian::Little => String::from_utf8(bytes)
                .map_err(|_| NbtReadError::InvalidString { encoding: "UTF-8" }),
        }
    }

    /// The next entry's tag id and key, charged, or `None` at the compound's `TAG_End`.
    fn read_entry_header(&mut self) -> Result<Option<(u8, String)>, NbtReadError> {
        let id = self.read_u8()?;
        if id == 0 {
            return Ok(None);
        }
        let key = self.read_string()?;
        self.charge(ENTRY_BYTES + key.len())?;
        Ok(Some((id, key)))
    }

    fn read_list_header(&mut self) -> Result<(u8, usize), NbtReadError> {
        let element_type_id = self.read_u8()?;
        if element_type_id > 12 {
            return Err(NbtReadError::UnknownTagId {
                id: element_type_id,
            });
        }
        let len = self.read_len("list")?;
        if element_type_id == 0 && len > 0 {
            return Err(NbtReadError::UntypedList {
                len: i32::try_from(len).expect("read from an i32"),
            });
        }
        Ok((element_type_id, len))
    }

    /// The tag of type `id` whose payload is next, at `depth`.
    fn read_payload(&mut self, id: u8, depth: usize) -> Result<Tag, NbtReadError> {
        let mut open: Vec<Building> = Vec::new();
        let mut next = id;
        loop {
            let mut value = match self.read_value(next, depth + open.len(), &mut open)? {
                Step::Value(value) => value,
                Step::Opened(id) => {
                    next = id;
                    continue;
                }
            };
            // Hand the finished value to the container around it, closing every one it fills.
            loop {
                match open.last_mut() {
                    None => return Ok(value),
                    Some(Building::List {
                        element_type_id,
                        remaining,
                        items,
                    }) => {
                        items.push(value);
                        *remaining -= 1;
                        if *remaining > 0 {
                            next = *element_type_id;
                            break;
                        }
                        let (element_type_id, items) = (*element_type_id, std::mem::take(items));
                        open.pop();
                        value = Tag::List(List::of_tags(element_type_id, items));
                    }
                    Some(Building::Compound { compound, key }) => {
                        compound.insert(std::mem::take(key), value);
                        if let Some((id, name)) = self.read_entry_header()? {
                            *key = name;
                            next = id;
                            break;
                        }
                        let compound = std::mem::take(compound);
                        open.pop();
                        value = Tag::Compound(compound);
                    }
                }
            }
        }
    }

    /// Reads a tag of type `id` at `depth` whole, or opens it onto `open` when it holds a first
    /// item or entry, returning that item's type.
    fn read_value(
        &mut self,
        id: u8,
        depth: usize,
        open: &mut Vec<Building>,
    ) -> Result<Step, NbtReadError> {
        self.charge(TAG_BYTES)?;
        Ok(Step::Value(match id {
            1 => Tag::Byte(i8::from_be_bytes([self.read_u8()?])),
            2 => Tag::Short(self.read_i16()?),
            3 => Tag::Int(self.read_i32()?),
            4 => Tag::Long(self.read_i64()?),
            5 => Tag::Float(self.read_f32()?),
            6 => Tag::Double(self.read_f64()?),
            7 => {
                let len = self.read_len("byte_array")?;
                self.charge(len)?;
                Tag::ByteArray(
                    self.read_bytes(len)?
                        .into_iter()
                        .map(|byte| i8::from_be_bytes([byte]))
                        .collect(),
                )
            }
            8 => {
                let value = self.read_string()?;
                self.charge(value.len())?;
                Tag::String(value)
            }
            9 => {
                check_depth(depth)?;
                let (element_type_id, len) = self.read_list_header()?;
                if len == 0 {
                    Tag::List(List::empty())
                } else {
                    open.push(Building::List {
                        element_type_id,
                        remaining: len,
                        items: Vec::new(),
                    });
                    return Ok(Step::Opened(element_type_id));
                }
            }
            10 => {
                check_depth(depth)?;
                match self.read_entry_header()? {
                    None => Tag::Compound(Compound::new()),
                    Some((id, key)) => {
                        open.push(Building::Compound {
                            compound: Compound::new(),
                            key,
                        });
                        return Ok(Step::Opened(id));
                    }
                }
            }
            11 => {
                let len = self.read_len("int_array")?;
                self.charge(len.saturating_mul(4))?;
                Tag::IntArray(
                    (0..len)
                        .map(|_| self.read_i32())
                        .collect::<Result<_, _>>()?,
                )
            }
            12 => {
                let len = self.read_len("long_array")?;
                self.charge(len.saturating_mul(8))?;
                Tag::LongArray(
                    (0..len)
                        .map(|_| self.read_i64())
                        .collect::<Result<_, _>>()?,
                )
            }
            id => return Err(NbtReadError::UnknownTagId { id }),
        }))
    }

    /// Reads past the tag of type `id` whose payload is next, at `depth`, building nothing.
    fn skip_payload(&mut self, id: u8, depth: usize) -> Result<(), NbtReadError> {
        let mut open: Vec<Skipping> = Vec::new();
        let mut next = Some(id);
        loop {
            if let Some(id) = next.take() {
                self.skip_value(id, depth + open.len(), &mut open)?;
            }
            match open.last_mut() {
                None => return Ok(()),
                Some(Some((_, 0))) => {
                    open.pop();
                }
                Some(Some((element_type_id, remaining))) => {
                    *remaining -= 1;
                    next = Some(*element_type_id);
                }
                Some(None) => {
                    let id = self.read_u8()?;
                    if id == 0 {
                        open.pop();
                    } else {
                        let len = self.read_u16()?;
                        self.skip_bytes(u64::from(len))?;
                        next = Some(id);
                    }
                }
            }
        }
    }

    /// Reads past a tag of type `id` at `depth`, or opens it onto `open` when it has a body.
    fn skip_value(
        &mut self,
        id: u8,
        depth: usize,
        open: &mut Vec<Skipping>,
    ) -> Result<(), NbtReadError> {
        match id {
            9 => {
                check_depth(depth)?;
                let (element_type_id, len) = self.read_list_header()?;
                match fixed_width(element_type_id) {
                    Some(width) => self.skip_bytes(width.saturating_mul(as_u64(len)))?,
                    None => open.push(Some((element_type_id, len))),
                }
            }
            10 => {
                check_depth(depth)?;
                open.push(None);
            }
            8 => {
                let len = self.read_u16()?;
                self.skip_bytes(u64::from(len))?;
            }
            7 | 11 | 12 => {
                let (context, width) = match id {
                    7 => ("byte_array", 1),
                    11 => ("int_array", 4),
                    _ => ("long_array", 8),
                };
                let len = self.read_len(context)?;
                self.skip_bytes(as_u64(len).saturating_mul(width))?;
            }
            id => match fixed_width(id) {
                Some(width) => self.skip_bytes(width)?,
                None => return Err(NbtReadError::UnknownTagId { id }),
            },
        }
        Ok(())
    }
}

fn as_u64(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// The payload width of a scalar tag id, or `None` for one with a length or a body.
fn fixed_width(id: u8) -> Option<u64> {
    match id {
        1 => Some(1),
        2 => Some(2),
        3 | 5 => Some(4),
        4 | 6 => Some(8),
        _ => None,
    }
}

/// Refuses a compound or list at `depth`, counting the root as 1, past [`MAX_DEPTH`].
fn check_depth(depth: usize) -> Result<(), NbtReadError> {
    if depth > MAX_DEPTH {
        Err(NbtReadError::TooDeep)
    } else {
        Ok(())
    }
}

/// Java's Modified UTF-8: UTF-8 except that NUL is two bytes and a character past U+FFFF is two
/// three-byte surrogates.
fn decode_modified_utf8(bytes: &[u8]) -> Option<String> {
    let mut units = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some((&first, tail)) = rest.split_first() {
        let continuation = |at: usize| {
            tail.get(at)
                .filter(|&&byte| byte & 0xC0 == 0x80)
                .map(|&byte| u16::from(byte & 0x3F))
        };
        let (unit, width) = match first {
            0x01..=0x7F => (u16::from(first), 1),
            0xC0..=0xDF => ((u16::from(first & 0x1F) << 6) | continuation(0)?, 2),
            0xE0..=0xEF => (
                (u16::from(first & 0x0F) << 12) | (continuation(0)? << 6) | continuation(1)?,
                3,
            ),
            _ => return None,
        };
        units.push(unit);
        rest = &rest[width..];
    }
    String::from_utf16(&units).ok()
}
