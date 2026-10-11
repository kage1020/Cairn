//! The streaming writers against the tree writers.
//!
//! A streamed root is only useful if it is the file the tree writer would
//! have produced, so every byte-level test here streams a tree and compares
//! with the tree writer's bytes rather than restating a layout. The
//! remaining tests pin how the stream refuses what it cannot write.

use std::io::Write;

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{
    CompoundStream, ListStream, NbtIoError, stream_bedrock_uncompressed, stream_java_uncompressed,
    write_bedrock_uncompressed, write_java_uncompressed,
};

/// Stream `tag` under `name` through the structural calls — a compound as
/// [`CompoundStream::compound`], a list as [`CompoundStream::list`] with each
/// item streamed in turn — so the comparison exercises every entry point
/// rather than only [`CompoundStream::tag`].
fn stream_entry<W: Write>(
    compound: &mut CompoundStream<'_, W>,
    name: &str,
    tag: &Tag,
) -> Result<(), NbtIoError> {
    match tag {
        Tag::Compound(inner) => compound.compound(name, |c| stream_body(c, inner)),
        Tag::List(list) => compound.list(name, list.element_type_id, list.items.len(), |l| {
            stream_items(l, list)
        }),
        scalar => compound.tag(name, scalar),
    }
}

fn stream_body<W: Write>(
    compound: &mut CompoundStream<'_, W>,
    tree: &Compound,
) -> Result<(), NbtIoError> {
    for (name, tag) in &tree.entries {
        stream_entry(compound, name, tag)?;
    }
    Ok(())
}

fn stream_items<W: Write>(stream: &mut ListStream<'_, W>, list: &List) -> Result<(), NbtIoError> {
    for item in &list.items {
        match item {
            Tag::Compound(inner) => stream.compound(|c| stream_body(c, inner))?,
            Tag::List(inner) => stream.list(inner.element_type_id, inner.items.len(), |l| {
                stream_items(l, inner)
            })?,
            scalar => stream.item(scalar)?,
        }
    }
    Ok(())
}

/// A root holding every shape the structure backends stream: scalars, a
/// string, an empty list, a list of ints, a list of compounds each holding
/// a list, a nested compound, and a list of lists.
fn every_shape() -> Compound {
    let mut entry = Compound::new();
    entry.insert("state", Tag::Int(2));
    entry.insert("pos", Tag::List(List::of_ints([1, -2, 3])));
    let mut inner = Compound::new();
    inner.insert("name", Tag::String("minecraft:oak_planks".to_owned()));
    inner.insert("states", Tag::Compound(Compound::new()));
    inner.insert("version", Tag::Int(18_168_865));

    let mut root = Compound::new();
    root.insert("byte", Tag::Byte(-1));
    root.insert("short", Tag::Short(0x0102));
    root.insert("long", Tag::Long(0x0102_0304_0506_0708));
    root.insert("double", Tag::Double(1.5));
    root.insert("ints", Tag::IntArray(vec![1, 2]));
    root.insert("empty", Tag::List(List::empty()));
    root.insert("size", Tag::List(List::of_ints([4, 5, 6])));
    root.insert(
        "blocks",
        Tag::List(List::of_compounds(vec![entry.clone(), entry])),
    );
    root.insert("palette", Tag::Compound(inner));
    root.insert(
        "layers",
        Tag::List(List {
            element_type_id: 9,
            items: vec![
                Tag::List(List::of_ints([0, 1])),
                Tag::List(List::of_ints([-1, -1])),
            ],
        }),
    );
    root
}

#[test]
fn a_streamed_root_is_the_bytes_the_tree_writer_writes() {
    let tree = every_shape();

    let mut java_tree = Vec::new();
    write_java_uncompressed(&mut java_tree, "root", &tree).expect("tree write");
    let mut java_stream = Vec::new();
    stream_java_uncompressed(&mut java_stream, "root", |c| stream_body(c, &tree))
        .expect("stream write");
    assert_eq!(java_stream, java_tree, "big-endian");

    let mut bedrock_tree = Vec::new();
    write_bedrock_uncompressed(&mut bedrock_tree, "", &tree).expect("tree write");
    let mut bedrock_stream = Vec::new();
    stream_bedrock_uncompressed(&mut bedrock_stream, "", |c| stream_body(c, &tree))
        .expect("stream write");
    assert_eq!(bedrock_stream, bedrock_tree, "little-endian");
}

