//! Which file a path reaches, as the file system tells one file from another.

use std::io;
use std::path::Path;

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
    Canonical(std::path::PathBuf),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spelling_of_a_file_is_the_file_and_no_other_is() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("hut.crn");
        std::fs::write(&file, "").expect("write file");
        std::fs::write(dir.path().join("other.crn"), "").expect("write other");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).expect("make sub");

        assert!(same_file(&file, &sub.join("..").join("hut.crn")));
        assert!(same_file(&file, &dir.path().join(".").join("hut.crn")));
        assert!(!same_file(&file, &dir.path().join("other.crn")));
        assert!(!same_file(&file, &dir.path().join("missing.crn")));
        assert!(!same_file(
            &dir.path().join("missing.crn"),
            &dir.path().join("missing.crn")
        ));
    }

    #[test]
    fn a_hard_link_to_a_file_is_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("hut.crn");
        std::fs::write(&file, "").expect("write file");
        std::fs::create_dir(dir.path().join("sub")).expect("make sub");

        for link in ["hard", "sub/hut.crn"] {
            std::fs::hard_link(&file, dir.path().join(link)).expect("hard link");
            assert!(same_file(&file, &dir.path().join(link)), "{link}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_to_a_file_is_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("hut.crn");
        std::fs::write(&file, "").expect("write file");
        std::os::unix::fs::symlink(&file, dir.path().join("soft")).expect("symlink");
        std::os::unix::fs::symlink(dir.path(), dir.path().join("dir")).expect("symlink dir");

        for link in ["soft", "dir/hut.crn"] {
            assert!(same_file(&file, &dir.path().join(link)), "{link}");
        }
    }
}
