//! The streaming writers against the tree writers.
//!
//! A streamed root is only useful if it is the file the tree writer would
//! have produced, so every byte-level test here streams a tree and compares
//! with the tree writer's bytes rather than restating a layout. The
//! remaining tests pin how the stream refuses what it cannot write.

use std::io::Write;

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{
    CompoundStream, ListStream, NbtIoError, stream_bedrock_uncompressed, stream_java_gzip,
    stream_java_uncompressed, write_bedrock_uncompressed, write_java_gzip, write_java_uncompressed,
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

    // The gzip twin wraps the same payload in the same envelope, so the
    // compressed bytes agree too, not only what they decompress to.
    let mut gzip_tree = Vec::new();
    write_java_gzip(&mut gzip_tree, "", &tree).expect("tree write");
    let mut gzip_stream = Vec::new();
    stream_java_gzip(&mut gzip_stream, "", |c| stream_body(c, &tree)).expect("stream write");
    assert_eq!(gzip_stream, gzip_tree, "gzip");
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
fn an_empty_list_claiming_an_element_type_is_refused() {
    // The tree writer refuses `List { element_type_id: 3, items: [] }`; the
    // stream refuses the same declaration before writing its prefix, for
    // a list entry and for a list item alike.
    let mut buf = Vec::new();
    let err = stream_java_uncompressed(&mut buf, "", |root| root.list("size", 3, 0, |_| Ok(())))
        .expect_err("an empty list of ints");
    assert!(
        matches!(err, NbtIoError::EmptyListWithElementType { declared: 3 }),
        "entry: got {err:?}",
    );

    let mut buf = Vec::new();
    let err = stream_bedrock_uncompressed(&mut buf, "", |root| {
        root.list("block_indices", 9, 1, |layers| {
            layers.list(3, 0, |_| Ok(()))
        })
    })
    .expect_err("an empty layer of ints");
    assert!(
        matches!(err, NbtIoError::EmptyListWithElementType { declared: 3 }),
        "item: got {err:?}",
    );
}
