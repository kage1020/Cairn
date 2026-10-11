//! sha256 hashing used by [`super::Lockfile`].
//!
//! Hex-encoded with a `sha256:` prefix so a different algorithm (BLAKE3, multibase) only touches
//! this file. [`hash_source`] hashes the raw `.crn` bytes for `source_hash`, and
//! [`hash_resolved_ir`] the lowered block-array IR for `resolved_ir_hash`.

use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::block_array::BlockArrayIr;

const PREFIX: &str = "sha256:";
const HEX_LEN: usize = 64;

/// Hex-encoded sha256 with the `sha256:` prefix `spec/versioning-editions` "Provenance and lock"
/// gives it.
///
/// A newtype rather than a bare `String`, so another algorithm is one `impl From` away and an
/// arbitrary string cannot be handed over where a hash is expected. The field is private:
/// [`HashHex::from_bytes`] and [`HashHex::zero`] guarantee the shape, and [`HashHex::parse`]
/// validates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashHex(String);

/// Invalid `sha256:<hex64>` shape encountered while parsing or deserialising.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HashParseError {
    /// Missing or wrong prefix.
    #[error("hash is missing the `sha256:` prefix")]
    MissingPrefix,
    /// Hex body had the wrong length.
    #[error("hash hex body must be {HEX_LEN} chars, got {got}")]
    WrongLength {
        /// Length actually seen.
        got: usize,
    },
    /// Hex body contained a non-hex character.
    #[error("hash hex body has non-hex character at index {index}")]
    NonHexChar {
        /// Byte index inside the hex body (not counting the prefix).
        index: usize,
    },
}

impl HashHex {
    /// Canonical zero hash written into [`super::LockInputs`] for inputs not yet hashed (registry
    /// pack, constraint catalog). A zero rather than `None` keeps the lockfile shape stable when
    /// they gain real values.
    pub const ZERO_STR: &'static str =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000";

    /// Hex-encode an arbitrary digest with the `sha256:` prefix.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut hex = String::with_capacity(PREFIX.len() + digest.len() * 2);
        hex.push_str(PREFIX);
        for byte in digest {
            use std::fmt::Write as _;
            write!(&mut hex, "{byte:02x}").expect("writing to a String never fails");
        }
        Self(hex)
    }

    /// `sha256:000...000`, for inputs not yet populated.
    #[must_use]
    pub fn zero() -> Self {
        Self(Self::ZERO_STR.to_owned())
    }

    /// Parse a `sha256:<hex64>` literal, validating both the prefix and the 64-character hex body.
    /// [`Deserialize`] goes through it, as does any caller holding a literal from outside (a
    /// lockfile read by hand, a CLI flag).
    ///
    /// # Errors
    ///
    /// Returns [`HashParseError`] when the prefix or hex body is malformed.
    pub fn parse(s: &str) -> Result<Self, HashParseError> {
        let body = s
            .strip_prefix(PREFIX)
            .ok_or(HashParseError::MissingPrefix)?;
        if body.len() != HEX_LEN {
            return Err(HashParseError::WrongLength { got: body.len() });
        }
        if let Some(index) = body.bytes().position(|b| !b.is_ascii_hexdigit()) {
            return Err(HashParseError::NonHexChar { index });
        }
        Ok(Self(s.to_owned()))
    }

    /// Borrow the underlying `sha256:<hex64>` literal.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HashHex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for HashHex {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for HashHex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(D::Error::custom)
    }
}

/// Errors raised while hashing inputs into a [`HashHex`].
#[derive(Debug, Error)]
pub enum HashError {
    /// Serialising the block-array IR failed, as JSON does on a `NaN` or infinite float; this makes
    /// it a build error rather than a panic.
    #[error("hashing resolved IR: {0}")]
    SerialiseIr(#[from] serde_json::Error),
}

/// Hash the raw `.crn` source bytes the parser saw.
///
/// UTF-8 only is fine here because the parser already rejects non-UTF-8
/// input; the lockfile records the exact byte sequence that was compiled.
#[must_use]
pub fn hash_source(src: &str) -> HashHex {
    HashHex::from_bytes(src.as_bytes())
}

/// Hash the lowered block-array IR.
///
/// `serde_json::to_vec` is the canonical input rather than a custom binary encoder: it walks
/// `BlockArrayIr` in `IndexMap` insertion order and the IR's own order is stable (palette by intern
/// order, voxels by `(y, z, x)`), so the bytes are deterministic without a normalisation pass, and
/// a field added to `BlockArrayIr` reaches the hash on its own.
///
/// # Errors
///
/// Returns [`HashError::SerialiseIr`] if `serde_json` cannot encode the IR. The IR is integer-only,
/// so this cannot fire; the `Result` lets a float that turns out `NaN` fail the build rather than
/// panic inside the CLI.
pub fn hash_resolved_ir(ir: &BlockArrayIr) -> Result<HashHex, HashError> {
    let bytes = serde_json::to_vec(ir)?;
    Ok(HashHex::from_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::HashHex;

    #[test]
    fn zero_is_the_prefix_and_sixty_four_hex_zeros() {
        assert_eq!(HashHex::ZERO_STR, format!("sha256:{}", "0".repeat(64)));
        assert_eq!(
            HashHex::parse(HashHex::ZERO_STR).expect("the zero hash parses"),
            HashHex::zero(),
        );
    }
}
