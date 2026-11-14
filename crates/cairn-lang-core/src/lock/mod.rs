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

use serde::Serialize;
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
        let refused = schema::refused_identifier(body)?;
        Ok(refused.map(|(error, probe)| RefusedIdentifierLock {
            error,
            target: probe.target,
            member_version_sensitivity: probe.member_version_sensitivity,
        }))
    }
}

/// What the lockfile at a path says it verified, as `spec/versioning-editions` "The `verified
/// target` row" reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LockReading {
    /// No file at the path.
    Missing,
    /// A document the schema accepts, or one it refuses only for an identifier.
    Read {
        /// The target the lock records.
        target: LockTarget,
        /// The lock's `verified` field.
        verified: bool,
        /// Whether the source hashes to the lock's `source_hash`.
        source_unchanged: bool,
        /// The release that wrote the lock.
        cairn_version: String,
        /// The identifier the current rule refuses, as its error reads, when the lock records one.
        #[serde(skip_serializing_if = "Option::is_none")]
        refused_identifier: Option<String>,
    },
    /// A document declaring a schema revision newer than this build reads.
    NewerSchema {
        /// Version the document declared.
        lock_schema_version: u32,
        /// Highest version this build understands.
        supported: u32,
    },
    /// A file that cannot be read, or a document the schema refuses for another reason.
    Unreadable {
        /// The cause, as [`LockError`] words it.
        error: String,
    },
}

impl LockReading {
    /// Read the lockfile at `path` for the source whose text is `source`.
    #[must_use]
    pub fn of(path: &Path, source: &str) -> Self {
        // An empty path names no file, but reading one fails as the platform words it: a browser,
        // which has no file system, answers `Unsupported`, not `NotFound`.
        if path.as_os_str().is_empty() {
            return Self::Missing;
        }
        let body = match fs::read_to_string(path) {
            Ok(body) => body,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::Missing,
            Err(err) => return Self::unreadable(&err.into()),
        };
        let source_hash = hash_source(source);
        let err = match Lockfile::from_yaml(&body) {
            Ok(lock) => {
                return Self::Read {
                    source_unchanged: lock.source_hash == source_hash,
                    target: lock.target,
                    verified: lock.verified,
                    cairn_version: lock.cairn_version,
                    refused_identifier: None,
                };
            }
            Err(LockError::UnsupportedSchemaVersion { found, supported }) => {
                return Self::NewerSchema {
                    lock_schema_version: found,
                    supported,
                };
            }
            Err(err) => err,
        };
        match schema::refused_identifier(&body) {
            Ok(Some((error, probe))) => Self::Read {
                source_unchanged: probe.source_hash == source_hash,
                target: probe.target,
                verified: probe.verified,
                cairn_version: probe.cairn_version,
                refused_identifier: Some(error.to_string()),
            },
            // The probe's error, not the strict one: the strict read may have stopped at an
            // identifier before reaching what is really wrong.
            Err(cause) => Self::unreadable(&cause.into()),
            Ok(None) => Self::unreadable(&err),
        }
    }

    fn unreadable(err: &LockError) -> Self {
        Self::Unreadable {
            error: err.to_string(),
        }
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
