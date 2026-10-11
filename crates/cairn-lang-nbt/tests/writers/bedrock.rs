//! The Bedrock writer's little-endian byte stream.

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{NbtIoError, write_bedrock_uncompressed, write_java_uncompressed};

use crate::find_subsequence;

fn encode(name: &str, root: &Compound) -> Vec<u8> {
    let mut buf = Vec::new();
    write_bedrock_uncompressed(&mut buf, name, root).expect("uncompressed write");
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
        &[0x04, 0x00],
        b"name",
        &[0x03],
        &[0x04, 0x00],
        b"name",
        &[0x2a, 0x00, 0x00, 0x00],
        &[0x00],
    ]
    .concat();
    assert_eq!(buf, expected);
}

#[test]
fn string_length_prefix_is_little_endian() {
    let mut root = Compound::new();
    root.insert("s", Tag::String("hi".to_owned()));
    let buf = encode("", &root);
    // Offset 7 is past the root header and the entry's id and name.
    assert_eq!(&buf[7..11], &[0x02, 0x00, b'h', b'i']);
}

#[test]
fn numeric_types_are_little_endian() {
    let mut root = Compound::new();
    root.insert("i16", Tag::Short(0x1234));
    root.insert("i32", Tag::Int(0x1234_5678));
    root.insert("i64", Tag::Long(0x0123_4567_89AB_CDEF_i64));
    root.insert("f32", Tag::Float(f32::from_bits(0xDEAD_BEEF)));
    root.insert("f64", Tag::Double(f64::from_bits(0xCAFE_BABE_DEAD_BEEF)));
    let buf = encode("", &root);

    let find = |needle: &[u8]| -> usize { find_subsequence(&buf, needle).expect("found") };
    let i16_off = find(b"i16") + 3;
    assert_eq!(&buf[i16_off..i16_off + 2], &[0x34, 0x12]);
    let i32_off = find(b"i32") + 3;
    assert_eq!(&buf[i32_off..i32_off + 4], &[0x78, 0x56, 0x34, 0x12]);
    let i64_off = find(b"i64") + 3;
    assert_eq!(
        &buf[i64_off..i64_off + 8],
        &[0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01]
    );
    let f32_off = find(b"f32") + 3;
    assert_eq!(&buf[f32_off..f32_off + 4], &[0xEF, 0xBE, 0xAD, 0xDE]);
    let f64_off = find(b"f64") + 3;
    assert_eq!(
        &buf[f64_off..f64_off + 8],
        &[0xEF, 0xBE, 0xAD, 0xDE, 0xBE, 0xBA, 0xFE, 0xCA]
    );
}

#[test]
fn int_list_and_array_lengths_are_little_endian() {
    let mut root = Compound::new();
    root.insert("xs", Tag::List(List::of_ints([1, 2])));
    root.insert("arr", Tag::IntArray(vec![3]));
    let buf = encode("", &root);

    let find = |needle: &[u8]| -> usize { find_subsequence(&buf, needle).expect("found") };
    let xs_off = find(b"xs") + 2;
    assert_eq!(
        &buf[xs_off..xs_off + 13],
        &[
            0x03, // element id Int
            0x02, 0x00, 0x00, 0x00, // length 2 LE
            0x01, 0x00, 0x00, 0x00, // item 1 LE
            0x02, 0x00, 0x00, 0x00, // item 2 LE
        ]
    );
    let arr_off = find(b"arr") + 3;
    assert_eq!(
        &buf[arr_off..arr_off + 8],
        &[0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00]
    );
}

#[test]
fn same_tree_through_both_writers_differs_only_in_byte_order() {
    let mut root = Compound::new();
    root.insert("i", Tag::Int(0x0102_0304));
    let mut java = Vec::new();
    write_java_uncompressed(&mut java, "", &root).expect("java write");
    let bedrock = encode("", &root);
    assert_eq!(
        java,
        vec![
            0x0a, 0x00, 0x00, 0x03, 0x00, 0x01, b'i', 0x01, 0x02, 0x03, 0x04, 0x00
        ]
    );
    assert_eq!(
        bedrock,
        vec![
            0x0a, 0x00, 0x00, 0x03, 0x01, 0x00, b'i', 0x04, 0x03, 0x02, 0x01, 0x00
        ]
    );
}

#[test]
fn string_and_list_validation_matches_java() {
    let mut root = Compound::new();
    root.insert("bad", Tag::String("a\0b".to_owned()));
    let mut buf = Vec::new();
    let err = write_bedrock_uncompressed(&mut buf, "", &root).expect_err("nul rejected");
    match err {
        NbtIoError::InvalidString { byte, index } => {
            assert_eq!(byte, 0);
            assert_eq!(index, 1);
        }
        other => panic!("expected InvalidString, got {other:?}"),
    }

    let mut root = Compound::new();
    root.insert(
        "xs",
        Tag::List(List {
            element_type_id: 3, // Int
            items: vec![Tag::Int(1), Tag::String("oops".to_owned())],
        }),
    );
    let mut buf = Vec::new();
    let err = write_bedrock_uncompressed(&mut buf, "", &root).expect_err("heterogeneous");
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
