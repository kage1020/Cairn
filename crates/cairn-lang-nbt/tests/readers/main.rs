//! The NBT readers, on bytes the writers emit and on bytes no writer would.

mod bedrock;
mod java;
mod refusals;

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{NbtRoot, ReadOptions};

/// A budget no test tree comes near.
const ROOMY: usize = 1 << 20;

/// Every root entry kept, within [`ROOMY`].
fn keep_all() -> ReadOptions<'static> {
    ReadOptions {
        keep: None,
        max_tree_bytes: ROOMY,
    }
}

/// One entry of every tag type, nested ones included.
fn every_tag() -> Compound {
    let mut inner = Compound::new();
    inner.insert("deep", Tag::List(List::of_ints([7, 8])));
    let mut root = Compound::new();
    root.insert("byte", Tag::Byte(-3))
        .insert("short", Tag::Short(-300))
        .insert("int", Tag::Int(70_000))
        .insert("long", Tag::Long(-5_000_000_000))
        .insert("float", Tag::Float(1.5))
        .insert("double", Tag::Double(-2.25))
        .insert("bytes", Tag::ByteArray(vec![-1, 0, 1]))
        .insert("string", Tag::String("oak_planks".to_owned()))
        .insert(
            "list",
            Tag::List(List::of_compounds(vec![inner.clone(), Compound::new()])),
        )
        .insert("compound", Tag::Compound(inner))
        .insert("ints", Tag::IntArray(vec![i32::MIN, 0, i32::MAX]))
        .insert("longs", Tag::LongArray(vec![i64::MIN, i64::MAX]))
        .insert("empty", Tag::List(List::empty()));
    root
}

/// The root a read keeping everything returns for `root` named `name`.
fn read_back(name: &str, root: Compound) -> NbtRoot {
    NbtRoot {
        name: name.to_owned(),
        compound: root,
        skipped: vec![],
    }
}
