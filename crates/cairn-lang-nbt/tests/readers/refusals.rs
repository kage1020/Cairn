//! Bytes no writer emits, each refused with the error that names what is wrong, whether the entry
//! holding them is built or skipped.

use cairn_lang_nbt::tag::{List, Tag};
use cairn_lang_nbt::{MAX_DEPTH, NbtReadError, ReadOptions, read_java_uncompressed};

use crate::{ROOMY, keep_all};

/// Every root entry skipped.
const SKIP_ALL: ReadOptions<'static> = ReadOptions {
    keep: Some(&[]),
    max_tree_bytes: ROOMY,
};

/// An unnamed root holding one entry `x` of tag `id` whose payload is `payload`.
fn root_with(id: u8, payload: &[u8]) -> Vec<u8> {
    [&[0x0a, 0, 0, id, 0, 1, b'x'], payload, &[0]].concat()
}

/// A root whose entry `x` is `lists` lists, each the only item of the one around it.
fn nested_lists(lists: usize) -> Vec<u8> {
    let mut payload = [9, 0, 0, 0, 1].repeat(lists - 1);
    payload.extend([0, 0, 0, 0, 0]);
    root_with(9, &payload)
}

/// The refusal both a built and a skipped read give `bytes`.
fn refusal(bytes: &[u8]) -> (NbtReadError, NbtReadError) {
    let built = read_java_uncompressed(bytes, &keep_all()).expect_err("refused when built");
    let skipped = read_java_uncompressed(bytes, &SKIP_ALL).expect_err("refused when skipped");
    (built, skipped)
}

#[test]
fn a_root_that_is_not_a_compound() {
    let err = read_java_uncompressed(&[0x08, 0, 0, 0, 0][..], &keep_all()).expect_err("refused");
    assert!(
        matches!(err, NbtReadError::RootNotCompound { id: 8 }),
        "{err:?}"
    );
}

#[test]
fn a_tag_id_that_does_not_exist() {
    for bytes in [root_with(13, &[]), root_with(9, &[13, 0, 0, 0, 0])] {
        let (built, skipped) = refusal(&bytes);
        for err in [built, skipped] {
            assert!(
                matches!(err, NbtReadError::UnknownTagId { id: 13 }),
                "{err:?}"
            );
        }
    }
}

#[test]
fn a_negative_length() {
    for (id, payload, context) in [
        (9, &[3, 0xFF, 0xFF, 0xFF, 0xFF][..], "list"),
        (7, &[0xFF, 0xFF, 0xFF, 0xFE][..], "byte_array"),
        (11, &[0xFF, 0xFF, 0xFF, 0xFF][..], "int_array"),
        (12, &[0x80, 0, 0, 0][..], "long_array"),
    ] {
        let (built, skipped) = refusal(&root_with(id, payload));
        for err in [built, skipped] {
            assert!(
                matches!(err, NbtReadError::NegativeLength { context: c, .. } if c == context),
                "{context}: {err:?}"
            );
        }
    }
}

#[test]
fn items_of_no_type() {
    let (built, skipped) = refusal(&root_with(9, &[0, 0, 0, 0, 2]));
    for err in [built, skipped] {
        assert!(
            matches!(err, NbtReadError::UntypedList { len: 2 }),
            "{err:?}"
        );
    }
}

#[test]
fn an_empty_list_of_a_declared_type_reads_as_empty() {
    let read = read_java_uncompressed(root_with(9, &[3, 0, 0, 0, 0]).as_slice(), &keep_all())
        .expect("read");
    assert_eq!(read.compound.entries["x"], Tag::List(List::empty()));
}

#[test]
fn nesting_up_to_the_games_depth_and_no_deeper() {
    let deepest = MAX_DEPTH - 1;
    for options in [keep_all(), SKIP_ALL] {
        read_java_uncompressed(nested_lists(deepest).as_slice(), &options)
            .expect("the deepest nesting the game reads");
    }
    let (built, skipped) = refusal(&nested_lists(deepest + 1));
    for err in [built, skipped] {
        assert!(matches!(err, NbtReadError::TooDeep), "{err:?}");
    }
}

#[test]
fn a_tree_past_its_budget() {
    let bytes = root_with(7, &[[0, 0, 0x10, 0].as_slice(), &[0; 4096]].concat());
    let options = ReadOptions {
        keep: None,
        max_tree_bytes: 4096,
    };
    let err = read_java_uncompressed(bytes.as_slice(), &options).expect_err("refused");
    assert!(
        matches!(err, NbtReadError::TooLarge { limit: 4096 }),
        "{err:?}"
    );
}

#[test]
fn input_that_ends_inside_a_tag() {
    for bytes in [
        root_with(3, &[0, 0]),
        root_with(7, &[0, 0, 0, 4, 1, 2]),
        root_with(11, &[0, 0, 0, 2, 0, 0, 0, 1]),
        root_with(8, &[0, 5, b'a']),
        vec![0x0a, 0, 0, 3, 0, 1, b'x', 0, 0, 0, 1],
    ] {
        let (built, skipped) = refusal(&bytes);
        for err in [built, skipped] {
            assert!(
                matches!(err, NbtReadError::Truncated),
                "{bytes:x?}: {err:?}"
            );
        }
    }
}
