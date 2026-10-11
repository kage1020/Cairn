//! The `pack.json` manifest of a registry pack.

use cairn_lang_core::Edition;
use serde::Deserialize;

/// The body of `pack.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PackManifest {
    /// The manifest's schema version.
    pub schema_version: u32,
    /// The edition the pack targets.
    pub edition: Edition,
    /// The pack's name, shown in error messages.
    pub name: String,
    /// A description the compiler does not read.
    pub description: String,
    /// The component files.
    pub files: PackFiles,
}

/// The component files of a pack; all but `data_versions` are optional, so older packs still load.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PackFiles {
    /// The `data_versions` file, relative to the pack.
    pub data_versions: String,
    /// The `materials` file.
    #[serde(default)]
    pub materials: Option<String>,
    /// The `blocks` file; without it, every id passes as known.
    #[serde(default)]
    pub blocks: Option<String>,
    /// The `aliases` file; without it, an `E_UNKNOWN_ID` offers only the typo search.
    #[serde(default)]
    pub aliases: Option<String>,
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
        assert_eq!(m.edition, Edition::Java);
        assert_eq!(m.name, "cairn-builtin-java");
        assert_eq!(m.files.data_versions, "data_versions.json");
        assert!(m.files.materials.is_none(), "materials defaults to None");
        assert!(m.files.blocks.is_none(), "blocks defaults to None");
        assert!(m.files.aliases.is_none(), "aliases defaults to None");
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
    fn manifest_with_aliases_component_roundtrips() {
        let src = r#"{
            "schema_version": 1,
            "edition": "bedrock",
            "name": "cairn-builtin-bedrock",
            "description": "test",
            "files": {
                "data_versions": "data_versions.json",
                "blocks": "blocks.json",
                "aliases": "aliases.json"
            }
        }"#;
        let m: PackManifest = serde_json::from_str(src).expect("deserialise manifest");
        assert_eq!(m.files.aliases.as_deref(), Some("aliases.json"));
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
