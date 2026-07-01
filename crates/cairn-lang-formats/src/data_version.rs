//! `--target <mc_version>` → Minecraft `DataVersion` resolution.
//!
//! Java structure NBT carries a `DataVersion` integer that the loading
//! client uses to know whether and how to run DFU upgrades. The mapping
//! between human-facing `1.21.4` and the integer `4189` is sourced from
//! the built-in registry pack at `registry-data/java/data_versions.json`
//! (see [`crate::registry`]). The hardcoded table that used to live in
//! this module was removed when the registry pack ingest landed.

use std::fmt;

use crate::registry::builtin_java;

/// Resolved Minecraft target. `mc_version` is owned (cloned from the
/// registry pack) so a future `--registry-pack <dir>` flow can drop
/// runtime-loaded strings in here. `Clone` instead of `Copy` because the
/// owned `mc_version` cannot be byte-copied; callers that previously
/// relied on `Copy` now pass `&JavaTarget` instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaTarget {
    /// Human-facing version string, e.g. `"1.21.4"`.
    pub mc_version: String,
    /// `DataVersion` integer written into the structure NBT root.
    pub data_version: i32,
}

/// `--target` value did not match any version in the registry pack.
#[derive(Debug)]
pub struct UnsupportedTarget {
    /// Verbatim value passed via `--target`.
    pub requested: String,
    /// Closest supported version when one sits within the suggestion pass's
    /// length-scaled Damerau-Levenshtein cap (see `cairn-lang-core::suggest`),
    /// or `None` when the input is too far from any candidate. Holds the
    /// bare candidate name; the surrounding `did you mean ...?` phrasing is
    /// the [`fmt::Display`] impl's responsibility.
    pub suggestion: Option<String>,
    /// Pre-formatted list of supported versions, ready to print.
    pub supported: String,
}

impl fmt::Display for UnsupportedTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported java target `{}`. ", self.requested)?;
        if let Some(s) = &self.suggestion {
            write!(f, "did you mean `{s}`? ")?;
        }
        write!(f, "supported targets: {}", self.supported)
    }
}

impl std::error::Error for UnsupportedTarget {}

/// Resolve a `--target` string against the built-in registry pack. The
/// literal `"latest"` aliases the row named by `data_versions.latest`,
/// matching the working assumption that "no explicit `--target`" means
/// "the newest supported".
///
/// # Errors
///
/// Returns [`UnsupportedTarget`] when the requested string is neither an
/// exact `mc_version` match nor the `"latest"` alias.
pub fn resolve_java_target(requested: &str) -> Result<JavaTarget, UnsupportedTarget> {
    builtin_java().resolve_java_target(requested)
}

/// Human-readable list of supported `mc_version` strings (built-in pack),
/// joined for error messages. Public so the CLI can print the same list
/// in `--help`-style contexts without re-deriving it.
#[must_use]
pub fn supported_list() -> String {
    builtin_java().supported_list()
}

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
        // `latest` must resolve to a real row that itself resolves to the
        // same target. Comparing against a recomputed lookup avoids
        // hardcoding the answer here so a registry-pack update doesn't
        // need a parallel edit in this test.
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
        // `1.21.5` is one substitution away from `1.21.4`; the suggestion
        // field must point at the real version so a user re-running
        // `cairn compile --target 1.21.5` sees the typo immediately.
        let err = resolve_java_target("1.21.5").expect_err("unknown");
        assert_eq!(err.suggestion.as_deref(), Some("1.21.4"));
        // The Display impl wraps the bare suggestion into the wider error.
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
