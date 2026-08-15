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

impl PackEdition {
    /// Lowercase label matching the manifest's on-disk spelling and the
    /// CLI's `--edition` vocabulary. Used in error messages so the text a
    /// user reads matches the flag value they typed.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            PackEdition::Java => "java",
            PackEdition::Bedrock => "bedrock",
        }
    }
}

/// Component file references inside a registry pack.
///
/// `data_versions` is the only required component in the initial cut.
/// Later additions slot in `Option`-typed entries for blocks, items,
/// tags, and the semantic-sensitivity catalog so older packs stay
/// loadable.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PackFiles {
    /// Relative filename of the `DataVersionTable` JSON.
    pub data_versions: String,
    /// Relative filename of the abstract `materials` catalog JSON. Optional
    /// for backwards compatibility: a pack written before PR2 lacks this
    /// component, and the loader fills [`crate::registry::MaterialsIndex::empty`]
    /// in its place.
    #[serde(default)]
    pub materials: Option<String>,
    /// Relative filename of the per-version `blocks` id table JSON.
    /// Optional on the same terms as `materials`: a pack without it loads,
    /// and the loader fills [`crate::registry::BlocksIndex::empty`] in its
    /// place. A compile against such a pack cannot decide whether an id
    /// exists, so it does not try — every id passes, exactly as it did
    /// before this component existed. Nothing announces that; the pack is
    /// what has to grow the table.
    #[serde(default)]
    pub blocks: Option<String>,
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
        assert!(m.files.materials.is_none(), "materials defaults to None");
        assert!(m.files.blocks.is_none(), "blocks defaults to None");
    }

    #[test]
    fn manifest_with_blocks_component_roundtrips() {
        let src = r#"{
            "schema_version": 1,
            "edition": "bedrock",
            "name": "cairn-builtin-bedrock",
            "description": "test",
            "files": {
                "data_versions": "data_versions.json",
                "blocks": "blocks.json"
            }
        }"#;
        let m: PackManifest = serde_json::from_str(src).expect("deserialise manifest");
        assert_eq!(m.files.blocks.as_deref(), Some("blocks.json"));
    }

    #[test]
    fn manifest_with_materials_component_roundtrips() {
        let src = r#"{
            "schema_version": 1,
            "edition": "java",
            "name": "cairn-builtin-java",
            "description": "test",
            "files": {
                "data_versions": "data_versions.json",
                "materials": "materials.json"
            }
        }"#;
        let m: PackManifest = serde_json::from_str(src).expect("deserialise manifest");
        assert_eq!(m.files.materials.as_deref(), Some("materials.json"));
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
