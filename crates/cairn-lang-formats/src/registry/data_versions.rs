//! `data_versions.json` schema — the `(mc_version, version integer)` table.
//!
//! Replaces the hardcoded `JAVA_TARGETS` array that lived in
//! [`crate::data_version`] before the registry pack ingest landed. The
//! file is loaded once per process and resolution against it happens via
//! [`crate::data_version::resolve_java_target`] /
//! [`crate::data_version::resolve_bedrock_target`]. Both editions share
//! the schema; what the integer means is a per-edition contract documented
//! on [`DataVersionEntry::data_version`].

use serde::Deserialize;

/// Body of `data_versions.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DataVersionTable {
    /// Schema version of this file. Bumped only when an incompatible
    /// shape change lands.
    pub schema_version: u32,
    /// `mc_version` string the `"latest"` alias resolves to. Held
    /// explicitly rather than implicit-by-last-entry so a pack author can
    /// pin `"latest"` to something other than the newest known row when
    /// the latest patch is unstable.
    pub latest: String,
    /// Known Minecraft versions. Stored in ascending release order by
    /// convention; the resolver does not depend on order so the order of
    /// rows in the JSON file is informational.
    pub versions: Vec<DataVersionEntry>,
}

/// One row of the `DataVersionTable`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DataVersionEntry {
    /// Human-facing Minecraft version, e.g. `"1.21.4"`.
    pub mc_version: String,
    /// Edition-specific version integer. For Java packs this is the
    /// `DataVersion` the structure NBT root carries; for Bedrock packs it
    /// is the block-palette `version` integer every `.mcstructure`
    /// palette entry carries (`(major << 24) | (minor << 16) |
    /// (patch << 8) | revision` of the client build).
    pub data_version: i32,
    /// Release date in `YYYY-MM-DD` form. Informational; not consumed by
    /// the compiler. `Option` so older packs without the field still load.
    #[serde(default)]
    pub released: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_version_table_roundtrip() {
        let src = r#"{
            "schema_version": 1,
            "latest": "1.21.4",
            "versions": [
                { "mc_version": "1.20.4", "data_version": 3700, "released": "2023-12-07" },
                { "mc_version": "1.21",   "data_version": 3953 },
                { "mc_version": "1.21.4", "data_version": 4189, "released": "2024-12-03" }
            ]
        }"#;
        let t: DataVersionTable = serde_json::from_str(src).expect("deserialise table");
        assert_eq!(t.schema_version, 1);
        assert_eq!(t.latest, "1.21.4");
        assert_eq!(t.versions.len(), 3);
        assert_eq!(t.versions[1].mc_version, "1.21");
        assert_eq!(t.versions[1].data_version, 3953);
        assert_eq!(t.versions[1].released, None);
    }
}
