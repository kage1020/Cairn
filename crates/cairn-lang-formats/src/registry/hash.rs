//! sha256 over the manifest bytes + each named component, in declared
//! order. The output is the value that lands in the lockfile under
//! `inputs.registry_pack_hash`.

use cairn_lang_core::lock::HashHex;

/// Hash the manifest body and every referenced component file in declared
/// order so two packs with the same bytes (and matching filenames) produce
/// the same digest. Each component contributes
/// `name | 0x00 | body` so a rename without a content change still moves
/// the digest — otherwise an attacker (or a typo) could swap the
/// `data_versions` filename for an unrelated component and keep the hash
/// stable.
#[must_use]
pub fn pack_hash(manifest_bytes: &[u8], components: &[(&str, &[u8])]) -> HashHex {
    let extra: usize = components
        .iter()
        .map(|(name, body)| name.len() + 1 + body.len())
        .sum();
    let mut buf: Vec<u8> = Vec::with_capacity(manifest_bytes.len() + extra);
    buf.extend_from_slice(manifest_bytes);
    for (name, body) in components {
        buf.extend_from_slice(name.as_bytes());
        buf.push(0);
        buf.extend_from_slice(body);
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
}
