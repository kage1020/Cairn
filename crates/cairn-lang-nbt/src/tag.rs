//! NBT tag tree shared by every writer in this crate.
//!
//! The tree owns its data — callers construct a [`Tag`] / [`Compound`] /
//! [`List`] in memory and hand it to a writer. A borrow-based zero-copy form
//! is deferred until profiling shows it matters; M2 structures are KB-scale.

use indexmap::IndexMap;

/// A single NBT tag.
///
/// Variants mirror the Java vanilla tag ids 1..=12; `TAG_End` (0) is implicit
/// in [`Compound`] termination and is not surfaced here so a caller cannot
/// construct an out-of-place end marker.
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    /// Signed 8-bit integer (`TAG_Byte`, id 1).
    Byte(i8),
    /// Signed 16-bit integer, big-endian on the wire (`TAG_Short`, id 2).
    Short(i16),
    /// Signed 32-bit integer, big-endian on the wire (`TAG_Int`, id 3).
    Int(i32),
    /// Signed 64-bit integer, big-endian on the wire (`TAG_Long`, id 4).
    Long(i64),
    /// IEEE 754 single, big-endian on the wire (`TAG_Float`, id 5).
    Float(f32),
    /// IEEE 754 double, big-endian on the wire (`TAG_Double`, id 6).
    Double(f64),
    /// Length-prefixed signed byte array (`TAG_Byte_Array`, id 7).
    ByteArray(Vec<i8>),
    /// Length-prefixed Modified UTF-8 string (`TAG_String`, id 8).
    String(String),
    /// Homogeneous list (`TAG_List`, id 9). See [`List`].
    List(List),
    /// Keyed collection of named tags (`TAG_Compound`, id 10).
    Compound(Compound),
    /// Length-prefixed `i32` array (`TAG_Int_Array`, id 11).
    IntArray(Vec<i32>),
    /// Length-prefixed `i64` array (`TAG_Long_Array`, id 12).
    LongArray(Vec<i64>),
}

impl Tag {
    /// Wire tag id used by the Java/Bedrock NBT formats. Matches the
    /// variant order above (1 = `Byte` through 12 = `LongArray`); `TAG_End` is
    /// `0` and is written by [`Compound`] termination, not by this enum.
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

/// An ordered map of named NBT tags.
///
/// `IndexMap` is used so write order matches insertion order. Java vanilla
/// does not require a canonical key order, but determinism is what the
/// downstream `resolved_ir_hash` rests on — two compiles of the same source
/// must produce the same byte stream.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Compound {
    /// Named entries in insertion order.
    pub entries: IndexMap<String, Tag>,
}

impl Compound {
    /// Empty compound.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a named entry and return `&mut self` so callers can
    /// chain. Insertion order is preserved on first insert; replacement
    /// keeps the original position.
    pub fn insert(&mut self, name: impl Into<String>, value: Tag) -> &mut Self {
        self.entries.insert(name.into(), value);
        self
    }
}

/// A homogeneous list of tags.
///
/// The element type is captured by `element_type_id` rather than inferred
/// from `items[0]` so an empty list can still declare its element kind. The
/// spec requires `0` (`TAG_End`) for empty lists, which is what
/// [`List::empty`] writes; populated lists are expected to declare the same
/// id as every item carries.
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    /// Wire id of every item. `0` for an empty list (spec).
    pub element_type_id: u8,
    /// List items, all of the same wire type.
    pub items: Vec<Tag>,
}

impl List {
    /// Empty list with the spec-mandated `element_type_id = 0`.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            element_type_id: 0,
            items: vec![],
        }
    }

    /// Build a `List<TAG_Int>` from a slice of integers. Used for the
    /// `size` and `pos` triples in Java vanilla structure NBT.
    #[must_use]
    pub fn of_ints(values: impl IntoIterator<Item = i32>) -> Self {
        Self {
            element_type_id: 3,
            items: values.into_iter().map(Tag::Int).collect(),
        }
    }

    /// Build a `List<TAG_Compound>` from a vector of [`Compound`]s. Used
    /// for `palette` and `blocks` in Java vanilla structure NBT (and for
    /// `entities` once entity lowering lands).
    #[must_use]
    pub fn of_compounds(items: Vec<Compound>) -> Self {
        Self {
            element_type_id: 10,
            items: items.into_iter().map(Tag::Compound).collect(),
        }
    }
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
    fn empty_list_carries_end_element_id() {
        let l = List::empty();
        assert_eq!(l.element_type_id, 0);
        assert!(l.items.is_empty());
    }

    #[test]
    fn of_ints_sets_int_element_id() {
        let l = List::of_ints([1, 2, 3]);
        assert_eq!(l.element_type_id, 3);
        assert_eq!(l.items.len(), 3);
    }

    #[test]
    fn of_compounds_sets_compound_element_id() {
        let l = List::of_compounds(vec![Compound::new(), Compound::new()]);
        assert_eq!(l.element_type_id, 10);
        assert_eq!(l.items.len(), 2);
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
