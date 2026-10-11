//! `--target <mc_version>` → per-edition version-integer resolution.
//!
//! Java structure NBT carries a `DataVersion` integer that the loading
//! client uses to know whether and how to run DFU upgrades. The mapping
//! between human-facing `1.21.4` and the integer `4189` is sourced from
//! the built-in registry pack at `registry-data/java/data_versions.json`,
//! so the lookup lives with the pack in [`crate::registry`] and is
//! re-exported here beside the types it returns.
//!
//! Bedrock `.mcstructure` palettes carry an analogous `version` integer
//! per block: the block-state version the release writes, packed as
//! `(major << 24) | (minor << 16) | (patch << 8) | revision`;
//! its table lives at `registry-data/bedrock/data_versions.json` and
//! resolves through the same machinery into a [`BedrockTarget`].

use std::fmt;

use cairn_lang_core::Edition;
use cairn_lang_core::suggest::did_you_mean;

pub use crate::registry::{resolve_bedrock_target, resolve_java_target};

/// Resolved Minecraft Java target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaTarget {
    /// Human-facing version string, e.g. `"1.21.4"`.
    pub mc_version: String,
    /// `DataVersion` integer written into the structure NBT root.
    pub data_version: i32,
}

/// Resolved Bedrock target. The `block_version` integer is what every
/// `.mcstructure` block-palette entry carries under `version` so the game
/// knows which schema the block states were authored against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedrockTarget {
    /// Human-facing version string, e.g. `"1.21.60"`.
    pub mc_version: String,
    /// Block palette `version` integer written into every `.mcstructure`
    /// palette entry.
    pub block_version: i32,
}

/// `--target` value equal to no targetable version in the registry pack,
/// and not the `latest` alias.
///
/// Equal as versions rather than as strings: the comparison is the one
/// [`VersionOrder::key_of`](cairn_lang_core::resolve::VersionOrder::key_of)
/// uses. The rows the pack only orders against, having no block data, are
/// never candidates.
#[derive(Debug)]
pub struct UnsupportedTarget {
    /// The edition whose version table was consulted.
    pub edition: Edition,
    /// Verbatim value passed via `--target`.
    pub requested: String,
    /// Closest supported version within the suggestion pass's
    /// length-scaled Damerau-Levenshtein cap (`cairn-lang-core::suggest`),
    /// or `None` when the input is too far from any candidate. The bare
    /// name; [`fmt::Display`] adds the `did you mean ...?` phrasing.
    pub suggestion: Option<String>,
    /// Pre-formatted list of supported versions, ready to print.
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
    fn resolves_known_version() {
        let t = resolve_java_target("1.21.4").expect("known version");
        assert_eq!(t.mc_version, "1.21.4");
        assert_eq!(t.data_version, 4189);
    }

    #[test]
    fn resolves_latest_alias() {
        let latest = resolve_java_target("latest").expect("latest");
        // Compared with a second lookup rather than a literal, so a
        // registry-pack update needs no edit here.
        let echoed = resolve_java_target(&latest.mc_version).expect("echo");
        assert_eq!(latest, echoed);
    }

    #[test]
    fn rejects_unknown_version() {
        let err = resolve_java_target("0.0.0").expect_err("unknown");
        assert_eq!(err.requested, "0.0.0");
        assert!(err.supported.contains("1.20.4"));
        assert!(err.supported.contains("latest"));
    }

    #[test]
    fn unknown_version_near_known_one_suggests_it() {
        // `1.21.5` is one substitution away from `1.21.4`.
        let err = resolve_java_target("1.21.5").expect_err("unknown");
        assert_eq!(err.suggestion.as_deref(), Some("1.21.4"));
        let rendered = err.to_string();
        assert!(
            rendered.contains("did you mean `1.21.4`?"),
            "Display should embed the suggestion, got: {rendered}",
        );
    }

    #[test]
    fn distant_unknown_version_skips_suggestion() {
        let err = resolve_java_target("0.0.0").expect_err("unknown");
        assert!(
            err.suggestion.is_none(),
            "no suggestion expected for a distant input, got: {:?}",
            err.suggestion,
        );
    }
}
