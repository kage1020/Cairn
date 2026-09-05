//! `data_versions.json` schema — the `(mc_version, version integer)` table.
//!
//! The table has two readers with different needs, which is why a row
//! carries [`DataVersionEntry::targetable`]. `--target` may only name a
//! version the pack has block and material data for — three per edition
//! today. Ordering an `@requires` floor needs every *release*, because a
//! floor may name any of them and "inside the table's span, naming no row"
//! is only the same fact as "not a release of this edition" when the table
//! carries them all (`spec/versioning-editions.md` §10.4).
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
    /// the latest patch is unstable — and, since the newest row is now a
    /// release the pack has no block data for, it is never the newest row.
    /// Validated to name a [`DataVersionEntry::targetable`] row.
    pub latest: String,
    /// Where the rows came from, for a reader asking whether to trust
    /// them. Informational and not consumed by the compiler; `None` for a
    /// pack that does not say.
    ///
    /// Recorded because the two halves of this table have different
    /// provenance: the version integers of the targetable rows are what
    /// the backend stamps into a structure file and are verified against
    /// it, while the rest are an ordering key transcribed from a published
    /// list.
    #[serde(default)]
    pub source: Option<String>,
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
    /// Whether `--target` may name this version.
    ///
    /// `false` for a row that is here to be *ordered against* rather than
    /// built for: the pack knows when the release happened and what its
    /// version integer is, and has neither a block table nor material
    /// overrides for it. Defaulted `true` so a `schema_version: 1` pack,
    /// whose rows were all buildable targets, keeps meaning what it meant.
    #[serde(default = "targetable_by_default")]
    pub targetable: bool,
}

/// A row that does not say is a buildable target, which is what every row
/// of a `schema_version: 1` table was.
fn targetable_by_default() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `schema_version: 1` table has no `targetable` key and every row
    /// of it was a buildable target, so the default has to be `true` or
    /// such a pack silently loses every `--target` it used to accept.
    #[test]
    fn a_row_that_does_not_say_is_targetable() {
        let src = r#"{
            "schema_version": 1,
            "latest": "1.21.4",
            "versions": [{ "mc_version": "1.21.4", "data_version": 4189 }]
        }"#;
        let t: DataVersionTable = serde_json::from_str(src).expect("deserialise table");
        assert!(t.versions[0].targetable);
        assert_eq!(t.source, None);
    }

    /// And a row that says `false` is an ordering row: the pack knows
    /// where the release sits and cannot build for it.
    #[test]
    fn a_row_may_be_ordered_against_without_being_buildable() {
        let src = r#"{
            "schema_version": 2,
            "latest": "1.21.4",
            "source": "somewhere",
            "versions": [
                { "mc_version": "1.21.1", "data_version": 3955, "targetable": false },
                { "mc_version": "1.21.4", "data_version": 4189 }
            ]
        }"#;
        let t: DataVersionTable = serde_json::from_str(src).expect("deserialise table");
        assert!(!t.versions[0].targetable);
        assert!(t.versions[1].targetable);
        assert_eq!(t.source.as_deref(), Some("somewhere"));
    }

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
