//! sha256 hashing used by [`super::Lockfile`].
//!
//! Hex-encoded with a `sha256:` prefix so swapping in a different algorithm
//! later (BLAKE3, multibase) only touches this file. The two public
//! entrypoints map to the lockfile fields they feed: [`hash_source`] over
//! raw `.crn` bytes for `source_hash`, [`hash_resolved_ir`] over the lowered
//! block-array IR for `resolved_ir_hash`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::block_array::BlockArrayIr;

/// Hex-encoded sha256 with the spec §10.6 `sha256:` prefix.
///
/// Newtype rather than a bare `String` so a future algorithm switch is one
/// `impl From` away, and so a caller cannot accidentally hand an arbitrary
/// string where a hash is expected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HashHex(pub String);

impl HashHex {
    /// Canonical zero hash written into [`super::LockInputs`] for inputs
    /// that don't exist yet (registry pack, constraint catalog). Carrying a
    /// zero rather than `Option::None` keeps the lockfile shape stable: the
    /// fields will gain real values in 2026.12.0 without a schema change.
    pub const ZERO_STR: &'static str =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000";

    /// Hex-encode an arbitrary digest with the `sha256:` prefix.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut hex = String::with_capacity("sha256:".len() + digest.len() * 2);
        hex.push_str("sha256:");
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
/// JSON serialisation is the canonical input rather than a custom binary
/// encoder: `serde_json::to_writer` walks `BlockArrayIr` in
/// `IndexMap`-insertion order and the IR's own ordering is already stable
/// (palette by intern order, voxels by `(y, z, x)`), so the resulting byte
/// stream is deterministic without an extra normalisation pass. Field
/// additions to `BlockArrayIr` flow into the hash automatically.
///
/// # Panics
///
/// Panics only if `serde_json` cannot serialise [`BlockArrayIr`] — the IR
/// is a closed set of types that all derive `Serialize`, so any failure
/// here would be a programming error in this crate, not a runtime
/// condition a caller could recover from.
#[must_use]
pub fn hash_resolved_ir(ir: &BlockArrayIr) -> HashHex {
    let bytes = serde_json::to_vec(ir).expect("BlockArrayIr Serialize impl is total");
    HashHex::from_bytes(&bytes)
}
