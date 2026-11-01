//! The `data_versions.json` table, and a `--target` resolved against it. The table lists every
//! release, targetable or not, because ordering an `@requires` floor needs them all
//! (`spec/versioning-editions` "Fail-loud and minimum-version inference").

use std::fmt;

use cairn_lang_core::Edition;
use cairn_lang_core::suggest::did_you_mean;
use serde::Deserialize;

/// The body of `data_versions.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DataVersionTable {
    /// The file's schema version.
    pub schema_version: u32,
    /// The `mc_version` `"latest"` resolves to, which must name a targetable row.
    pub latest: String,
    /// Where the rows came from, for a human reader; the compiler ignores it.
    #[serde(default)]
    pub source: Option<String>,
    /// The versions, oldest first, with `data_version` and label both strictly ascending.
    pub versions: Vec<DataVersionEntry>,
}

/// One row of the `DataVersionTable`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DataVersionEntry {
    /// The version, such as `"1.21.4"`.
    pub mc_version: String,
    /// Java's `DataVersion` or Bedrock's palette `version`. A row that is not targetable never
    /// reaches a file, so its Bedrock integer only orders it.
    pub data_version: i32,
    /// The release date, `YYYY-MM-DD`; the compiler ignores it.
    #[serde(default)]
    pub released: Option<String>,
    /// Whether `--target` may name this version; absent means `true`, as in `schema_version` 1.
    #[serde(default = "targetable_by_default")]
    pub targetable: bool,
}

fn targetable_by_default() -> bool {
    true
}

/// A resolved Java target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaTarget {
    /// The version, such as `"1.21.4"`.
    pub mc_version: String,
    /// The `DataVersion` written into the structure root.
    pub data_version: i32,
}

/// A resolved Bedrock target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedrockTarget {
    /// The version, such as `"1.21.60"`.
    pub mc_version: String,
    /// The palette entries' `version`, packed as `major << 24 | minor << 16 | patch << 8 | rev`.
    pub block_version: i32,
}

/// A `--target` that is not `latest` and, compared as a version, no targetable version in the pack.
#[derive(Debug)]
pub struct UnsupportedTarget {
    /// The edition whose version table was consulted.
    pub edition: Edition,
    /// The `--target` value, verbatim.
    pub requested: String,
    /// The closest supported version, bare; [`fmt::Display`] adds the `did you mean` phrasing.
    pub suggestion: Option<String>,
    /// The supported versions, formatted for printing.
    pub supported: String,
}

impl fmt::Display for UnsupportedTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported {} target `{}`. ",
            self.edition, self.requested
        )?;
        if let Some(s) = &self.suggestion {
            write!(f, "{} ", did_you_mean(s))?;
        }
        write!(f, "supported targets: {}", self.supported)
    }
}

impl std::error::Error for UnsupportedTarget {}

#[cfg(test)]
mod tests {
    use super::*;

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
