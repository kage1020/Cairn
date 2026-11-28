//! Bytes the reader refuses, each with the offset where the fault starts.

use cairn_lang_nbt::tag::Tag;
use cairn_lang_nbt::{
    MAX_DEPTH, NbtReadError, NbtReader, read_bedrock_uncompressed, read_java_uncompressed,
};

use crate::{bedrock_bytes, every_tag, java_bytes};

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
fn a_length_at_the_readers_limit_is_read() {
    let bytes = root_with_entry(12, &[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 7, 0]);
    let mut reader = NbtReader::java(bytes.as_slice()).with_max_len(1);
    reader.next_event().expect("the root opens");
    let event = reader.next_event().expect("one long is within the limit");
    assert_eq!(
        event,
        Some(cairn_lang_nbt::Event::Value {
            name: Some("l".to_owned()),
            tag: cairn_lang_nbt::Tag::LongArray(vec![7]),
        })
    );
}

#[test]
fn an_array_limit_leaves_lists_their_own() {
    let list = root_with_entry(9, &[1, 0, 0, 0, 3, 1, 2, 3, 0]);
    let mut reader = NbtReader::java(list.as_slice())
        .with_max_len(3)
        .with_max_array_len(2);
    reader.next_event().expect("the root opens");
    reader
        .skip_rest()
        .expect("a list of three is within the list limit");

    let array = root_with_entry(7, &[0, 0, 0, 3, 1, 2, 3, 0]);
    let mut reader = NbtReader::java(array.as_slice())
        .with_max_len(3)
        .with_max_array_len(2);
    reader.next_event().expect("the root opens");
    let err = reader
        .next_event()
        .expect_err("an array of three is past the array limit");
    assert!(
        matches!(
            err,
            NbtReadError::TooLong {
                context: "byte_array",
                len: 3,
                limit: 2,
                offset: 7,
            }
        ),
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
fn input_ending_inside_the_root_is_refused_where_it_ends() {
    let bytes = java_bytes("", &every_tag());
    for cut in [0, 1, 2, 9, bytes.len() / 2, bytes.len() - 1] {
        let err = java_error(&bytes[..cut]);
        assert!(
            matches!(err, NbtReadError::Truncated { offset } if offset == cut as u64),
            "cut at {cut}: got {err:?}"
        );
    }
}

#[test]
fn input_ending_inside_a_skipped_array_is_refused_where_it_ends() {
    let bytes = root_with_entry(12, &[0, 0, 0, 2, 0, 0, 0, 0]);
    let mut reader = NbtReader::java(bytes.as_slice());
    reader.next_event().expect("the root opens");
    let err = reader.skip_rest().expect_err("the array is cut");
    assert!(
        matches!(err, NbtReadError::Truncated { offset: 15 }),
        "got {err:?}"
    );
}

#[test]
fn input_past_the_root_is_refused_where_it_starts() {
    let mut java = java_bytes("", &every_tag());
    let end = java.len() as u64;
    java.push(0);
    let err = java_error(&java);
    assert!(
        matches!(err, NbtReadError::TrailingBytes { offset } if offset == end),
        "got {err:?}"
    );

    let mut bedrock = bedrock_bytes("", &every_tag());
    let end = bedrock.len() as u64;
    bedrock.extend(bedrock_bytes("", &every_tag()));
    let err = read_bedrock_uncompressed(bedrock.as_slice()).expect_err("a second root follows");
    assert!(
        matches!(err, NbtReadError::TrailingBytes { offset } if offset == end),
        "got {err:?}"
    );
}

#[test]
fn a_reader_refuses_every_call_after_one_that_failed() {
    let mut reader = NbtReader::java(&[8, 0, 0, 0, 0][..]);
    let first = reader.next_event().expect_err("the root is a string");
    assert!(
        matches!(first, NbtReadError::RootNotCompound { id: 8 }),
        "got {first:?}"
    );
    for _ in 0..2 {
        let again = reader.next_event().expect_err("the reader failed");
        assert!(
            matches!(again, NbtReadError::AfterFailure { offset: 1 }),
            "got {again:?}"
        );
    }
    let finished = reader.finish().expect_err("the reader failed");
    assert!(
        matches!(finished, NbtReadError::AfterFailure { offset: 1 }),
        "got {finished:?}"
    );

    // The list's type and length were consumed, so a retry would read its payload as a tag.
    let bytes = root_with_entry(9, &[3, 0xFF, 0xFF, 0xFF, 0xFF, 0]);
    let mut reader = NbtReader::java(bytes.as_slice());
    reader.next_event().expect("the root opens");
    let first = reader
        .next_event()
        .expect_err("the list's length is negative");
    assert!(
        matches!(first, NbtReadError::NegativeLength { offset: 8, .. }),
        "got {first:?}"
    );
    let again = reader.next_event().expect_err("the reader failed");
    assert!(
        matches!(again, NbtReadError::AfterFailure { offset: 12 }),
        "got {again:?}"
    );
}

#[test]
#[should_panic(expected = "`finish` is called once the root has closed")]
fn finishing_a_reader_whose_root_is_open_panics() {
    let bytes = java_bytes("", &every_tag());
    let mut reader = NbtReader::java(bytes.as_slice());
    reader.next_event().expect("the root opens");
    let _ = reader.finish();
}

/// The tree `bytes` holds under its root, read with [`NbtReader::read_rest_within`] at `limit`.
fn within(bytes: &[u8], limit: usize) -> Result<(Tag, usize), NbtReadError> {
    let mut reader = NbtReader::java(bytes);
    reader.next_event().expect("the root opens");
    reader.read_rest_within(limit)
}

#[test]
fn a_tree_read_within_exactly_what_it_takes_reads_and_one_byte_less_is_refused() {
    let bytes = java_bytes("", &every_tag());
    let (tree, taken) = within(&bytes, usize::MAX).expect("the tree reads");
    assert_eq!(tree, Tag::Compound(every_tag()));
    assert_eq!(
        within(&bytes, taken).expect("the tree reads"),
        (tree, taken)
    );
    assert!(
        matches!(within(&bytes, taken - 1), Err(NbtReadError::HeldTooMuch { limit, .. }) if limit == taken - 1),
        "one byte less is refused"
    );
}

#[test]
fn an_array_past_the_limit_is_refused_at_its_prefix_and_the_reader_reads_no_further() {
    let mut bytes = root_with_entry(10, &[7, 0, 1, b'a']);
    bytes.extend(2000_i32.to_be_bytes());
    bytes.extend([0; 2000]);
    bytes.extend([0, 0]);
    let mut reader = NbtReader::java(bytes.as_slice());
    reader.next_event().expect("the root opens");
    reader.next_event().expect("`l` opens");
    let err = reader
        .read_rest_within(1000)
        .expect_err("the array is refused");
    assert!(
        matches!(
            err,
            NbtReadError::HeldTooMuch {
                limit: 1000,
                offset: 15,
            }
        ),
        "got {err:?}"
    );
    let err = reader
        .next_event()
        .expect_err("the reader reads no further");
    assert!(
        matches!(err, NbtReadError::AfterFailure { offset: 15 }),
        "got {err:?}"
    );
}

#[test]
fn a_list_is_charged_for_its_tags_not_for_the_bytes_that_encode_them() {
    let len: i32 = 1 << 16;
    let mut bytes = root_with_entry(9, &[1]);
    bytes.extend(len.to_be_bytes());
    bytes.extend(vec![0; usize::try_from(len).expect("a length")]);
    bytes.push(0);
    let limit = usize::try_from(len).expect("a length") * 8;
    let mut reader = NbtReader::java(bytes.as_slice());
    reader.next_event().expect("the root opens");
    reader.next_event().expect("`l` opens");
    assert!(
        matches!(
            reader.read_rest_within(limit),
            Err(NbtReadError::HeldTooMuch { limit: at, .. }) if at == limit
        ),
        "a list of 65536 one-byte tags takes more than 8 bytes a tag"
    );
}
