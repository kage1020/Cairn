//! The NBT writers on the exact bytes they emit: the Java writer
//! big-endian, the Bedrock writer little-endian, and the streaming writers
//! against the tree writers.

mod bedrock;
mod java;
mod stream;

/// The offset of the first occurrence of `needle` in `haystack`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