#[test]
fn a_held_compound_streams_the_bytes_of_the_tag_wrapping_it() {
    let inner = every_shape();
    let mut wrapped = Vec::new();
    stream_java_uncompressed(&mut wrapped, "", |root| {
        root.tag("palette", &Tag::Compound(inner.clone()))
    })
    .expect("tag write");
    let mut held = Vec::new();
    stream_java_uncompressed(&mut held, "", |root| root.compound_tag("palette", &inner))
        .expect("compound_tag write");
    assert_eq!(held, wrapped);
}

#[test]
#[should_panic(expected = "a streamed list declared 3 items and was given 2")]
fn a_list_given_fewer_items_than_it_declared_panics() {
    let mut buf = Vec::new();
    let _ = stream_java_uncompressed(&mut buf, "", |root| {
        root.list("pos", 3, 3, |pos| {
            pos.item(&Tag::Int(1))?;
            pos.item(&Tag::Int(2))
        })
    });
}

#[test]
#[should_panic(expected = "a streamed list declared 1 items and was given more")]
fn a_list_given_more_items_than_it_declared_panics() {
    let mut buf = Vec::new();
    let _ = stream_java_uncompressed(&mut buf, "", |root| {
        root.list("pos", 3, 1, |pos| {
            pos.item(&Tag::Int(1))?;
            pos.item(&Tag::Int(2))
        })
    });
}

#[test]
#[should_panic(expected = "a streamed list declared 0 items and was given more")]
fn an_item_in_a_list_declared_empty_panics() {
    let mut buf = Vec::new();
    let _ = stream_bedrock_uncompressed(&mut buf, "", |root| {
        root.list("entities", 0, 0, |entities| entities.compound(|_| Ok(())))
    });
}

#[test]
fn an_item_of_another_type_is_refused_with_its_index() {
    let mut buf = Vec::new();
    let err = stream_java_uncompressed(&mut buf, "", |root| {
        root.list("layers", 9, 2, |layers| {
            layers.list(3, 1, |l| l.item(&Tag::Int(0)))?;
            layers.compound(|_| Ok(()))
        })
    })
    .expect_err("a compound in a list of lists");
    assert!(
        matches!(
            err,
            NbtIoError::HeterogeneousList {
                declared: 9,
                index: 1,
                actual: 10,
            }
        ),
        "got {err:?}",
    );
}

#[test]
fn an_empty_list_declares_tag_end_whatever_type_it_names() {
    // `List::of_tags(3, vec![])` declares `TAG_End`, and so does a streamed
    // list of no items, for a list entry and for a list item alike, so a
    // caller does not have to re-derive the rule.
    let mut tree = Compound::new();
    tree.insert("size", Tag::List(List::of_tags(3, vec![])));
    tree.insert(
        "block_indices",
        Tag::List(List {
            element_type_id: 9,
            items: vec![Tag::List(List::of_tags(3, vec![]))],
        }),
    );
    let mut tree_bytes = Vec::new();
    write_bedrock_uncompressed(&mut tree_bytes, "", &tree).expect("tree write");

    let mut streamed = Vec::new();
    stream_bedrock_uncompressed(&mut streamed, "", |root| {
        root.list("size", 3, 0, |_| Ok(()))?;
        root.list("block_indices", 9, 1, |layers| {
            layers.list(3, 0, |_| Ok(()))
        })
    })
    .expect("an empty list of any named type streams");
    assert_eq!(streamed, tree_bytes);
}

/// A writer that accepts `budget` bytes and then reports a full disk.
struct FillsUp {
    budget: usize,
}

impl Write for FillsUp {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.budget == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
        }
        let n = buf.len().min(self.budget);
        self.budget -= n;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_writer_failing_inside_a_list_returns_its_error_rather_than_panicking() {
    // The count check runs only after the list's closure returns `Ok`. A
    // write failing part-way leaves the list short, and checking the count
    // first would turn a full disk into an abort in a `panic = "abort"`
    // build.
    let mut disk = FillsUp { budget: 64 };
    let err = stream_bedrock_uncompressed(&mut disk, "", |root| {
        root.list("layer", 3, 1000, |layer| {
            for i in 0..1000 {
                layer.item(&Tag::Int(i))?;
            }
            Ok(())
        })
    })
    .expect_err("the disk fills up");
    assert!(
        matches!(&err, NbtIoError::Io(io) if io.kind() == std::io::ErrorKind::StorageFull),
        "got {err:?}",
    );
}
