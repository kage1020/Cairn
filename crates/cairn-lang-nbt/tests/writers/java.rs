//! The Java writer's big-endian byte stream, against `minecraft.wiki/w/NBT_format`.

use std::io::Read;

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{
    NbtIoError, check_bedrock_string, check_java_string, read_java_uncompressed, write_java_gzip,
    write_java_uncompressed,
};

use crate::find_subsequence;

fn encode(name: &str, root: &Compound) -> Vec<u8> {
    let mut buf = Vec::new();
    write_java_uncompressed(&mut buf, name, root).expect("uncompressed write");
    buf
}

#[test]
fn empty_root_emits_header_plus_end() {
    let buf = encode("", &Compound::new());
    assert_eq!(buf, vec![0x0a, 0x00, 0x00, 0x00]);
}

#[test]
fn named_int_root_matches_known_bytes() {
    let mut root = Compound::new();
    root.insert("name", Tag::Int(42));
    let buf = encode("name", &root);
    let expected: Vec<u8> = [
        &[0x0a_u8] as &[u8],
        &[0x00, 0x04],
        b"name",
        &[0x03],
        &[0x00, 0x04],
        b"name",
        &[0x00, 0x00, 0x00, 0x2a],
        &[0x00],
    ]
    .concat();
    assert_eq!(buf, expected);
}

#[test]
fn string_is_length_prefixed_utf8() {
    let mut root = Compound::new();
    root.insert("s", Tag::String("hi".to_owned()));
    let buf = encode("", &root);
    // Root header, entry id, name length, "s", string length, "hi", TAG_End.
    assert_eq!(buf.len(), 3 + 1 + 2 + 1 + 2 + 2 + 1);
    assert_eq!(&buf[7..11], &[0x00, 0x02, b'h', b'i']);
}

