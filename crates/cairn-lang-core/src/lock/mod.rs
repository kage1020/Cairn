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
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub use hash::{HashError, HashHex, HashParseError, hash_resolved_ir, hash_source};
pub use schema::{
    LockEdition, LockInputs, LockPlacement, LockTarget, LockWalkway, Lockfile, MemberSensitivity,
};

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
    /// Writes through a sibling temporary file and a rename, so a failure
    /// part-way cannot leave a truncated lockfile where a valid one was.
    /// `fs::write` truncates in place, which turns a full disk or a lost
    /// handle into a file that no longer parses — worse than the old
    /// contents and worse than no file at all.
    pub fn write_to_path(&self, path: &Path) -> Result<(), LockError> {
        let body = self.to_yaml()?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);

        let staged = (|| -> Result<(), std::io::Error> {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(body.as_bytes())?;
            file.flush()?;
            // The rename below publishes the name, not the bytes; without
            // this a crash can leave the new name pointing at a partial
            // file.
            file.sync_all()
        })();
        if let Err(err) = staged {
            let _ = fs::remove_file(&tmp);
            return Err(err.into());
        }
        if let Err(err) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(err.into());
        }
        Ok(())
    }

    /// Serialise to the YAML body [`Self::write_to_path`] would write.
    ///
    /// Separating the encoding from the I/O lets a caller that has to place
    /// several files together fail out of the encode without having touched
    /// the filesystem at all.
    ///
    /// # Errors
    ///
    /// Propagates YAML encode failure.
    pub fn to_yaml(&self) -> Result<String, LockError> {
        Ok(serde_yml::to_string(self)?)
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
