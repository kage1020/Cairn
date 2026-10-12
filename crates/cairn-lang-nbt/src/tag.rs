//! The in-memory NBT tag tree the writers encode.

use indexmap::IndexMap;

use crate::error::NbtIoError;

/// An NBT tag; `TAG_End` exists only as a compound's terminator, so it has no variant.
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    /// `TAG_Byte`.
    Byte(i8),
    /// `TAG_Short`.
    Short(i16),
    /// `TAG_Int`.
    Int(i32),
    /// `TAG_Long`.
    Long(i64),
    /// `TAG_Float`.
    Float(f32),
    /// `TAG_Double`.
    Double(f64),
    /// `TAG_Byte_Array`.
    ByteArray(Vec<i8>),
    /// `TAG_String`.
    String(String),
    /// `TAG_List`.
    List(List),
    /// `TAG_Compound`.
    Compound(Compound),
    /// `TAG_Int_Array`.
    IntArray(Vec<i32>),
    /// `TAG_Long_Array`.
    LongArray(Vec<i64>),
}

impl Tag {
    /// The wire tag id, the same in both dialects.
    #[must_use]
    pub fn type_id(&self) -> u8 {
        match self {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(_) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }
}

/// Named NBT tags, written in insertion order so one source always compiles to the same bytes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Compound {
    /// The entries, in insertion order.
    pub entries: IndexMap<String, Tag>,
}

impl Compound {
    /// An empty compound.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces `name`; a replaced entry keeps its position.
    pub fn insert(&mut self, name: impl Into<String>, value: Tag) -> &mut Self {
        self.entries.insert(name.into(), value);
        self
    }
}

/// A homogeneous list of tags.
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    /// The wire id of every item, `0` (`TAG_End`) for an empty list.
    pub element_type_id: u8,
    /// The items.
    pub items: Vec<Tag>,
}

impl List {
    /// An empty list.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            element_type_id: 0,
            items: vec![],
        }
    }

    /// A `TAG_Int` list, built as [`List::of_tags`] builds one.
    #[must_use]
    pub fn of_ints(values: impl IntoIterator<Item = i32>) -> Self {
        Self::of_tags(3, values.into_iter().map(Tag::Int).collect())
    }

    /// A `TAG_Compound` list, built as [`List::of_tags`] builds one.
    #[must_use]
    pub fn of_compounds(items: Vec<Compound>) -> Self {
        Self::of_tags(10, items.into_iter().map(Tag::Compound).collect())
    }

    /// A list of `items` declaring `element_type_id`, or `TAG_End` when `items` is empty.
    #[must_use]
    pub fn of_tags(element_type_id: u8, items: Vec<Tag>) -> Self {
        if items.is_empty() {
            return Self::empty();
        }
        Self {
            element_type_id,
            items,
        }
    }
}

/// Checks that the writers would accept `s` as an NBT string, without writing it.
///
/// # Errors
///
/// [`NbtIoError::InvalidString`] for a NUL or non-ASCII byte, or [`NbtIoError::LengthOverflow`].
pub fn check_string(s: &str) -> Result<(), NbtIoError> {
    string_len(s).map(drop)
}

/// [`check_string`], returning the length prefix to write.
pub(crate) fn string_len(s: &str) -> Result<u16, NbtIoError> {
    for (index, &byte) in s.as_bytes().iter().enumerate() {
        if byte == 0 || byte >= 0x80 {
            return Err(NbtIoError::InvalidString { byte, index });
        }
    }
    u16::try_from(s.len()).map_err(|_| NbtIoError::LengthOverflow {
        context: "string",
        len: s.len(),
        limit: u16::MAX as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_ids_match_spec() {
        assert_eq!(Tag::Byte(0).type_id(), 1);
        assert_eq!(Tag::Long(0).type_id(), 4);
        assert_eq!(Tag::String(String::new()).type_id(), 8);
        assert_eq!(Tag::Compound(Compound::new()).type_id(), 10);
        assert_eq!(Tag::LongArray(vec![]).type_id(), 12);
    }

    #[test]
    fn a_list_declares_the_element_id_of_what_built_it() {
        for (label, list, element_type_id, len) in [
            ("empty", List::empty(), 0, 0),
            ("of_ints of nothing", List::of_ints([]), 0, 0),
            ("of_compounds of nothing", List::of_compounds(vec![]), 0, 0),
            ("of_ints", List::of_ints([1, 2, 3]), 3, 3),
            (
                "of_compounds",
                List::of_compounds(vec![Compound::new(), Compound::new()]),
                10,
                2,
            ),
        ] {
            assert_eq!(list.element_type_id, element_type_id, "{label} element id");
            assert_eq!(list.items.len(), len, "{label} length");
        }
    }

    #[test]
    fn compound_insert_preserves_first_position_on_replace() {
        let mut c = Compound::new();
        c.insert("a", Tag::Int(1));
        c.insert("b", Tag::Int(2));
        c.insert("a", Tag::Int(99));
        let keys: Vec<&String> = c.entries.keys().collect();
        assert_eq!(keys, vec!["a", "b"]);
        assert_eq!(c.entries["a"], Tag::Int(99));
    }
}
