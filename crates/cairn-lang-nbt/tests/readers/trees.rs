//! The tree readers return what the tree writers wrote.

use cairn_lang_nbt::tag::Tag;
use cairn_lang_nbt::{
    NbtReadError, read_bedrock_uncompressed, read_java_gzip, read_java_uncompressed,
    write_java_gzip,
};

use crate::{bedrock_bytes, every_tag, java_bytes};

#[test]
fn every_tag_type_reads_back_as_written_in_both_dialects() {
    let root = every_tag();

    let (name, read) =
        read_java_uncompressed(java_bytes("root", &root).as_slice()).expect("java reads");
    assert_eq!((name.as_str(), &read), ("root", &root), "big-endian");

    let (name, read) =
        read_bedrock_uncompressed(bedrock_bytes("", &root).as_slice()).expect("bedrock reads");
    assert_eq!((name.as_str(), &read), ("", &root), "little-endian");
}

#[test]
fn a_gzip_root_reads_back_as_written() {
    let root = every_tag();
    let mut gzip = Vec::new();
    write_java_gzip(&mut gzip, "", &root).expect("the root writes");
    let (name, read) = read_java_gzip(gzip.as_slice()).expect("gzip reads");
    assert_eq!((name.as_str(), read), ("", root));
}

#[test]
fn input_that_is_not_gzip_is_an_io_error() {
    let err = read_java_gzip(java_bytes("", &every_tag()).as_slice()).expect_err("not gzip");
    assert!(matches!(err, NbtReadError::Io(_)), "got {err:?}");
}

/// A Java root `{s: <bytes>}` under the empty name, its string's bytes as given.
fn java_string_root(bytes: &[u8]) -> Vec<u8> {
    let len = u16::try_from(bytes.len()).expect("a short string");
    let mut root = vec![10, 0, 0, 8, 0, 1, b's'];
    root.extend(len.to_be_bytes());
    root.extend(bytes);
    root.push(0);
    root
}

#[test]
fn java_strings_decode_as_modified_utf_8() {
    for (label, bytes, expected) in [
        ("two-byte NUL", &[0xC0, 0x80][..], "\0"),
        ("two-byte character", &[0xC3, 0xA9], "\u{e9}"),
        ("three-byte character", &[0xE3, 0x81, 0x82], "\u{3042}"),
        (
            "surrogate pair",
            &[0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80],
            "\u{1F600}",
        ),
    ] {
        let (_, root) = read_java_uncompressed(java_string_root(bytes).as_slice())
            .unwrap_or_else(|err| panic!("{label}: {err}"));
        assert_eq!(
            root.entries["s"],
            Tag::String(expected.to_owned()),
            "{label}"
        );
    }
}

#[test]
fn bedrock_strings_decode_as_utf_8() {
    let mut bytes = vec![10, 0, 0, 8, 1, 0, b's', 4, 0];
    bytes.extend("\u{1F600}".as_bytes());
    bytes.push(0);
    let (_, root) = read_bedrock_uncompressed(bytes.as_slice()).expect("utf-8 reads");
    assert_eq!(root.entries["s"], Tag::String("\u{1F600}".to_owned()));
}
