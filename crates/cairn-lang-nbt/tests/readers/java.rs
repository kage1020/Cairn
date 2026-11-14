//! The Java reader: big-endian, gzip-wrapped or not, strings in Modified UTF-8.

use cairn_lang_nbt::tag::{Compound, Tag};
use cairn_lang_nbt::{
    NbtReadError, NbtRoot, ReadOptions, read_java_gzip, read_java_uncompressed, write_java_gzip,
    write_java_uncompressed,
};

use crate::{ROOMY, every_tag, keep_all, read_back};

fn encode(name: &str, root: &Compound) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_java_uncompressed(&mut bytes, name, root).expect("uncompressed write");
    bytes
}

/// A root holding one string entry `s`, its value given as raw bytes.
fn string_root(value: &[u8]) -> Vec<u8> {
    let len = u16::try_from(value.len()).expect("short").to_be_bytes();
    [&[0x0a, 0, 0, 0x08, 0, 1, b's'], &len[..], value, &[0]].concat()
}

#[test]
fn reads_back_every_tag_the_writer_writes() {
    let bytes = encode("root", &every_tag());
    let read = read_java_uncompressed(bytes.as_slice(), &keep_all()).expect("read");
    assert_eq!(read, read_back("root", every_tag()));
}

#[test]
fn reads_back_a_gzip_wrapped_root() {
    let mut bytes = Vec::new();
    write_java_gzip(&mut bytes, "", &every_tag()).expect("gzip write");
    let read = read_java_gzip(bytes.as_slice(), &keep_all()).expect("read");
    assert_eq!(read, read_back("", every_tag()));
}

#[test]
fn keeps_only_the_named_root_entries_and_lists_the_rest() {
    let bytes = encode("", &every_tag());
    let options = ReadOptions {
        keep: Some(&["int", "longs", "absent"]),
        max_tree_bytes: ROOMY,
    };
    let read = read_java_uncompressed(bytes.as_slice(), &options).expect("read");
    let mut kept = Compound::new();
    kept.insert("int", Tag::Int(70_000))
        .insert("longs", Tag::LongArray(vec![i64::MIN, i64::MAX]));
    assert_eq!(
        read,
        NbtRoot {
            name: String::new(),
            compound: kept,
            skipped: [
                ("byte", 1),
                ("short", 2),
                ("long", 4),
                ("float", 5),
                ("double", 6),
                ("bytes", 7),
                ("string", 8),
                ("list", 9),
                ("compound", 10),
                ("ints", 11),
                ("empty", 9),
            ]
            .map(|(name, id)| (name.to_owned(), id))
            .to_vec(),
        }
    );
}

#[test]
fn a_skipped_entry_costs_its_name_alone() {
    let mut root = Compound::new();
    root.insert("blocks", Tag::ByteArray(vec![0; 4096]))
        .insert("DataVersion", Tag::Int(4189));
    let options = ReadOptions {
        keep: Some(&["DataVersion"]),
        max_tree_bytes: 256,
    };
    let read = read_java_uncompressed(encode("", &root).as_slice(), &options).expect("read");
    assert_eq!(read.compound.entries["DataVersion"], Tag::Int(4189));
    assert_eq!(read.skipped, vec![("blocks".to_owned(), 7)]);
}

#[test]
fn decodes_modified_utf8() {
    for (bytes, expected) in [
        (&b"plain"[..], "plain"),
        (&[0xC3, 0xA9][..], "é"),
        (&[0xE6, 0x97, 0xA5][..], "日"),
        (&[0xC0, 0x80][..], "\0"),
        (&[0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80][..], "😀"),
    ] {
        let read = read_java_uncompressed(string_root(bytes).as_slice(), &keep_all())
            .unwrap_or_else(|err| panic!("{bytes:x?}: {err}"));
        assert_eq!(
            read.compound.entries["s"],
            Tag::String(expected.to_owned()),
            "{bytes:x?}"
        );
    }
}

#[test]
fn refuses_what_modified_utf8_never_writes() {
    for bytes in [
        &[0x00][..],
        &[0xF0, 0x9F, 0x98, 0x80][..],
        &[0x80][..],
        &[0xC3][..],
        &[0xED, 0xA0, 0xBD][..],
    ] {
        let err = read_java_uncompressed(string_root(bytes).as_slice(), &keep_all())
            .expect_err("refused");
        assert!(
            matches!(
                err,
                NbtReadError::InvalidString {
                    encoding: "Modified UTF-8"
                }
            ),
            "{bytes:x?}: {err:?}"
        );
    }
}

#[test]
fn refuses_bytes_that_are_not_gzip() {
    let err =
        read_java_gzip(encode("", &every_tag()).as_slice(), &keep_all()).expect_err("not gzip");
    assert!(matches!(err, NbtReadError::Io(_)), "{err:?}");
}
