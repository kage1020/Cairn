//! The tree readers return what the tree writers wrote.

use cairn_lang_nbt::tag::{Compound, Tag};
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

fn gzip_of(root: &Compound) -> Vec<u8> {
    let mut gzip = Vec::new();
    write_java_gzip(&mut gzip, "", root).expect("the root writes");
    gzip
}

/// The `std::io::Error` `read_java_gzip` refuses `bytes` with.
fn gzip_io_error(bytes: &[u8]) -> std::io::Error {
    match read_java_gzip(bytes).expect_err("the gzip is refused") {
        NbtReadError::Io(io) => io,
        other => panic!("not a gzip failure: {other:?}"),
    }
}

#[test]
fn a_gzip_trailer_that_does_not_match_is_refused() {
    let root = every_tag();
    let gzip = gzip_of(&root);
    let mut crc = gzip.clone();
    let at = crc.len() - 8;
    crc[at] ^= 1;
    let mut size = gzip.clone();
    let at = size.len() - 1;
    size[at] ^= 1;
    let cut = &gzip[..gzip.len() - 8];
    let kinds = [crc.as_slice(), size.as_slice(), cut].map(|bytes| gzip_io_error(bytes).kind());
    assert_eq!(
        kinds,
        [
            std::io::ErrorKind::InvalidInput,
            std::io::ErrorKind::InvalidInput,
            std::io::ErrorKind::UnexpectedEof,
        ]
    );
}

#[test]
fn bytes_after_the_gzip_member_are_refused() {
    let root = every_tag();
    let mut garbage = gzip_of(&root);
    garbage.extend(b"not a gzip member");
    assert_eq!(
        gzip_io_error(&garbage).kind(),
        std::io::ErrorKind::InvalidInput
    );

    let mut second_root = gzip_of(&root);
    let end = java_bytes("", &root).len() as u64;
    second_root.extend(gzip_of(&root));
    let err = read_java_gzip(second_root.as_slice()).expect_err("a second root follows");
    assert!(
        matches!(err, NbtReadError::TrailingBytes { offset } if offset == end),
        "got {err:?}"
    );
}

#[test]
fn a_root_split_across_gzip_members_reads_as_one_stream() {
    let root = every_tag();
    let bytes = java_bytes("", &root);
    let (head, tail) = bytes.split_at(bytes.len() / 2);
    let mut members = Vec::new();
    for part in [head, tail] {
        let mut gz = flate2::write::GzEncoder::new(&mut members, flate2::Compression::default());
        std::io::Write::write_all(&mut gz, part).expect("the part compresses");
        gz.finish().expect("the member closes");
    }
    let (name, read) = read_java_gzip(members.as_slice()).expect("both members read");
    assert_eq!((name.as_str(), read), ("", root));
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
