//! Which file a path reaches, as the file system tells one file from another.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

/// A file or directory as the file system tells it apart, the same for every path that reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileIdentity(Key);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    /// Its device and inode.
    #[cfg(unix)]
    Inode(u64, u64),
    /// Its volume's serial number and its index on that volume.
    #[cfg(windows)]
    FileIndex(u64, u64),
    /// Its canonical path, which spells each component as the directory stores it.
    #[cfg(not(any(unix, windows)))]
    Canonical(PathBuf),
}

impl FileIdentity {
    /// What `path` reaches, with its symbolic links followed.
    ///
    /// # Errors
    ///
    /// The I/O failure asking the file system, `NotFound` when `path` reaches nothing.
    pub fn of(path: &Path) -> io::Result<Self> {
        key(path).map(Self)
    }
}

/// `path`'s device and inode, so every hard link is one file.
#[cfg(unix)]
fn key(path: &Path) -> io::Result<Key> {
    use std::os::unix::fs::MetadataExt as _;
    // `canonicalize` keeps each component's spelling on Linux, so `out` and `OUT` in a
    // case-folding directory would be two keys.
    let metadata = std::fs::metadata(path)?;
    Ok(Key::Inode(metadata.dev(), metadata.ino()))
}

/// `path`'s volume serial number and file index, so every hard link is one file.
#[cfg(windows)]
fn key(path: &Path) -> io::Result<Key> {
    // `canonicalize` gives a hard link the path it was opened through, so two links would be
    // two keys.
    let handle = winapi_util::Handle::from_path_any(path)?;
    let information = winapi_util::file::information(&handle)?;
    Ok(Key::FileIndex(
        information.volume_serial_number(),
        information.file_index(),
    ))
}

#[cfg(not(any(unix, windows)))]
fn key(path: &Path) -> io::Result<Key> {
    path.canonicalize().map(Key::Canonical)
}

/// Whether `a` and `b` reach one file, false when the file system cannot say what either reaches.
#[must_use]
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (FileIdentity::of(a), FileIdentity::of(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// How a path reaches the file another path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// By name: both paths lead to one entry of one directory, however each is spelled.
    Name,
    /// Through a link, symbolic or hard, that makes another entry the same file.
    Link,
}

/// A path the file system could not say the file of, for a reason other than there being none.
#[derive(Debug, thiserror::Error)]
#[error("`{}`: {error}", path.display())]
pub struct Unidentified {
    /// The path asked about.
    pub path: PathBuf,
    /// The file system's answer.
    pub error: io::Error,
}

/// A file as one path names it.
#[derive(Debug, Clone)]
pub struct NamedFile {
    path: PathBuf,
    file: FileIdentity,
}

impl NamedFile {
    /// The file `path` names, or `None` when `path` reaches nothing.
    ///
    /// # Errors
    ///
    /// An [`Unidentified`] when the file system cannot say what `path` reaches.
    pub fn at(path: &Path) -> Result<Option<Self>, Unidentified> {
        Ok(identity_if_any(path)?.map(|file| Self {
            path: path.to_path_buf(),
            file,
        }))
    }

    /// How `path` reaches this file, or `None` when it reaches another file or nothing.
    ///
    /// # Errors
    ///
    /// An [`Unidentified`] when the file system cannot say what `path`, or its directory, reaches.
    pub fn reached_by(&self, path: &Path) -> Result<Option<Reach>, Unidentified> {
        if identity_if_any(path)?.as_ref() != Some(&self.file) {
            return Ok(None);
        }
        let by_name = match (self.path.file_name(), path.file_name()) {
            (Some(own), Some(other)) if folded(own) == folded(other) => {
                identity(parent(&self.path))? == identity(parent(path))?
            }
            _ => false,
        };
        Ok(Some(if by_name { Reach::Name } else { Reach::Link }))
    }
}

/// What `path` reaches, or `None` when it is empty or a missing component, or one that is not a
/// directory, means it reaches nothing.
fn identity_if_any(path: &Path) -> Result<Option<FileIdentity>, Unidentified> {
    // A browser, which has no file system, answers even an empty path with `Unsupported`.
    if path.as_os_str().is_empty() {
        return Ok(None);
    }
    match FileIdentity::of(path) {
        Ok(file) => Ok(Some(file)),
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(Unidentified {
            path: path.to_path_buf(),
            error,
        }),
    }
}

/// What `path` reaches, which something must.
fn identity(path: &Path) -> Result<FileIdentity, Unidentified> {
    FileIdentity::of(path).map_err(|error| Unidentified {
        path: path.to_path_buf(),
        error,
    })
}

/// The directory `path` names its entry in, `.` for a bare file name.
fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// `name` without the trailing dots and spaces Windows drops, in lower case, so two spellings of
/// one entry fold alike.
fn folded(name: &OsStr) -> String {
    name.to_string_lossy()
        .trim_end_matches(['.', ' '])
        .to_lowercase()
}

#[cfg(test)]
mod tests;
