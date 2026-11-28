//! The NBT writers on the exact bytes they emit.

use cairn_lang_nbt::NbtIoError;
use cairn_lang_nbt::tag::{Compound, Tag};

mod bedrock;
mod java;
mod stream;

/// A tree writer of one dialect, as `write_java_uncompressed` and `write_bedrock_uncompressed` are.
type WriteRoot = fn(&mut Vec<u8>, &str, &Compound) -> Result<(), NbtIoError>;

/// The bytes `write` puts after the length prefix, which `prefix` reads, of `text` as the one
/// string of a root.
fn string_bytes(write: WriteRoot, prefix: fn([u8; 2]) -> u16, text: &str) -> Vec<u8> {
    let mut root = Compound::new();
    root.insert("s", Tag::String(text.to_owned()));
    let mut buf = Vec::new();
    write(&mut buf, "", &root).expect("the string writes");
    // Root id and empty name (3), the entry's id and name `s` (4), then the prefix (2).
    let len = usize::from(prefix([buf[7], buf[8]]));
    assert_eq!(
        buf.len(),
        9 + len + 1,
        "the root's TAG_End follows the string"
    );
    buf[9..9 + len].to_vec()
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
