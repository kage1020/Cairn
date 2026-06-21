//! Top-level `pack.json` schema describing a registry pack.

use serde::Deserialize;

/// `pack.json` body.
///
/// Field order matters for the `manifest_roundtrip` test only; on disk the
/// JSON keys can appear in any order because `serde_json` reads by name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PackManifest {
    /// Schema version of the manifest itself. Bumped only when an
    /// incompatible shape change lands.
    pub schema_version: u32,
    /// Edition the pack targets.
    pub edition: PackEdition,
    /// Stable identifier for the pack. Surfaced in error messages so a
    /// `--registry-pack` mismatch points the user at the actual pack.
    pub name: String,
    /// Free-form description. Informational; not consumed by the compiler.
    pub description: String,
    /// Component file references.
    pub files: PackFiles,
}

/// Edition a pack targets. Closed enum so a typo (`"jav"`, `"BEDROCK"`)
/// cannot ride along as a valid pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PackEdition {
    /// Java Edition.
    Java,
    /// Bedrock Edition.
    Bedrock,
}

/// Component file references inside a registry pack.
///
/// `data_versions` is the only required component in the 2026.12.0 PR1
/// cut. Subsequent PRs add `Option`-typed entries for blocks, items, tags,
/// and the semantic-sensitivity catalog so older packs stay loadable.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PackFiles {
    /// Relative filename of the `DataVersionTable` JSON.
    pub data_versions: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip() {
        let src = r#"{
            "schema_version": 1,
            "edition": "java",
            "name": "cairn-builtin-java",
            "description": "test",
            "files": { "data_versions": "data_versions.json" }
        }"#;
        let m: PackManifest = serde_json::from_str(src).expect("deserialise manifest");
        assert_eq!(m.schema_version, 1);
        assert_eq!(m.edition, PackEdition::Java);
        assert_eq!(m.name, "cairn-builtin-java");
        assert_eq!(m.files.data_versions, "data_versions.json");
    }

    #[test]
    fn manifest_rejects_unknown_edition() {
        let src = r#"{
            "schema_version": 1,
            "edition": "windows",
            "name": "x",
            "description": "x",
            "files": { "data_versions": "x" }
        }"#;
        let err = serde_json::from_str::<PackManifest>(src).expect_err("unknown edition");
        assert!(err.to_string().contains("windows"));
    }
}
