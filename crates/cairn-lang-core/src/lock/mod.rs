//! `build.cairn.lock` reader/writer.
//!
//! The lockfile pins three pieces of state at the moment of a successful
//! compile: the source bytes, the target (edition + Minecraft version +
//! `DataVersion`), and the resolved IR. Re-running `cairn compile` against
//! a different target prints the divergence by comparing fields.
//!
//! Note that the canonical filename for a single-source build is
//! `build.cairn.lock` (per `spec/versioning-editions.md` §10.6), but the
//! CLI defaults to `<source>.lock` so multi-source workspaces stay
//! unambiguous; the lockfile schema does not care which path it lives at.

mod hash;
mod schema;

use std::fs;
use std::path::Path;

pub use hash::{HashError, HashHex, HashParseError, hash_resolved_ir, hash_source};
pub use schema::{LockEdition, LockInputs, LockTarget, Lockfile, MemberSensitivity};

use thiserror::Error;

/// Errors raised while reading or writing a lockfile.
#[derive(Debug, Error)]
pub enum LockError {
    /// Underlying filesystem I/O failure.
    #[error("lockfile I/O: {0}")]
    Io(#[from] std::io::Error),
    /// YAML encoder/decoder rejected the contents.
    #[error("lockfile YAML: {0}")]
    Yaml(#[from] serde_yml::Error),
}

impl Lockfile {
    /// Write the lockfile to `path` as YAML, overwriting any existing file.
    ///
    /// # Errors
    ///
    /// Propagates I/O failure from creating or writing `path`, or YAML
    /// encoder failure from the schema.
    pub fn write_to_path(&self, path: &Path) -> Result<(), LockError> {
        let body = serde_yml::to_string(self)?;
        fs::write(path, body)?;
        Ok(())
    }

    /// Read a lockfile back from `path`.
    ///
    /// # Errors
    ///
    /// Propagates I/O failure from reading `path` and YAML decode failure
    /// when the file's shape does not match [`Lockfile`].
    pub fn read_from_path(path: &Path) -> Result<Self, LockError> {
        let body = fs::read_to_string(path)?;
        let lf: Lockfile = serde_yml::from_str(&body)?;
        Ok(lf)
    }
}
