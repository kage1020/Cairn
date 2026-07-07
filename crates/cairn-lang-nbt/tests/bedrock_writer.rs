//! AC B1–B7 for the Bedrock NBT writer.
//!
//! Bedrock NBT shares the Java tag-id vocabulary but stores every numeric
//! payload and length prefix little-endian, and `.mcstructure` files are
//! written uncompressed (wiki.bedrock.dev, "mcstructure" format page). The
//! tests mirror `java_writer.rs` so a divergence between the two writers
//! beyond byte order fails loudly on the exact byte.

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{NbtIoError, write_bedrock_uncompressed, write_java_uncompressed};

fn encode(name: &str, root: &Compound) -> Vec<u8> {
    let mut buf = Vec::new();
    write_bedrock_uncompressed(&mut buf, name, root).expect("uncompressed write");
    buf
}

#[test]
fn b1_empty_root_emits_header_plus_end() {
    // AC B1: empty Compound root with empty name. A zero u16 length is the
    // same two bytes in either endianness, so this matches the Java bytes.
    let buf = encode("", &Compound::new());
    assert_eq!(buf, vec![0x0a, 0x00, 0x00, 0x00]);
}

#[test]
fn b2_named_int_root_matches_known_bytes() {
    // AC B2: {name: Int(42)} with root name "name", little-endian.
    // Layout:
    //   0x0a                       — root tag id (Compound)
    //   0x04 0x00 "name"           — root name, u16 length little-endian
    //   0x03                       — entry tag id (Int)
    //   0x04 0x00 "name"           — entry name
    //   0x2a 0x00 0x00 0x00        — payload 42 little-endian
    //   0x00                       — TAG_End
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
fn b3_string_length_prefix_is_little_endian() {
    // AC B3: TAG_String is u16 length (little-endian) + ASCII-range UTF-8.
    let mut root = Compound::new();
    root.insert("s", Tag::String("hi".to_owned()));
    let buf = encode("", &root);
    // Root header (3 with empty name) + entry tag id (1) + name length (2)
    // + name "s" (1) = offset 7, then the string payload.
    assert_eq!(&buf[7..11], &[0x02, 0x00, b'h', b'i']);
}

#[test]
fn b4_numeric_types_are_little_endian() {
    // AC B4: numeric payloads are little-endian regardless of host
    // endianness. Mirrors java_writer.rs::n8 with reversed expectations.
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
fn b5_int_list_and_array_lengths_are_little_endian() {
    // AC B5: the i32 length prefixes of lists and arrays flip byte order
    // alongside the element payloads.
    let mut root = Compound::new();
    root.insert("xs", Tag::List(List::of_ints([1, 2])));
    root.insert("arr", Tag::IntArray(vec![3]));
    let buf = encode("", &root);

    let find = |needle: &[u8]| -> usize { find_subsequence(&buf, needle).expect("found") };
    // List payload: element id (1) + length i32 LE + items.
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
    // IntArray payload: length i32 LE + items.
    let arr_off = find(b"arr") + 3;
    assert_eq!(
        &buf[arr_off..arr_off + 8],
        &[0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00]
    );
}

#[test]
fn b6_same_tree_through_both_writers_differs_only_in_byte_order() {
    // AC B6: the writers share structure byte-for-byte; every multi-byte
    // scalar — the entry-name u16 length prefix included — flips, and
    // nothing else moves. Pinning both full streams keeps the shared-core
    // contract honest on the exact bytes.
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
fn b7_string_and_list_validation_matches_java() {
    // AC B7: `InvalidString` and `HeterogeneousList` fire identically —
    // validation lives in the shared writer core, not per-endianness.
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

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
