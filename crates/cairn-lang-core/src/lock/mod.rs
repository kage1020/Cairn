//! Lockfile reader and writer (`spec/versioning-editions` "Provenance and lock").

mod hash;
mod schema;

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

pub use hash::{HashError, HashHex, HashParseError, hash_resolved_ir, hash_source};
pub use schema::{
    LOCK_SCHEMA_VERSION, LockInputs, LockPlacement, LockTarget, LockWalkway, Lockfile,
    MemberSensitivity, RefusedIdentifierLock,
};

use serde::{Serialize, Serializer};
use thiserror::Error;

use crate::ids::IdError;

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
}

/// A lockfile's text, classified the way every reader of one treats it.
#[derive(Debug)]
pub enum LockfileRead {
    /// A document the schema accepts.
    Valid(Lockfile),
    /// A document the schema accepts but for an identifier the current rule refuses, as one an
    /// earlier Cairn wrote can be.
    RefusedIdentifier(RefusedIdentifierLock),
    /// A document declaring a schema revision newer than this build reads.
    NewerSchema {
        /// Version the document declared.
        found: u32,
        /// Highest version this build understands.
        supported: u32,
    },
    /// A document the schema refuses for any other reason.
    Invalid(LockError),
}

impl LockfileRead {
    /// The lockfile text `body`, classified.
    #[must_use]
    pub fn from_body(body: &str) -> Self {
        let strict = match Lockfile::from_yaml(body) {
            Ok(lock) => return Self::Valid(lock),
            Err(LockError::UnsupportedSchemaVersion { found, supported }) => {
                return Self::NewerSchema { found, supported };
            }
            Err(err) => err,
        };
        match schema::refused_identifier(body) {
            Ok(Some(refused)) => Self::RefusedIdentifier(refused),
            // The probe's error, not the strict one: the strict read may have stopped at an
            // identifier before reaching what is really wrong.
            Err(cause) => Self::Invalid(cause.into()),
            // Unreachable while the probe mirrors the strict schema.
            Ok(None) => Self::Invalid(strict),
        }
    }

    /// The lockfile at `path`, classified, or `None` when no file is there.
    ///
    /// # Errors
    ///
    /// The failure reading `path` as UTF-8 text, a directory at `path` among them.
    pub fn from_path(path: &Path) -> Result<Option<Self>, io::Error> {
        // An empty path names no file, but reading one fails as the platform words it: a browser,
        // which has no file system, answers `Unsupported`, not `NotFound`.
        if path.as_os_str().is_empty() {
            return Ok(None);
        }
        // Checked before the read, since Windows answers a directory with `PermissionDenied`.
        if path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                "a directory, not a file",
            ));
        }
        match fs::read_to_string(path) {
            Ok(body) => Ok(Some(Self::from_body(&body))),
            // Unix answers a file in place of a directory with `NotADirectory`, Windows with
            // `NotFound`; either way no file is at `path`.
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }
}

/// What the lockfile at a path holds, as `spec/versioning-editions` "The `verified target` row"
/// reports it.
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
        /// The identifier the current rule refuses, when the lock records one.
        #[serde(
            skip_serializing_if = "Option::is_none",
            serialize_with = "as_its_message"
        )]
        refused_identifier: Option<IdError>,
    },
    /// A document declaring a schema revision newer than this build reads.
    NewerSchema {
        /// Version the document declared.
        lock_schema_version: u32,
        /// Highest version this build understands.
        supported: u32,
    },
    /// A file that cannot be read as text, or a document the schema refuses for another reason.
    Unreadable {
        /// Which of the two it is.
        kind: UnreadableKind,
        /// The cause, as [`LockError`] words it.
        error: String,
    },
}

/// Why a lockfile is [`LockReading::Unreadable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnreadableKind {
    /// The path could not be read as UTF-8 text.
    Io,
    /// The text is a document the schema refuses.
    Invalid,
}

#[expect(
    clippy::ref_option,
    reason = "`serialize_with` hands the field over by reference"
)]
fn as_its_message<S: Serializer>(
    refused: &Option<IdError>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match refused {
        Some(refused) => serializer.collect_str(refused),
        None => serializer.serialize_none(),
    }
}

impl LockReading {
    /// The lockfile at `path`, read for the source whose text is `source`.
    #[must_use]
    pub fn of(path: &Path, source: &str) -> Self {
        match LockfileRead::from_path(path) {
            Ok(Some(read)) => Self::from_read(read, source),
            Ok(None) => Self::Missing,
            Err(err) => Self::Unreadable {
                kind: UnreadableKind::Io,
                error: LockError::Io(err).to_string(),
            },
        }
    }

    /// The lockfile text `body`, read for the source whose text is `source`.
    #[must_use]
    pub fn from_body(body: &str, source: &str) -> Self {
        Self::from_read(LockfileRead::from_body(body), source)
    }

    fn from_read(read: LockfileRead, source: &str) -> Self {
        let source_hash = hash_source(source);
        match read {
            LockfileRead::Valid(lock) => Self::Read {
                source_unchanged: lock.source_hash == source_hash,
                target: lock.target,
                verified: lock.verified,
                cairn_version: lock.cairn_version,
                refused_identifier: None,
            },
            LockfileRead::RefusedIdentifier(refused) => Self::Read {
                source_unchanged: refused.source_hash == source_hash,
                target: refused.target,
                verified: refused.verified,
                cairn_version: refused.cairn_version,
                refused_identifier: Some(refused.error),
            },
            LockfileRead::NewerSchema { found, supported } => Self::NewerSchema {
                lock_schema_version: found,
                supported,
            },
            LockfileRead::Invalid(err) => Self::Unreadable {
                kind: UnreadableKind::Invalid,
                error: err.to_string(),
            },
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
