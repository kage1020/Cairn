//! The Minecraft edition a build targets.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A Minecraft edition Cairn can target.
///
/// Java sorts first: it is the base and Bedrock the diff (`spec/versioning-editions` "Backend =
/// data tables").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    /// Java Edition, written as `.nbt`.
    Java,
    /// Bedrock Edition, written as `.mcstructure`.
    Bedrock,
}

impl Edition {
    /// The lowercase name, as `--edition` spells it.
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

/// A string that names no edition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "unknown edition `{input}`. Valid: java, bedrock. Fix: pass one of the supported edition names"
)]
pub struct UnknownEdition {
    /// The string as the caller passed it.
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
        assert_eq!(
            err.to_string(),
            "unknown edition `foo`. Valid: java, bedrock. Fix: pass one of the supported edition names",
        );
    }

    #[test]
    fn from_str_is_case_sensitive() {
        assert!("Java".parse::<Edition>().is_err());
        assert!("BEDROCK".parse::<Edition>().is_err());
    }

    #[test]
    fn ord_puts_java_before_bedrock() {
        assert!(Edition::Java < Edition::Bedrock);
    }

    #[test]
    fn serde_uses_the_lowercase_name() {
        for edition in [Edition::Java, Edition::Bedrock] {
            let json = serde_json::to_string(&edition).unwrap();
            assert_eq!(json, format!("\"{edition}\""));
            assert_eq!(serde_json::from_str::<Edition>(&json).unwrap(), edition);
        }
        assert!(serde_json::from_str::<Edition>("\"Java\"").is_err());
    }
}
