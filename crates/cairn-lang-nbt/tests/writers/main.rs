//! The NBT writers on the exact bytes they emit.

mod bedrock;
mod java;
mod stream;

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
