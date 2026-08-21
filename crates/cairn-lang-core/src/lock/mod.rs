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
    LOCK_SCHEMA_VERSION, LockEdition, LockInputs, LockPlacement, LockTarget, LockWalkway, Lockfile,
    MemberSensitivity,
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
    /// The document declares a schema revision this build does not know.
    ///
    /// Refused rather than read: the field names would be the same, so a
    /// later format would deserialise into this struct and mean something
    /// else.
    #[error("lockfile declares schema version {found}; this build reads {supported}")]
    UnsupportedSchemaVersion {
        /// Version the document declared.
        found: u32,
        /// Highest version this build understands.
        supported: u32,
    },
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
    /// The body always ends with exactly one newline.
    ///
    /// `serde_yml` closes an empty flow sequence without a break, so a
    /// document whose last-written field is one — the common case, where
    /// `member_version_sensitivity`, `placements` and `walkways` are all
    /// empty and the latter two are skipped — ends mid-line. That makes
    /// `git diff` report `\ No newline at end of file` on every change and
    /// an appended byte corrupt the document. The contract here is
    /// unconditional; the shape that used to break it is not.
    ///
    /// # Errors
    ///
    /// Propagates YAML encode failure.
    pub fn to_yaml(&self) -> Result<String, LockError> {
        let mut body = serde_yml::to_string(self)?;
        if !body.ends_with('\n') {
            body.push('\n');
        }
        Ok(body)
    }

    /// Read a lockfile back from `path`.
    ///
    /// # Errors
    ///
    /// Propagates I/O failure from reading `path`, and everything
    /// [`Self::from_yaml`] can return.
    pub fn read_from_path(path: &Path) -> Result<Self, LockError> {
        Self::from_yaml(&fs::read_to_string(path)?)
    }

    /// Decode the YAML body [`Self::to_yaml`] would have written.
    ///
    /// The mirror of [`Self::to_yaml`], and where the schema version is
    /// enforced — so a caller that already holds the bytes has the same
    /// gate available as one reading a file. It is not a chokepoint:
    /// `Lockfile` is public and derives `Deserialize`, so
    /// `serde_yml::from_str` reaches one without the version check. This is
    /// the entry point the compiler uses.
    ///
    /// # Errors
    ///
    /// Returns [`LockError::UnsupportedSchemaVersion`] when the document
    /// declares a revision above [`LOCK_SCHEMA_VERSION`], and
    /// [`LockError::Yaml`] when its shape does not match [`Lockfile`] —
    /// which includes a key the schema does not declare, at any depth.
    ///
    /// The version is read first and on its own. Deciding it after the
    /// full parse would put it behind `deny_unknown_fields`, so any later
    /// format that adds a key would be reported as malformed rather than
    /// as newer — and the leading field would not mean what its doc says.
    pub fn from_yaml(body: &str) -> Result<Self, LockError> {
        let found = schema::declared_schema_version(body)?;
        if found > LOCK_SCHEMA_VERSION {
            return Err(LockError::UnsupportedSchemaVersion {
                found,
                supported: LOCK_SCHEMA_VERSION,
            });
        }
        Ok(serde_yml::from_str(body)?)
    }
}
