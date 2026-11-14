//! The NBT reader against the writers' bytes and against bytes the writers never emit.

mod events;
mod refusals;
mod trees;

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{write_bedrock_uncompressed, write_java_uncompressed};

/// A root holding every tag type, nested compounds and lists, and an empty list.
fn every_tag() -> Compound {
    let mut inner = Compound::new();
    inner.insert("Name", Tag::String("minecraft:oak_stairs".to_owned()));
    let mut properties = Compound::new();
    properties.insert("facing", Tag::String("east".to_owned()));
    inner.insert("Properties", Tag::Compound(properties));

    let mut root = Compound::new();
    root.insert("byte", Tag::Byte(-1));
    root.insert("short", Tag::Short(-0x0102));
    root.insert("int", Tag::Int(i32::MIN));
    root.insert("long", Tag::Long(0x0102_0304_0506_0708));
    root.insert("float", Tag::Float(-1.5));
    root.insert("double", Tag::Double(1.0e300));
    root.insert("bytes", Tag::ByteArray(vec![-128, 0, 127]));
    root.insert("string", Tag::String("Cairn".to_owned()));
    root.insert("empty", Tag::List(List::empty()));
    root.insert("ints", Tag::List(List::of_ints([1, -2, 3])));
    root.insert(
        "palette",
        Tag::List(List::of_compounds(vec![inner.clone(), Compound::new()])),
    );
    root.insert(
        "layers",
        Tag::List(List {
            element_type_id: 9,
            items: vec![Tag::List(List::of_ints([0])), Tag::List(List::empty())],
        }),
    );
    root.insert("nested", Tag::Compound(inner));
    root.insert("int_array", Tag::IntArray(vec![i32::MAX, 0]));
    root.insert("long_array", Tag::LongArray(vec![-1, i64::MAX]));
    root
}

fn java_bytes(name: &str, root: &Compound) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_java_uncompressed(&mut bytes, name, root).expect("the root writes");
    bytes
}

fn bedrock_bytes(name: &str, root: &Compound) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_bedrock_uncompressed(&mut bytes, name, root).expect("the root writes");
    bytes
}
