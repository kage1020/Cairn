//! Cross-cutting edition marker shared by resolver and CLI.
//!
//! The compilation target's edition (Java / Bedrock) surfaces at three layers
//! that must agree: the CLI's `--edition` flag, the resolver's per-edition
//! theme-variant selection (spec versioning-editions §10.7), and the
//! downstream backend that lowers to `.nbt` / `.mcstructure`. Keeping the
//! marker enum here — in `cairn-lang-core`, the layer both callers depend on
//! — avoids either a duplicated definition in `cairn-lang-formats` and
//! `cairn-lang-cli` or a `formats → cli` dependency reversal.
//!
//! A third edition (Education) can slot in by adding one variant; the
//! `Display` / `FromStr` pair keeps the CLI-facing string vocabulary
//! (`"java"`, `"bedrock"`) canonical in one place.

use std::fmt;
use std::str::FromStr;

/// A Minecraft edition Cairn can target.
///
/// The variants are ordered `Java`, `Bedrock` so the derived `Ord` matches
/// the spec's "Java as the base, Bedrock as overriding diffs" framing
/// (versioning-editions §10.3) — Java sorts first when the ordering is
/// otherwise arbitrary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Edition {
    /// Java Edition. Emits gzip-compressed vanilla `.nbt` structures.
    Java,
    /// Bedrock Edition. Emits uncompressed little-endian `.mcstructure`.
    Bedrock,
}

impl Edition {
    /// Canonical lowercase name of the edition. Used by the CLI, the
    /// registry-pack manifest, and any JSON that names an edition.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Java => "java",
            Self::Bedrock => "bedrock",
        }
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Failure while parsing an edition string. Carries the offending value verbatim
/// so the CLI can surface it inside the "what is wrong / what is valid /
/// suggested fix" self-correction triple (spec versioning-editions §10.4).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "unknown edition `{input}`. Valid: java, bedrock. Fix: pass one of the supported edition names"
)]
pub struct UnknownEdition {
    /// Verbatim input string the caller passed.
    pub input: String,
}

impl FromStr for Edition {
    type Err = UnknownEdition;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "java" => Ok(Self::Java),
            "bedrock" => Ok(Self::Bedrock),
            other => Err(UnknownEdition {
                input: other.to_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_as_str_agree() {
        assert_eq!(Edition::Java.as_str(), "java");
        assert_eq!(Edition::Bedrock.as_str(), "bedrock");
        assert_eq!(Edition::Java.to_string(), "java");
        assert_eq!(Edition::Bedrock.to_string(), "bedrock");
    }

    #[test]
    fn from_str_accepts_canonical_names() {
        assert_eq!("java".parse::<Edition>().unwrap(), Edition::Java);
        assert_eq!("bedrock".parse::<Edition>().unwrap(), Edition::Bedrock);
    }

    #[test]
    fn from_str_rejects_unknown_carrying_input() {
        let err = "foo".parse::<Edition>().unwrap_err();
        assert_eq!(err.input, "foo");
        let msg = err.to_string();
        assert!(msg.contains("foo"), "wrong: {msg}");
        assert!(msg.contains("java, bedrock"), "valid: {msg}");
    }

    #[test]
    fn from_str_is_case_sensitive() {
        // Lowercase is the canonical form — the CLI's clap layer already
        // enforces it, so the parse is deliberately strict here to catch
        // any accidental UPPER/MixedCase leak from a future entry point.
        assert!("Java".parse::<Edition>().is_err());
        assert!("BEDROCK".parse::<Edition>().is_err());
    }

    #[test]
    fn ord_puts_java_before_bedrock() {
        assert!(Edition::Java < Edition::Bedrock);
    }
}
