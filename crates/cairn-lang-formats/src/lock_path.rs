//! Where `cairn compile` writes a source's lock, and the paths it refuses to write it at
//! (`spec/versioning-editions` "Provenance and lock").

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::file_identity::{NamedFile, Unidentified};
use crate::write_path::{Reached, WritePath};

/// `<source>.lock`, keeping `.crn` so no two sources share a lock.
#[must_use]
pub fn default_lock_path(source: &Path) -> PathBuf {
    let mut path = source.as_os_str().to_owned();
    path.push(".lock");
    PathBuf::from(path)
}

/// Why `cairn compile` refuses a `--lock` path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockPathRefusal {
    /// The lock would be written through a path that reaches the source's file.
    Source(Reached),
    /// The path's file name ends in `.crn`, as a source's does.
    SourceName,
}

/// Why `cairn compile` refuses `--lock lock` for `source`, or `None` when it writes there.
///
/// # Errors
///
/// An [`Unidentified`] when the file system cannot say whether a path the lock is written through
/// reaches the source.
pub fn lock_path_refusal(
    source: &Path,
    lock: &Path,
) -> Result<Option<LockPathRefusal>, Unidentified> {
    if let Some(reached) = reaching_source(source, lock)? {
        return Ok(Some(LockPathRefusal::Source(reached)));
    }
    Ok(lock
        .file_name()
        .is_some_and(names_a_source)
        .then_some(LockPathRefusal::SourceName))
}

/// The path through which `cairn compile` would write `<source>.lock` over the source, or `None`
/// when it writes there.
///
/// # Errors
///
/// An [`Unidentified`] when the file system cannot say whether one of those paths reaches the
/// source.
pub fn default_lock_refusal(source: &Path) -> Result<Option<WritePath>, Unidentified> {
    // Every path `<source>.lock` is written through is named apart from the source, so only a
    // link makes one of them the source.
    Ok(reaching_source(source, &default_lock_path(source))?.map(|reached| reached.path))
}

/// Whether a file `name` ends in `.crn`, in any case.
fn names_a_source(name: &OsStr) -> bool {
    name.to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(".crn")
}

/// The first path a lock at `lock` is written through that reaches `source`'s file.
fn reaching_source(source: &Path, lock: &Path) -> Result<Option<Reached>, Unidentified> {
    match NamedFile::at(source)? {
        Some(source) => WritePath::reaching(&source, lock),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests;
