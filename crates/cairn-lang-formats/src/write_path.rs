//! The paths a file is written through, its own and the two scratch paths beside it
//! (`spec/versioning-editions` "Provenance and lock").

use std::path::{Path, PathBuf};

use crate::file_identity::FileIdentity;

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

impl WritePath {
    /// The first path the file at `final_path` is written through that reaches `file`, its own
    /// path first.
    #[must_use]
    pub fn reaching(file: &FileIdentity, final_path: &Path) -> Option<Self> {
        let scratch = Scratch::ALL.map(|scratch| (Self::Scratch(scratch), scratch.of(final_path)));
        // An error asking of a path means no file is there, or one a write would fail at as well.
        [(Self::Final, final_path.to_path_buf())]
            .into_iter()
            .chain(scratch)
            .find(|(_, at)| FileIdentity::of(at).is_ok_and(|reached| reached == *file))
            .map(|(path, _)| path)
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
            let file = FileIdentity::of(&dir.path().join(file)).expect("identity");
            WritePath::reaching(&file, &dir.path().join(final_path))
        };

        assert_eq!(reached("hut.nbt", "hut.nbt"), Some(WritePath::Final));
        assert_eq!(
            reached("notes.tmp", "notes"),
            Some(WritePath::Scratch(Scratch::Staged))
        );
        assert_eq!(
            reached("notes.bak", "notes"),
            Some(WritePath::Scratch(Scratch::Backup))
        );
        assert_eq!(reached("hut.nbt", "./hut.nbt"), Some(WritePath::Final));
        assert_eq!(reached("hut.nbt", "hut"), None);
        assert_eq!(reached("notes.tmp", "notes.tmp"), Some(WritePath::Final));
    }
}
