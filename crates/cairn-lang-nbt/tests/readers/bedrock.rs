//! The Bedrock reader: little-endian, uncompressed, strings in UTF-8.

use cairn_lang_nbt::tag::Tag;
use cairn_lang_nbt::{NbtReadError, read_bedrock_uncompressed, write_bedrock_uncompressed};

use crate::{every_tag, keep_all, read_back};

#[test]
fn reads_back_every_tag_the_writer_writes() {
    let mut bytes = Vec::new();
    write_bedrock_uncompressed(&mut bytes, "", &every_tag()).expect("write");
    let read = read_bedrock_uncompressed(bytes.as_slice(), &keep_all()).expect("read");
    assert_eq!(read, read_back("", every_tag()));
}

#[test]
fn reads_multi_byte_scalars_little_endian() {
    let bytes = [
        &[0x0a, 0, 0][..],
        &[0x03, 1, 0, b'i', 0x55, 0x10, 0, 0],
        &[0x02, 1, 0, b's', 0x34, 0x12],
        &[0],
    ]
    .concat();
    let read = read_bedrock_uncompressed(bytes.as_slice(), &keep_all()).expect("read");
    assert_eq!(read.compound.entries["i"], Tag::Int(0x1055));
    assert_eq!(read.compound.entries["s"], Tag::Short(0x1234));
}

#[test]
fn strings_are_utf8() {
    let string_root = |value: &[u8]| {
        let len = u16::try_from(value.len()).expect("short").to_le_bytes();
        [&[0x0a, 0, 0, 0x08, 1, 0, b's'], &len[..], value, &[0]].concat()
    };
    let read = read_bedrock_uncompressed(string_root("😀".as_bytes()).as_slice(), &keep_all())
        .expect("read");
    assert_eq!(read.compound.entries["s"], Tag::String("😀".to_owned()));
    let err = read_bedrock_uncompressed(string_root(&[0xC0, 0x80]).as_slice(), &keep_all())
        .expect_err("refused");
    assert!(
        matches!(err, NbtReadError::InvalidString { encoding: "UTF-8" }),
        "{err:?}"
    );
}
