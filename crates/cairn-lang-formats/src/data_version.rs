//! A `--target` version resolved to the integer its edition writes, Java's `DataVersion` or
//! Bedrock's block palette `version`, and to its key in the edition's version order.

use cairn_lang_core::Edition;

use crate::java_structure::OutputExt;
pub use crate::registry::data_versions::{BedrockTarget, JavaTarget, UnsupportedTarget};
use crate::registry::{RegistryPack, builtin_pack};

/// A `--target` value resolved against the built-in pack of its edition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTarget {
    /// A Java target.
    Java(JavaTarget),
    /// A Bedrock target.
    Bedrock(BedrockTarget),
}

impl ResolvedTarget {
    /// The edition whose table the target was resolved in.
    #[must_use]
    pub fn edition(&self) -> Edition {
        match self {
            Self::Java(_) => Edition::Java,
            Self::Bedrock(_) => Edition::Bedrock,
        }
    }

    /// The version the target names, `latest` resolved.
    #[must_use]
    pub fn mc_version(&self) -> &str {
        match self {
            Self::Java(target) => &target.mc_version,
            Self::Bedrock(target) => &target.mc_version,
        }
    }

    /// `DataVersion` on Java, the block palette `version` on Bedrock: the integer the artifact
    /// and its lockfile carry.
    #[must_use]
    pub fn version_int(&self) -> i32 {
        match self {
            Self::Java(target) => target.data_version,
            Self::Bedrock(target) => target.block_version,
        }
    }

    /// The built-in pack the target was resolved against.
    #[must_use]
    pub fn registry_pack(&self) -> &'static RegistryPack {
        builtin_pack(self.edition())
    }

    /// The extension of one array's structure file in the edition's own format.
    #[must_use]
    pub fn output_ext(&self) -> OutputExt {
        match self {
            Self::Java(_) => OutputExt::Nbt,
            Self::Bedrock(_) => OutputExt::Mcstructure,
        }
    }
}

/// Resolves a `--target` value against the built-in pack of `edition`.
///
/// # Errors
///
/// [`UnsupportedTarget`] when `requested` is neither `"latest"` nor a targetable version.
pub fn resolve_target(
    edition: Edition,
    requested: &str,
) -> Result<ResolvedTarget, UnsupportedTarget> {
    match edition {
        Edition::Java => resolve_java_target(requested).map(ResolvedTarget::Java),
        Edition::Bedrock => resolve_bedrock_target(requested).map(ResolvedTarget::Bedrock),
    }
}

/// Resolves a `--target` value against the built-in Java pack.
///
/// # Errors
///
/// [`UnsupportedTarget`] when `requested` is neither `"latest"` nor a targetable version.
pub fn resolve_java_target(requested: &str) -> Result<JavaTarget, UnsupportedTarget> {
    builtin_pack(Edition::Java).resolve_java_target(requested)
}

/// Resolves a `--target` value against the built-in Bedrock pack.
///
/// # Errors
///
/// [`UnsupportedTarget`] when `requested` is neither `"latest"` nor a targetable version.
pub fn resolve_bedrock_target(requested: &str) -> Result<BedrockTarget, UnsupportedTarget> {
    builtin_pack(Edition::Bedrock).resolve_bedrock_target(requested)
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
