//! The pull reader's events, and what `read_rest` and `skip_rest` take of them.

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{Event, NbtReader};

use crate::{bedrock_bytes, java_bytes};

fn all_events<R: std::io::Read>(reader: &mut NbtReader<R>) -> Vec<Event> {
    let mut events = Vec::new();
    while let Some(event) = reader.next_event().expect("the root reads") {
        events.push(event);
    }
    events
}

#[test]
fn a_root_yields_its_tags_in_wire_order() {
    let mut item = Compound::new();
    item.insert("b", Tag::Byte(2));
    let mut root = Compound::new();
    root.insert("a", Tag::Int(1));
    root.insert("l", Tag::List(List::of_compounds(vec![item])));
    root.insert("e", Tag::List(List::empty()));
    let expected = vec![
        Event::Compound {
            name: Some("r".to_owned()),
        },
        Event::Value {
            name: Some("a".to_owned()),
            tag: Tag::Int(1),
        },
        Event::List {
            name: Some("l".to_owned()),
            element_type_id: 10,
            len: 1,
        },
        Event::Compound { name: None },
        Event::Value {
            name: Some("b".to_owned()),
            tag: Tag::Byte(2),
        },
        Event::End,
        Event::End,
        Event::List {
            name: Some("e".to_owned()),
            element_type_id: 0,
            len: 0,
        },
        Event::End,
        Event::End,
    ];

    let bytes = java_bytes("r", &root);
    let mut java = NbtReader::java(bytes.as_slice());
    assert_eq!(all_events(&mut java), expected, "big-endian");
    assert_eq!(java.offset(), bytes.len() as u64, "every byte consumed");

    let bytes = bedrock_bytes("r", &root);
    let mut bedrock = NbtReader::bedrock(bytes.as_slice());
    assert_eq!(all_events(&mut bedrock), expected, "little-endian");
}

#[test]
fn an_empty_list_declaring_an_element_type_reads_as_one_of_tag_end() {
    // `{l: []}` declaring `TAG_Int`, as some writers other than Cairn's emit it.
    let bytes = [10, 0, 0, 9, 0, 1, b'l', 3, 0, 0, 0, 0, 0];
    let mut reader = NbtReader::java(bytes.as_slice());
    let events = all_events(&mut reader);
    assert_eq!(
        events[1],
        Event::List {
            name: Some("l".to_owned()),
            element_type_id: 0,
            len: 0,
        },
    );
}

#[test]
fn read_rest_takes_one_list_item_at_a_time() {
    let blocks: Vec<Compound> = (0..3)
        .map(|state| {
            let mut entry = Compound::new();
            entry.insert("state", Tag::Int(state));
            entry.insert("pos", Tag::List(List::of_ints([state, 0, 0])));
            entry
        })
        .collect();
    let mut root = Compound::new();
    root.insert("blocks", Tag::List(List::of_compounds(blocks.clone())));
    root.insert("DataVersion", Tag::Int(4189));
    let bytes = java_bytes("", &root);
    let mut reader = NbtReader::java(bytes.as_slice());

    assert_eq!(
        reader.next_event().expect("root"),
        Some(Event::Compound {
            name: Some(String::new())
        })
    );
    assert_eq!(
        reader.next_event().expect("blocks"),
        Some(Event::List {
            name: Some("blocks".to_owned()),
            element_type_id: 10,
            len: 3,
        }),
    );
    assert_eq!(reader.depth(), 2);
    let mut read = Vec::new();
    while let Some(Event::Compound { name: None }) = reader.next_event().expect("an item") {
        assert_eq!(reader.depth(), 3);
        read.push(reader.read_rest().expect("the item reads"));
    }
    assert_eq!(reader.depth(), 1, "the list closed");
    assert_eq!(
        read,
        blocks.into_iter().map(Tag::Compound).collect::<Vec<_>>()
    );
    assert_eq!(
        all_events(&mut reader),
        vec![
            Event::Value {
                name: Some("DataVersion".to_owned()),
                tag: Tag::Int(4189),
            },
            Event::End,
        ],
    );
}

#[test]
fn read_rest_on_an_open_list_returns_its_remaining_items() {
    let mut root = Compound::new();
    root.insert("ints", Tag::List(List::of_ints([1, 2, 3])));
    let bytes = java_bytes("", &root);
    let mut reader = NbtReader::java(bytes.as_slice());
    reader.next_event().expect("root");
    reader.next_event().expect("ints");
    reader.next_event().expect("the first item");
    assert_eq!(
        reader.read_rest().expect("the rest reads"),
        Tag::List(List::of_ints([2, 3]))
    );
    assert_eq!(all_events(&mut reader), vec![Event::End]);
}

#[test]
fn skip_rest_passes_over_a_container_and_its_arrays() {
    let mut voxel = Compound::new();
    voxel.insert("data", Tag::LongArray(vec![7; 64]));
    voxel.insert("name", Tag::String("skipped".to_owned()));
    voxel.insert("bytes", Tag::ByteArray(vec![1; 9]));
    voxel.insert("ints", Tag::IntArray(vec![1; 9]));
    let mut root = Compound::new();
    root.insert(
        "regions",
        Tag::List(List::of_compounds(vec![voxel.clone(), voxel])),
    );
    root.insert("after", Tag::Int(7));
    let bytes = bedrock_bytes("", &root);
    let mut reader = NbtReader::bedrock(bytes.as_slice());
    reader.next_event().expect("root");
    reader.next_event().expect("regions");
    reader.skip_rest().expect("the list skips");
    assert_eq!(reader.depth(), 1);
    assert_eq!(
        all_events(&mut reader),
        vec![
            Event::Value {
                name: Some("after".to_owned()),
                tag: Tag::Int(7),
            },
            Event::End,
        ],
    );
    assert_eq!(reader.offset(), bytes.len() as u64);
}

#[test]
#[should_panic(expected = "a compound or list is open")]
fn read_rest_before_the_root_opens_panics() {
    let bytes = java_bytes("", &Compound::new());
    let _ = NbtReader::java(bytes.as_slice()).read_rest();
}
