//! Lockfile reader and writer (`spec/versioning-editions` "Provenance and lock").

mod hash;
mod schema;

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub use hash::{HashError, HashHex, HashParseError, hash_resolved_ir, hash_source};
pub use schema::{
    LOCK_SCHEMA_VERSION, LockInputs, LockPlacement, LockTarget, LockWalkway, Lockfile,
    MemberSensitivity, RefusedIdentifierLock,
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
    Yaml(#[from] serde_norway::Error),
    /// The document declares a schema revision newer than this build reads.
    #[error("lockfile declares schema version {found}; this build reads {supported}")]
    UnsupportedSchemaVersion {
        /// Version the document declared.
        found: u32,
        /// Highest version this build understands.
        supported: u32,
    },
}

impl Lockfile {
    /// Write the lockfile to `path` as YAML, replacing any existing file atomically.
    ///
    /// # Errors
    ///
    /// I/O failure writing `path`, or YAML encoder failure.
    pub fn write_to_path(&self, path: &Path) -> Result<(), LockError> {
        let body = self.to_yaml()?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);

        let staged = (|| -> Result<(), std::io::Error> {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(body.as_bytes())?;
            file.flush()?;
            // The rename publishes the name, not the bytes.
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

    /// Serialise to the YAML body [`Self::write_to_path`] writes, ending in exactly one newline.
    ///
    /// # Errors
    ///
    /// YAML encoder failure.
    pub fn to_yaml(&self) -> Result<String, LockError> {
        let mut body = serde_norway::to_string(self)?;
        if !body.ends_with('\n') {
            body.push('\n');
        }
        Ok(body)
    }

    /// Read a lockfile back from `path`.
    ///
    /// # Errors
    ///
    /// I/O failure reading `path`, or anything [`Self::from_yaml`] returns.
    pub fn read_from_path(path: &Path) -> Result<Self, LockError> {
        Self::from_yaml(&fs::read_to_string(path)?)
    }

    /// Decode a lockfile body, checking its schema version first so a newer format reads as newer,
    /// not malformed.
    ///
    /// # Errors
    ///
    /// [`LockError::UnsupportedSchemaVersion`] for a revision above [`LOCK_SCHEMA_VERSION`], or
    /// [`LockError::Yaml`] when the shape does not match [`Lockfile`].
    pub fn from_yaml(body: &str) -> Result<Self, LockError> {
        gate_schema_version(body)?;
        Ok(serde_norway::from_str(body)?)
    }

    /// The first identifier the current id rule refuses, with the target and members `body`
    /// records, when that is all that is wrong with it; `Ok(None)` when `body` is valid.
    ///
    /// # Errors
    ///
    /// [`LockError::UnsupportedSchemaVersion`] for a newer revision, or [`LockError::Yaml`] when
    /// `body` is invalid even with its identifiers read as plain strings.
    pub fn refused_identifier(body: &str) -> Result<Option<RefusedIdentifierLock>, LockError> {
        gate_schema_version(body)?;
        Ok(schema::refused_identifier(body)?)
    }
}

fn gate_schema_version(body: &str) -> Result<(), LockError> {
    let found = schema::declared_schema_version(body)?;
    if found > LOCK_SCHEMA_VERSION {
        return Err(LockError::UnsupportedSchemaVersion {
            found,
            supported: LOCK_SCHEMA_VERSION,
        });
    }
    Ok(())
}
