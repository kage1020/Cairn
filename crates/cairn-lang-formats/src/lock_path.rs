//! Where `cairn compile` writes a source's lock, and the paths it refuses to write it at
//! (`spec/versioning-editions` "Provenance and lock").

use std::path::{Path, PathBuf};

use crate::file_identity::FileIdentity;
use crate::write_path::WritePath;

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
    /// The lock would be written through this path, which reaches the source's file.
    Source(WritePath),
    /// The path's file name ends in `.crn`, as a source's does.
    SourceName,
}

/// Why `cairn compile` refuses `--lock lock` for `source`, or `None` when it writes there.
#[must_use]
pub fn lock_path_refusal(source: &Path, lock: &Path) -> Option<LockPathRefusal> {
    if let Some(path) = reaching_source(source, lock) {
        return Some(LockPathRefusal::Source(path));
    }
    lock.file_name()
        .is_some_and(|name| {
            name.to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".crn")
        })
        .then_some(LockPathRefusal::SourceName)
}

/// The path through which `cairn compile` would write `<source>.lock` over the source, or `None`
/// when it writes there.
#[must_use]
pub fn default_lock_refusal(source: &Path) -> Option<WritePath> {
    // Every path `<source>.lock` is written through is named apart from the source, so only a
    // link makes one of them the source.
    reaching_source(source, &default_lock_path(source))
}

/// The first path a lock at `lock` is written through that reaches `source`'s file.
fn reaching_source(source: &Path, lock: &Path) -> Option<WritePath> {
    FileIdentity::of(source)
        .ok()
        .and_then(|source| WritePath::reaching(&source, lock))
}

#[cfg(test)]
mod tests;
