//! sha256 over the manifest bytes + each named component, in declared
//! order. The output is the value that lands in the lockfile under
//! `inputs.registry_pack_hash`.

use cairn_lang_core::lock::HashHex;

/// Width of the length prefix in [`push_framed`]. Fixed rather than
/// `usize`, so the digest does not depend on the pointer width of the
/// machine that computed it.
const LENGTH_PREFIX: usize = size_of::<u64>();

/// Append `bytes` preceded by its length, so no rearrangement of the same
/// bytes across two fields produces the same stream.
fn push_framed(buf: &mut Vec<u8>, bytes: &[u8]) {
    // `usize` is at most 64 bits on every target Rust supports, so this
    // conversion is the identity in practice; `try_from` rather than `as`
    // says that a length is being written, not narrowed.
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(bytes);
}

/// Hash the manifest body and every referenced component file in declared
/// order, so two packs produce the same digest exactly when they hold the
/// same bytes under the same filenames.
///
/// Every field is length-prefixed. Concatenation alone is not enough: with
/// the fields simply run together, the same bytes divided one place to the
/// left or right frame identically, and the digest cannot tell a rename
/// from a content edit that compensates for it. That is not theoretical —
/// it held between the manifest and the first component name, and again
/// between one component's body and the next component's name, so a single
/// separator would have closed only half of it. Both are pinned as tests.
#[must_use]
pub fn pack_hash(manifest_bytes: &[u8], components: &[(&str, &[u8])]) -> HashHex {
    let extra: usize = components
        .iter()
        .map(|(name, body)| name.len() + body.len() + 2 * LENGTH_PREFIX)
        .sum();
    let mut buf: Vec<u8> = Vec::with_capacity(manifest_bytes.len() + LENGTH_PREFIX + extra);
    push_framed(&mut buf, manifest_bytes);
    for (name, body) in components {
        push_framed(&mut buf, name.as_bytes());
        push_framed(&mut buf, body);
    }
    HashHex::from_bytes(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        let manifest = b"{}";
        let components: &[(&str, &[u8])] = &[("data_versions", b"[]")];
        let a = pack_hash(manifest, components);
        let b = pack_hash(manifest, components);
        assert_eq!(a, b);
    }

    #[test]
    fn renaming_a_component_changes_the_hash() {
        let manifest = b"{}";
        let body: &[u8] = b"[]";
        let with_one_name = pack_hash(manifest, &[("data_versions", body)]);
        let with_other_name = pack_hash(manifest, &[("blocks", body)]);
        assert_ne!(with_one_name, with_other_name);
    }

    #[test]
    fn changing_the_manifest_changes_the_hash() {
        let body: &[u8] = b"[]";
        let one = pack_hash(b"{\"a\":1}", &[("data_versions", body)]);
        let two = pack_hash(b"{\"a\":2}", &[("data_versions", body)]);
        assert_ne!(one, two);
    }

    #[test]
    fn a_byte_moved_from_the_manifest_into_a_name_changes_the_hash() {
        // The manifest ran straight into the first component name with
        // nothing between them, so the same bytes split one place to the
        // left or right hashed the same. `registry_pack_hash` is what a
        // lockfile records about the pack it was built from, so two
        // different packs answering to one digest is the whole failure.
        let body: &[u8] = b"BODY";
        let left = pack_hash(b"{\"x\":1}ab", &[("c", body)]);
        let right = pack_hash(b"{\"x\":1}a", &[("bc", body)]);
        assert_ne!(left, right);
    }

    #[test]
    fn a_byte_moved_across_a_component_boundary_changes_the_hash() {
        // The same gap exists between one component's body and the next
        // component's name, so a separator before the first name alone
        // would not close it: two components framed as `a\0X` + `b\0Y` are
        // indistinguishable from one component named `a` whose body happens
        // to be `X`, `b`, NUL, `Y`.
        let two = pack_hash(b"{}", &[("a", b"X"), ("b", b"Y")]);
        let one = pack_hash(b"{}", &[("a", b"Xb\0Y")]);
        assert_ne!(two, one);
    }
}
