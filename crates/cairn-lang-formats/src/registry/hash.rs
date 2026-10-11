//! The registry pack hash the lockfile records as `inputs.registry_pack_hash`.

use cairn_lang_core::lock::HashHex;

const LENGTH_PREFIX: usize = size_of::<u64>();

/// Appends `bytes` after its length, so moving bytes from one field to the next changes the hash.
fn push_framed(buf: &mut Vec<u8>, bytes: &[u8]) {
    let len = u64::try_from(bytes.len()).expect("a length fits in u64 on every supported target");
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(bytes);
}

/// The sha256 of the manifest and each named component in declared order, every field framed.
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
    fn the_framing_produces_a_known_digest() {
        // sha256 of `u64le(2) "{}" u64le(1) "a" u64le(1) "X"`, worked out by hand, not copied
        // from a run.
        assert_eq!(
            pack_hash(b"{}", &[("a", b"X")]).as_str(),
            "sha256:5587de979011a708a5592e56ccdb3073472266e0c4df7bd8c25171f8018692de",
        );
    }

    #[test]
    fn a_byte_moved_from_the_manifest_into_a_name_changes_the_hash() {
        let body: &[u8] = b"BODY";
        let left = pack_hash(b"{\"x\":1}ab", &[("c", body)]);
        let right = pack_hash(b"{\"x\":1}a", &[("bc", body)]);
        assert_ne!(left, right);
    }

    #[test]
    fn a_byte_moved_across_a_component_boundary_changes_the_hash() {
        let two = pack_hash(b"{}", &[("a", b"X"), ("b", b"Y")]);
        let one = pack_hash(b"{}", &[("a", b"Xb\0Y")]);
        assert_ne!(two, one);
    }
}
