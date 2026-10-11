//! `build.cairn.lock` reader/writer.
//!
//! The lockfile pins three pieces of state at the moment of a successful compile: the source bytes,
//! the target (edition, Minecraft version and `DataVersion`), and the resolved IR. Re-running
//! `cairn compile` against a different target prints the divergence by comparing fields.
//!
//! The canonical name for a single-source build is `build.cairn.lock` (`spec/versioning-editions`
//! "Provenance and lock"), but the CLI defaults to `<source>.lock` so multi-source workspaces stay
//! unambiguous; the schema does not care which path it lives at.

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
    /// Writes through a sibling temporary file and a rename, so a failure part-way cannot leave a
    /// truncated lockfile where a valid one was: `fs::write` truncates in place, and a full disk or
    /// a lost handle would leave a file that does not parse, worse than the old contents or no file
    /// at all.
    ///
    /// # Errors
    ///
    /// Propagates I/O failure from creating or writing `path`, or YAML encoder failure from the
    /// schema.
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
    /// Separate from the I/O so a caller placing several files together can fail out of the encode
    /// before touching the filesystem.
    ///
    /// The body always ends with exactly one newline. One that stops mid-line makes `git diff`
    /// report `\ No newline at end of file` on every change, and an appended byte corrupts the
    /// document. The guard is unconditional so this holds whatever the encoder does with the last
    /// field, `member_version_sensitivity`, which is often empty.
    ///
    /// # Errors
    ///
    /// Propagates YAML encode failure.
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
    /// Propagates I/O failure from reading `path`, and everything
    /// [`Self::from_yaml`] can return.
    pub fn read_from_path(path: &Path) -> Result<Self, LockError> {
        Self::from_yaml(&fs::read_to_string(path)?)
    }

    /// Decode the YAML body [`Self::to_yaml`] would have written.
    ///
    /// Where the schema version is enforced, so a caller holding the bytes has the same gate as one
    /// reading a file. Not a chokepoint: `Lockfile` is public and derives `Deserialize`, so
    /// `serde_norway::from_str` reaches one without the check. This is the entry point the compiler
    /// uses.
    ///
    /// The version is read first and on its own. Decided after the full parse, it would sit behind
    /// `deny_unknown_fields`, and a later format that adds a key would be reported as malformed
    /// rather than as newer.
    ///
    /// # Errors
    ///
    /// Returns [`LockError::UnsupportedSchemaVersion`] when the document declares a revision above
    /// [`LOCK_SCHEMA_VERSION`], and [`LockError::Yaml`] when its shape does not match [`Lockfile`],
    /// which includes a key the schema does not declare, at any depth.
    pub fn from_yaml(body: &str) -> Result<Self, LockError> {
        gate_schema_version(body)?;
        Ok(serde_norway::from_str(body)?)
    }

    /// The first identifier in `body` the current id rule refuses, when that is all that is wrong
    /// with `body`, with the target and version-sensitive members the document records.
    ///
    /// For a caller whose [`Self::from_yaml`] failed: it tells a lock valid under an earlier,
    /// looser identifier rule apart from a broken one, and recovers the target it was verified for.
    /// `body` is read as strictly as [`Self::from_yaml`] reads it (the same schema-version gate,
    /// every field required where it is required there, no undeclared key at any depth) except that
    /// the seven identifier fields (`site` and `id` of each placement; `site`, and the `place` and
    /// `port` of `from` and `to`, of each walkway) are read as plain strings.
    ///
    /// Identifiers are checked in field order: every placement, each by `site` then `id`, then
    /// every walkway, each by `site`, `from.place`, `from.port`, `to.place`, `to.port`. The first
    /// one refused is reported. That is the order this compiler writes them in, not necessarily a
    /// hand-written document's, so it need not be the identifier the strict read stopped at.
    ///
    /// `Ok(None)` means `body` is valid and records no refused identifier, a document
    /// [`Self::from_yaml`] reads.
    ///
    /// # Errors
    ///
    /// Returns [`LockError::UnsupportedSchemaVersion`] when `body` declares a revision above
    /// [`LOCK_SCHEMA_VERSION`], and [`LockError::Yaml`] when `body` is not a valid lockfile even
    /// with its identifiers read as plain strings. That error names what is wrong besides the
    /// identifiers, which is the reason to report: the strict read's own error can be an identifier
    /// it happened to meet first.
    pub fn refused_identifier(body: &str) -> Result<Option<RefusedIdentifierLock>, LockError> {
        gate_schema_version(body)?;
        Ok(schema::refused_identifier(body)?)
    }
}

/// Refuse a document that declares a schema revision this build does not
/// read, before anything else in it is read. Shared by every reader of a
/// whole document, so none of them can parse a later format as this one.
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
