//! AC N1–N8 for the Java NBT writer.
//!
//! Spec references for the wire format come from the Notch-era NBT spec
//! reprinted in `minecraft.wiki/w/NBT_format`. We exercise the byte stream
//! directly rather than round-tripping because there is no reader yet —
//! the binary shape is what matters for `assert_binary_snapshot` and the
//! lockfile's `resolved_ir_hash`.

use std::io::Read;

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{NbtIoError, write_java_gzip, write_java_uncompressed};

fn encode(name: &str, root: &Compound) -> Vec<u8> {
    let mut buf = Vec::new();
    write_java_uncompressed(&mut buf, name, root).expect("uncompressed write");
    buf
}

#[test]
fn n1_empty_root_emits_header_plus_end() {
    // AC N1: empty Compound root with empty name.
    // TAG_Compound (0x0a) + name length (0x0000) + TAG_End (0x00).
    let buf = encode("", &Compound::new());
    assert_eq!(buf, vec![0x0a, 0x00, 0x00, 0x00]);
}

#[test]
fn n2_named_int_root_matches_known_bytes() {
    // AC N2: {name: Int(42)} with root name "name".
    // Layout:
    //   0x0a                       — root tag id (Compound)
    //   0x00 0x04 "name"           — root name
    //   0x03                       — entry tag id (Int)
    //   0x00 0x04 "name"           — entry name (also "name" here)
    //   0x00 0x00 0x00 0x2a        — payload 42 big-endian
    //   0x00                       — TAG_End
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
fn n3_string_is_length_prefixed_utf8() {
    // AC N3: TAG_String is u16 length (big-endian) + ASCII-range UTF-8.
    let mut root = Compound::new();
    root.insert("s", Tag::String("hi".to_owned()));
    let buf = encode("", &root);
    // root header (1+2 = 3 with empty name) + entry: tag id (1) + name len
    // (2) + name "s" (1) + payload: u16 length (2) + "hi" (2) + TAG_End (1) = 12.
    assert_eq!(buf.len(), 3 + 1 + 2 + 1 + 2 + 2 + 1);
    // The string payload (length + bytes) starts at offset 3 + 4 = 7.
    assert_eq!(&buf[7..11], &[0x00, 0x02, b'h', b'i']);
}

#[test]
fn n4_empty_int_list_uses_end_element_id() {
    // AC N4: spec mandates element_type_id = 0 for an empty list.
    let mut root = Compound::new();
    root.insert("xs", Tag::List(List::empty()));
    let buf = encode("", &root);
    // After root header (3 with empty name) + entry tag id (1) + name
    // length (2) + "xs" (2) = offset 8, the list payload begins:
    // element id (1) + length (4) = five zero bytes.
    assert_eq!(&buf[8..13], &[0x00, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn n5_compound_list_elements_inline_with_end() {
    // AC N5: TAG_List<TAG_Compound> writes each element inline (no extra
    // tag id, body terminated by TAG_End).
    let mut inner = Compound::new();
    inner.insert("x", Tag::Byte(1));
    let mut root = Compound::new();
    root.insert("items", Tag::List(List::of_compounds(vec![inner])));
    let buf = encode("", &root);
    // Locate the list payload: root header (3 with empty name) + entry tag
    // (1) + name length (2) + "items" (5) = offset 11.
    let off = 3 + 1 + 2 + 5;
    assert_eq!(buf[off], 10, "element type id = TAG_Compound");
    assert_eq!(
        &buf[off + 1..off + 5],
        &[0x00, 0x00, 0x00, 0x01],
        "list length = 1"
    );
    // Inline compound starts at offset off+5: TAG_Byte (1) + name length (2)
    // + "x" (1) + payload (1) + TAG_End (1) = 6 bytes total.
    let body = &buf[off + 5..off + 5 + 6];
    assert_eq!(body, &[0x01, 0x00, 0x01, b'x', 0x01, 0x00]);
}

#[test]
fn n6_gzip_decompresses_to_uncompressed_form() {
    // AC N6: gzip-wrapped output, once decoded, is bit identical to the
    // raw NBT byte stream.
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
fn n7_string_with_nul_byte_rejected() {
    // AC N7: NUL is not encodable in Java Modified UTF-8.
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
fn n8_numeric_types_are_big_endian() {
    // AC N8: numeric payloads are big-endian regardless of host endianness.
    let mut root = Compound::new();
    root.insert("i32", Tag::Int(0x1234_5678));
    root.insert("i64", Tag::Long(0x0123_4567_89AB_CDEF_i64));
    root.insert("f32", Tag::Float(f32::from_bits(0xDEAD_BEEF)));
    root.insert("f64", Tag::Double(f64::from_bits(0xCAFE_BABE_DEAD_BEEF)));
    let buf = encode("", &root);

    // Find each named entry by scanning for its name (each unique here).
    let find = |needle: &[u8]| -> usize { find_subsequence(&buf, needle).expect("found") };
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

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[test]
fn heterogeneous_list_rejected_with_index_and_actual_type() {
    // A list declares Int but carries a String element: the writer must
    // pinpoint the offending index and the wrong type id.
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
fn length_overflow_message_names_the_context() {
    // We can't actually allocate a `Vec<i32>` of `i32::MAX + 1` to trigger
    // the overflow path, so this test only locks down the error's `context`
    // field via Display so a debug log meaningfully points at the offending
    // tag.
    let err = NbtIoError::LengthOverflow {
        context: "string",
        len: 1 << 17,
        limit: u16::MAX as usize,
    };
    let msg = err.to_string();
    assert!(msg.contains("string"), "context should appear, got: {msg}");
    assert!(msg.contains(&(u16::MAX as usize).to_string()));
}

#[test]
fn non_ascii_string_rejected_with_offending_byte_index() {
    // The Modified UTF-8 encoder for supplementary chars has not landed
    // yet, so the writer rejects any non-ASCII byte rather than silently
    // emitting invalid bytes.
    let mut root = Compound::new();
    root.insert("u8", Tag::String("café".to_owned()));
    let mut buf = Vec::new();
    let err = write_java_uncompressed(&mut buf, "", &root).expect_err("non ascii");
    match err {
        NbtIoError::InvalidString { byte, index } => {
            assert!(byte >= 0x80);
            // "café" → bytes are [0x63, 0x61, 0x66, 0xC3, 0xA9]; first
            // non-ASCII byte is at index 3.
            assert_eq!(index, 3);
        }
        other => panic!("expected InvalidString, got {other:?}"),
    }
}
