//! The paths a file is written through, its own and the two scratch paths beside it
//! (`spec/versioning-editions` "Provenance and lock").

use std::path::{Path, PathBuf};

use crate::file_identity::{NamedFile, Reach, Unidentified};

/// One of the paths a file is written through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePath {
    /// The file's own path.
    Final,
    /// One of the two scratch paths beside it.
    Scratch(Scratch),
}

/// One of the two scratch paths beside a file's own, through which the file is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scratch {
    /// The file's path with `.tmp` added, written and then renamed to the file's own.
    Staged,
    /// The file's path with `.bak` added, holding the file a write replaces until the new one is
    /// in place.
    Backup,
}

impl Scratch {
    const ALL: [Self; 2] = [Self::Staged, Self::Backup];

    /// This scratch path of the file at `final_path`.
    #[must_use]
    pub fn of(self, final_path: &Path) -> PathBuf {
        let suffix = match self {
            Self::Staged => ".tmp",
            Self::Backup => ".bak",
        };
        let mut raw = final_path.as_os_str().to_owned();
        raw.push(suffix);
        PathBuf::from(raw)
    }
}

/// One of the paths a file is written through that reaches another file, and how it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reached {
    /// The path that reaches the file.
    pub path: WritePath,
    /// Whether it reaches the file by name or through a link.
    pub by: Reach,
}

impl WritePath {
    /// The first path the file at `final_path` is written through that reaches `file`, its own
    /// path first, and how.
    ///
    /// # Errors
    ///
    /// An [`Unidentified`] when the file system cannot say what one of those paths reaches.
    pub fn reaching(file: &NamedFile, final_path: &Path) -> Result<Option<Reached>, Unidentified> {
        let scratch = Scratch::ALL.map(|scratch| (Self::Scratch(scratch), scratch.of(final_path)));
        for (path, at) in [(Self::Final, final_path.to_path_buf())]
            .into_iter()
            .chain(scratch)
        {
            if let Some(by) = file.reached_by(&at)? {
                return Ok(Some(Reached { path, by }));
            }
        }
        Ok(None)
    }
}

/// The two scratch paths beside the file at `final_path`, `.tmp` first.
#[must_use]
pub fn scratch_paths(final_path: &Path) -> [PathBuf; 2] {
    Scratch::ALL.map(|scratch| scratch.of(final_path))
}

/// Every path the file at `final_path` is written through, its own first.
#[must_use]
pub fn reserved_paths(final_path: &Path) -> [PathBuf; 3] {
    let [staged, backup] = scratch_paths(final_path);
    [final_path.to_path_buf(), staged, backup]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_written_through_its_path_with_tmp_and_with_bak_added() {
        assert_eq!(
            reserved_paths(Path::new("out/hut.nbt")),
            [
                PathBuf::from("out/hut.nbt"),
                PathBuf::from("out/hut.nbt.tmp"),
                PathBuf::from("out/hut.nbt.bak"),
            ],
        );
        assert_eq!(
            scratch_paths(Path::new("out/hut.nbt")),
            [
                PathBuf::from("out/hut.nbt.tmp"),
                PathBuf::from("out/hut.nbt.bak"),
            ],
        );
    }

    #[test]
    fn the_path_reaching_a_file_is_the_first_that_names_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        for name in ["hut.nbt", "notes.tmp", "notes.bak"] {
            std::fs::write(dir.path().join(name), "").expect("write file");
        }
        let reached = |file: &str, final_path: &str| {
            let file = NamedFile::at(&dir.path().join(file))
                .expect("identified")
                .expect("there");
            WritePath::reaching(&file, &dir.path().join(final_path)).expect("identified")
        };
        let by_name = |path| {
            Some(Reached {
                path,
                by: Reach::Name,
            })
        };

        assert_eq!(reached("hut.nbt", "hut.nbt"), by_name(WritePath::Final));
        assert_eq!(
            reached("notes.tmp", "notes"),
            by_name(WritePath::Scratch(Scratch::Staged))
        );
        assert_eq!(
            reached("notes.bak", "notes"),
            by_name(WritePath::Scratch(Scratch::Backup))
        );
        assert_eq!(reached("hut.nbt", "./hut.nbt"), by_name(WritePath::Final));
        assert_eq!(reached("hut.nbt", "hut"), None);
        assert_eq!(reached("notes.tmp", "notes.tmp"), by_name(WritePath::Final));
    }

    #[test]
    fn a_scratch_path_linked_to_a_file_reaches_it_through_the_link() {
        let dir = tempfile::tempdir().expect("temp dir");
        let source = dir.path().join("hut.crn");
        std::fs::write(&source, "").expect("write file");
        std::fs::hard_link(&source, dir.path().join("hut.nbt.tmp")).expect("hard link");
        let source = NamedFile::at(&source).expect("identified").expect("there");

        assert_eq!(
            WritePath::reaching(&source, &dir.path().join("hut.nbt")).expect("identified"),
            Some(Reached {
                path: WritePath::Scratch(Scratch::Staged),
                by: Reach::Link,
            }),
        );
    }
}
