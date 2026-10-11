//! A `--target` version resolved to the integer its edition writes: Java's `DataVersion`, or
//! Bedrock's block palette `version`.

use std::fmt;

use cairn_lang_core::Edition;
use cairn_lang_core::suggest::did_you_mean;

pub use crate::registry::{resolve_bedrock_target, resolve_java_target};

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
    fn resolves_known_version() {
        let t = resolve_java_target("1.21.4").expect("known version");
        assert_eq!(t.mc_version, "1.21.4");
        assert_eq!(t.data_version, 4189);
    }

    #[test]
    fn resolves_latest_alias() {
        let latest = resolve_java_target("latest").expect("latest");
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