#[test]
fn empty_int_list_uses_end_element_id() {
    let mut root = Compound::new();
    root.insert("xs", Tag::List(List::empty()));
    let buf = encode("", &root);
    // Offset 8 is past the root header and the entry's id and name.
    assert_eq!(&buf[8..13], &[0x00, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn compound_list_elements_inline_with_end() {
    let mut inner = Compound::new();
    inner.insert("x", Tag::Byte(1));
    let mut root = Compound::new();
    root.insert("items", Tag::List(List::of_compounds(vec![inner])));
    let buf = encode("", &root);
    // Past the root header, entry id, name length and "items".
    let off = 3 + 1 + 2 + 5;
    assert_eq!(buf[off], 10, "element type id = TAG_Compound");
    assert_eq!(
        &buf[off + 1..off + 5],
        &[0x00, 0x00, 0x00, 0x01],
        "list length = 1"
    );
    let body = &buf[off + 5..off + 5 + 6];
    assert_eq!(body, &[0x01, 0x00, 0x01, b'x', 0x01, 0x00]);
}

#[test]
fn gzip_decompresses_to_uncompressed_form() {
    let mut root = Compound::new();
    root.insert("DataVersion", Tag::Int(3955));
    root.insert("size", Tag::List(List::of_ints([1, 1, 1])));

    let mut gz_buf = Vec::new();
    write_java_gzip(&mut gz_buf, "", &root).expect("gzip write");
    let raw_buf = encode("", &root);

    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(gz_buf.as_slice())
        .read_to_end(&mut decoded)
        .expect("gzip decode");
    assert_eq!(decoded, raw_buf);
}

/// The bytes `text` is written as, after its length prefix, as the one string of a root.
fn string_bytes(text: &str) -> Vec<u8> {
    crate::string_bytes(write_java_uncompressed, u16::from_be_bytes, text)
}

#[test]
fn a_string_is_written_as_modified_utf8() {
    assert_eq!(string_bytes("a\0b"), [b'a', 0xC0, 0x80, b'b']);
    assert_eq!(string_bytes("Café"), "Café".as_bytes());
    assert_eq!(string_bytes("日本"), "日本".as_bytes());
    // U+1F600 as its surrogate pair, U+D83D U+DE00, three bytes each.
    assert_eq!(string_bytes("😀"), [0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]);
    // The NUL sends every character after it through the UTF-16 encoder too.
    assert_eq!(
        string_bytes("\0\u{7f}\u{80}\u{7ff}\u{800}\u{ffff}"),
        [
            0xC0, 0x80, 0x7F, 0xC2, 0x80, 0xDF, 0xBF, 0xE0, 0xA0, 0x80, 0xEF, 0xBF, 0xBF
        ]
    );
    assert_eq!(
        string_bytes("\u{10000}"),
        [0xED, 0xA0, 0x80, 0xED, 0xB0, 0x80]
    );
    assert_eq!(
        string_bytes("\u{10ffff}"),
        [0xED, 0xAF, 0xBF, 0xED, 0xBF, 0xBF]
    );
}

#[test]
fn every_string_round_trips_through_the_java_reader() {
    let mut root = Compound::new();
    for (name, text) in [
        ("nul", "a\0b"),
        ("latin", "Café"),
        ("cjk", "看板の文字"),
        ("astral", "x😀y"),
    ] {
        root.insert(name, Tag::String(text.to_owned()));
    }
    let (_, read) = read_java_uncompressed(encode("", &root).as_slice()).expect("reads back");
    assert_eq!(read, root);
}

#[test]
fn numeric_types_are_big_endian() {
    let mut root = Compound::new();
    root.insert("i16", Tag::Short(0x1234));
    root.insert("i32", Tag::Int(0x1234_5678));
    root.insert("i64", Tag::Long(0x0123_4567_89AB_CDEF_i64));
    root.insert("f32", Tag::Float(f32::from_bits(0xDEAD_BEEF)));
    root.insert("f64", Tag::Double(f64::from_bits(0xCAFE_BABE_DEAD_BEEF)));
    let buf = encode("", &root);

    let find = |needle: &[u8]| -> usize { find_subsequence(&buf, needle).expect("found") };
    let i16_off = find(b"i16") + 3;
    assert_eq!(&buf[i16_off..i16_off + 2], &[0x12, 0x34]);
    let i32_off = find(b"i32") + 3;
    assert_eq!(&buf[i32_off..i32_off + 4], &[0x12, 0x34, 0x56, 0x78]);
    let i64_off = find(b"i64") + 3;
    assert_eq!(
        &buf[i64_off..i64_off + 8],
        &[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]
    );
    let f32_off = find(b"f32") + 3;
    assert_eq!(&buf[f32_off..f32_off + 4], &[0xDE, 0xAD, 0xBE, 0xEF]);
    let f64_off = find(b"f64") + 3;
    assert_eq!(
        &buf[f64_off..f64_off + 8],
        &[0xCA, 0xFE, 0xBA, 0xBE, 0xDE, 0xAD, 0xBE, 0xEF]
    );
}

#[test]
fn heterogeneous_list_rejected_with_index_and_actual_type() {
    let mut root = Compound::new();
    root.insert(
        "xs",
        Tag::List(List {
            element_type_id: 3, // Int
            items: vec![Tag::Int(1), Tag::String("oops".to_owned())],
        }),
    );
    let mut buf = Vec::new();
    let err = write_java_uncompressed(&mut buf, "", &root).expect_err("heterogeneous");
    match err {
        NbtIoError::HeterogeneousList {
            declared,
            index,
            actual,
        } => {
            assert_eq!(declared, 3);
            assert_eq!(index, 1);
            assert_eq!(actual, 8);
        }
        other => panic!("expected HeterogeneousList, got {other:?}"),
    }
}

#[test]
fn an_empty_list_claiming_an_element_type_is_rejected() {
    // A struct literal bypasses `List::of_tags`.
    let mut root = Compound::new();
    root.insert(
        "xs",
        Tag::List(List {
            element_type_id: 3, // Int, with nothing to be an Int
            items: vec![],
        }),
    );
    let mut buf = Vec::new();
    let err = write_java_uncompressed(&mut buf, "", &root).expect_err("empty list, typed");
    match err {
        NbtIoError::EmptyListWithElementType { declared } => assert_eq!(declared, 3),
        other => panic!("expected EmptyListWithElementType, got {other:?}"),
    }
    let mut ok_root = Compound::new();
    ok_root.insert("xs", Tag::List(List::empty()));
    let mut ok_buf = Vec::new();
    write_java_uncompressed(&mut ok_buf, "", &ok_root).expect("TAG_End empty list");
}

#[test]
fn a_string_holds_65535_bytes_of_modified_utf8_and_no_more() {
    let fits = "a".repeat(usize::from(u16::MAX));
    assert_eq!(string_bytes(&fits), fits.as_bytes());
    let mut root = Compound::new();
    root.insert("s", Tag::String(format!("{fits}a")));
    let err = write_java_uncompressed(&mut Vec::new(), "", &root).expect_err("too long");
    assert_eq!(
        err.to_string(),
        "nbt: the string takes 65536 bytes of Modified UTF-8, past the 65535 an NBT string holds"
    );
    assert!(
        matches!(
            err,
            NbtIoError::StringTooLong {
                len: 65_536,
                encoding: "Modified UTF-8"
            }
        ),
        "{err:?}"
    );
}

#[test]
fn the_length_limit_counts_the_modified_utf8_bytes() {
    // An astral character is 4 bytes of UTF-8 and 6 of Modified UTF-8, a NUL 1 and 2.
    for (fits, refused, len) in [
        ("😀".repeat(10_922), "😀".repeat(10_923), 65_538),
        ("\0".repeat(32_767), "\0".repeat(32_768), 65_536),
    ] {
        assert!(check_java_string(&fits).is_ok());
        assert!(check_bedrock_string(&refused).is_ok());
        match check_java_string(&refused) {
            Err(NbtIoError::StringTooLong {
                len: counted,
                encoding: "Modified UTF-8",
            }) => assert_eq!(counted, len),
            other => panic!("expected StringTooLong, got {other:?}"),
        }
    }
}
