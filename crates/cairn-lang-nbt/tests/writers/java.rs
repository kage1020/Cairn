//! The Java writer's big-endian byte stream, against `minecraft.wiki/w/NBT_format`.

use std::io::Read;

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{NbtIoError, write_java_gzip, write_java_uncompressed};

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

#[test]
fn string_with_nul_byte_rejected() {
    let mut root = Compound::new();
    root.insert("bad", Tag::String("a\0b".to_owned()));
    let mut buf = Vec::new();
    let err = write_java_uncompressed(&mut buf, "", &root).expect_err("nul rejected");
    match err {
        NbtIoError::InvalidString { byte, index } => {
            assert_eq!(byte, 0);
            assert_eq!(index, 1);
        }
        other => panic!("expected InvalidString, got {other:?}"),
    }
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
fn a_string_past_the_u16_length_prefix_is_a_length_overflow() {
    let long = "a".repeat(usize::from(u16::MAX) + 1);
    let mut root = Compound::new();
    root.insert("s", Tag::String(long));
    let mut buf = Vec::new();
    let err = write_java_uncompressed(&mut buf, "", &root).expect_err("too long");
    match err {
        NbtIoError::LengthOverflow {
            context,
            len,
            limit,
        } => {
            assert_eq!(context, "string");
            assert_eq!(len, usize::from(u16::MAX) + 1);
            assert_eq!(limit, usize::from(u16::MAX));
        }
        other => panic!("expected LengthOverflow, got {other:?}"),
    }
}

#[test]
fn non_ascii_string_rejected_with_offending_byte_index() {
    let mut root = Compound::new();
    root.insert("u8", Tag::String("café".to_owned()));
    let mut buf = Vec::new();
    let err = write_java_uncompressed(&mut buf, "", &root).expect_err("non ascii");
    match err {
        NbtIoError::InvalidString { byte, index } => {
            assert!(byte >= 0x80);
            assert_eq!(index, 3);
        }
        other => panic!("expected InvalidString, got {other:?}"),
    }
}
