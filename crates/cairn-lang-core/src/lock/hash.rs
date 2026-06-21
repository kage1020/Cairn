//! sha256 hashing used by [`super::Lockfile`].
//!
//! Hex-encoded with a `sha256:` prefix so swapping in a different algorithm
//! later (BLAKE3, multibase) only touches this file. The two public
//! entrypoints map to the lockfile fields they feed: [`hash_source`] over
//! raw `.crn` bytes for `source_hash`, [`hash_resolved_ir`] over the lowered
//! block-array IR for `resolved_ir_hash`.

use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::block_array::BlockArrayIr;

const PREFIX: &str = "sha256:";
const HEX_LEN: usize = 64;

/// Hex-encoded sha256 with the spec §10.6 `sha256:` prefix.
///
/// Newtype rather than a bare `String` so a future algorithm switch is one
/// `impl From` away, and so a caller cannot accidentally hand an arbitrary
/// string where a hash is expected. The field stays private; construction
/// goes through [`HashHex::from_bytes`] / [`HashHex::zero`] (which
/// guarantee the shape) or through [`HashHex::parse`] (which validates).
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
    /// Canonical zero hash written into [`super::LockInputs`] for inputs
    /// that don't exist yet (registry pack, constraint catalog). Carrying a
    /// zero rather than `Option::None` keeps the lockfile shape stable: the
    /// fields will gain real values once those inputs land.
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

    /// `sha256:000...000`. Used for not-yet-populated inputs.
    #[must_use]
    pub fn zero() -> Self {
        Self(Self::ZERO_STR.to_owned())
    }

    /// Parse a `sha256:<hex64>` literal, validating both the prefix and
    /// the 64-character hex body. Used by [`Deserialize`] and by any
    /// caller that has the literal from outside (a lockfile read manually,
    /// a CLI flag, etc.).
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
    /// Serialisation of the block-array IR failed. JSON serialisation of
    /// the IR can fail in practice when a float field carries `NaN` or
    /// `±Infinity` (entity coordinates etc.), so this surface is here to
    /// turn that into a build error rather than a process panic.
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
/// JSON serialisation via `serde_json::to_vec` is the canonical input
/// rather than a custom binary encoder: it walks `BlockArrayIr` in
/// `IndexMap`-insertion order and the IR's own ordering is already stable
/// (palette by intern order, voxels by `(y, z, x)`), so the resulting byte
/// stream is deterministic without an extra normalisation pass. Field
/// additions to `BlockArrayIr` flow into the hash automatically.
///
/// # Errors
///
/// Returns [`HashError::SerialiseIr`] if `serde_json` cannot encode the
/// IR. In M2 this cannot fire — the IR is integer-only — but the result
/// surface is here so a later entity coordinate that turns out to be
/// `NaN` fails the build cleanly instead of panicking inside the CLI.
pub fn hash_resolved_ir(ir: &BlockArrayIr) -> Result<HashHex, HashError> {
    let bytes = serde_json::to_vec(ir)?;
    Ok(HashHex::from_bytes(&bytes))
}
