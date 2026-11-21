//! An `nbt={...}` compound a line writes, as the tags a writer saves it as (`spec/entities` "Two
//! tiers"): typed as SNBT reads it, and put over what the writer builds, key by key.

use cairn_lang_core::lex::{NbtCompound, NbtValue, parse_compound};
use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{NbtIoError, check_string};

/// The tags of `text`, a compound as a line's `nbt=` writes it; `None` for text the lexer refuses.
pub(crate) fn compound_of(text: &str) -> Option<Compound> {
    parse_compound(text).map(|compound| tags_of(&compound))
}

fn tags_of(compound: &NbtCompound) -> Compound {
    let mut tags = Compound::new();
    for (key, value) in &compound.entries {
        tags.insert(key.clone(), tag_of(value));
    }
    tags
}

fn tag_of(value: &NbtValue) -> Tag {
    match value {
        NbtValue::Byte(value) => Tag::Byte(*value),
        NbtValue::Short(value) => Tag::Short(*value),
        NbtValue::Int(value) => Tag::Int(*value),
        NbtValue::Long(value) => Tag::Long(*value),
        NbtValue::Float(value) => Tag::Float(*value),
        NbtValue::Double(value) => Tag::Double(*value),
        NbtValue::String(value) => Tag::String(value.clone()),
        NbtValue::List(items) => {
            let items: Vec<Tag> = items.iter().map(tag_of).collect();
            // The lexer holds a list to one type, so its first item names it.
            let element_type_id = items.first().map_or(0, Tag::type_id);
            Tag::List(List::of_tags(element_type_id, items))
        }
        NbtValue::Compound(compound) => Tag::Compound(tags_of(compound)),
        NbtValue::ByteArray(values) => Tag::ByteArray(values.clone()),
        NbtValue::IntArray(values) => Tag::IntArray(values.clone()),
        NbtValue::LongArray(values) => Tag::LongArray(values.clone()),
    }
}

/// `base` with each entry of `written` put over it, in place of an entry of its key.
pub(crate) fn put_over(base: &mut Compound, written: Compound) {
    for (key, value) in written.entries {
        base.insert(key, value);
    }
}

/// The first string of `compound`, a key or a value at any depth, that the NBT encoder refuses,
/// with its refusal.
pub(crate) fn first_unencodable(compound: &Compound) -> Option<(String, NbtIoError)> {
    compound.entries.iter().find_map(|(key, value)| {
        check_string(key)
            .err()
            .map(|refusal| (key.clone(), refusal))
            .or_else(|| unencodable_in(value))
    })
}

fn unencodable_in(tag: &Tag) -> Option<(String, NbtIoError)> {
    match tag {
        Tag::String(text) => check_string(text)
            .err()
            .map(|refusal| (text.clone(), refusal)),
        Tag::List(list) => list.items.iter().find_map(unencodable_in),
        Tag::Compound(compound) => first_unencodable(compound),
        _ => None,
    }
}
