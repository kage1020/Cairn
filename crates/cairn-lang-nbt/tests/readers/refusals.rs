//! Bytes the reader refuses, each with the offset where the fault starts.

use std::io::ErrorKind;

use cairn_lang_nbt::{
    MAX_DEPTH, NbtReadError, NbtReader, read_bedrock_uncompressed, read_java_uncompressed,
};

use crate::{every_tag, java_bytes};

fn java_error(bytes: &[u8]) -> NbtReadError {
    read_java_uncompressed(bytes).expect_err("the bytes are refused")
}

/// An unnamed Java root opening one entry named `l` of type `id`.
fn root_with_entry(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![10, 0, 0, id, 0, 1, b'l'];
    bytes.extend(payload);
    bytes
}

#[test]
fn a_root_that_is_not_a_compound_is_refused() {
    let err = java_error(&[8, 0, 0, 0, 0]);
    assert!(
        matches!(err, NbtReadError::RootNotCompound { id: 8 }),
        "got {err:?}"
    );
}

#[test]
fn a_type_id_past_long_array_is_refused_where_it_sits() {
    let err = java_error(&[10, 0, 0, 13, 0, 0]);
    assert!(
        matches!(err, NbtReadError::UnknownTagType { id: 13, offset: 3 }),
        "got {err:?}"
    );

    let err = java_error(&root_with_entry(9, &[13, 0, 0, 0, 1]));
    assert!(
        matches!(err, NbtReadError::UnknownTagType { id: 13, offset: 7 }),
        "a list's element type, got {err:?}"
    );
}

#[test]
fn a_negative_length_is_refused_where_its_prefix_starts() {
    let err = java_error(&root_with_entry(9, &[3, 0xFF, 0xFF, 0xFF, 0xFF]));
    assert!(
        matches!(
            err,
            NbtReadError::NegativeLength {
                context: "list",
                len: -1,
                offset: 8,
            }
        ),
        "got {err:?}"
    );

    let err = java_error(&root_with_entry(7, &[0xFF, 0xFF, 0xFF, 0xFE]));
    assert!(
        matches!(
            err,
            NbtReadError::NegativeLength {
                context: "byte_array",
                len: -2,
                offset: 7,
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn a_non_empty_list_of_tag_end_is_refused() {
    let err = java_error(&root_with_entry(9, &[0, 0, 0, 0, 2]));
    assert!(
        matches!(err, NbtReadError::ListOfEnd { len: 2, offset: 7 }),
        "got {err:?}"
    );
}

#[test]
fn a_length_past_the_readers_limit_is_refused_before_any_item_is_read() {
    let bytes = root_with_entry(12, &[0, 0, 0, 5]);
    let mut reader = NbtReader::java(bytes.as_slice()).with_max_len(4);
    reader.next_event().expect("the root opens");
    let err = reader.next_event().expect_err("the array is too long");
    assert!(
        matches!(
            err,
            NbtReadError::TooLong {
                context: "long_array",
                len: 5,
                limit: 4,
                offset: 7,
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn nesting_past_the_limit_is_refused_where_the_tag_that_goes_past_starts() {
    // The root, then lists of one list each: the root and 511 lists reach the limit.
    let mut bytes = root_with_entry(9, &[9, 0, 0, 0, 1]);
    for _ in 0..MAX_DEPTH {
        bytes.extend([9, 0, 0, 0, 1]);
    }
    let err = java_error(&bytes);
    let offset = 12 + 5 * (MAX_DEPTH as u64 - 2);
    assert!(
        matches!(err, NbtReadError::TooDeep { limit: MAX_DEPTH, offset: at } if at == offset),
        "got {err:?}"
    );
}

#[test]
fn a_string_that_is_not_its_dialects_encoding_is_refused() {
    for (label, bytes) in [
        ("lone surrogate", &[0xED, 0xA0, 0xBD][..]),
        ("four-byte form", &[0xF0, 0x9F, 0x98, 0x80]),
        ("cut continuation", &[0xC3]),
    ] {
        let mut root = root_with_entry(8, &[0, 0]);
        root[8] = u8::try_from(bytes.len()).expect("a short string");
        root.extend(bytes);
        root.push(0);
        let err = java_error(&root);
        assert!(
            matches!(
                err,
                NbtReadError::InvalidString {
                    encoding: "Modified UTF-8",
                    offset: 7,
                }
            ),
            "{label}: got {err:?}"
        );
    }

    let err = read_bedrock_uncompressed(&[10, 0, 0, 8, 1, 0, b's', 1, 0, 0xFF, 0][..])
        .expect_err("not UTF-8");
    assert!(
        matches!(
            err,
            NbtReadError::InvalidString {
                encoding: "UTF-8",
                offset: 7,
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn input_ending_inside_the_root_is_an_unexpected_eof() {
    let bytes = java_bytes("", &every_tag());
    for cut in [1, 2, 9, bytes.len() / 2, bytes.len() - 1] {
        let err = java_error(&bytes[..cut]);
        assert!(
            matches!(&err, NbtReadError::Io(io) if io.kind() == ErrorKind::UnexpectedEof),
            "cut at {cut}: got {err:?}"
        );
    }
}

#[test]
fn input_ending_inside_a_skipped_array_is_an_unexpected_eof() {
    let bytes = root_with_entry(12, &[0, 0, 0, 2, 0, 0, 0, 0]);
    let mut reader = NbtReader::java(bytes.as_slice());
    reader.next_event().expect("the root opens");
    let err = reader.skip_rest().expect_err("the array is cut");
    assert!(
        matches!(&err, NbtReadError::Io(io) if io.kind() == ErrorKind::UnexpectedEof),
        "got {err:?}"
    );
}
