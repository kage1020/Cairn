//! Staging a set of output files and committing them together; privacy keeps a staged file
//! reachable only through the scratch path chosen here.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What a staged file is going to become, so a failure can name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Artifact,
    Lockfile,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Artifact => f.write_str("artifact"),
            Self::Lockfile => f.write_str("lockfile"),
        }
    }
}

/// Which half of the commit failed; the two need different fixes at different paths.
#[derive(Debug, Clone, Copy)]
enum Step {
    /// Moving what is already at the destination out of the way.
    Displace,
    /// Renaming the staged file into the destination.
    Place,
}

/// A commit that could not complete. The directory has been put back.
#[derive(Debug)]
pub struct CommitFailure {
    kind: Kind,
    step: Step,
    /// The backup for a displace, the destination for a place.
    path: PathBuf,
    source: io::Error,
}

impl fmt::Display for CommitFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            kind,
            step,
            path,
            source,
        } = self;
        match step {
            Step::Displace => write!(
                f,
                "the existing {kind} could not be moved aside to `{}`: {source}",
                path.display(),
            ),
            Step::Place => write!(f, "writing {kind} `{}`: {source}", path.display()),
        }
    }
}

/// A file written beside its destination, waiting to be renamed there.
struct Staged {
    kind: Kind,
    final_path: PathBuf,
    tmp_path: PathBuf,
}

/// A destination's previous file, moved aside so a failed commit can put it back.
struct Displaced {
    final_path: PathBuf,
    backup_path: PathBuf,
}

/// Output files written but not yet in place.
#[derive(Default)]
pub struct StagedSet {
    entries: Vec<Staged>,
}

/// `path` with `suffix` appended to the whole file name, extension included.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut raw = path.as_os_str().to_owned();
    raw.push(suffix);
    PathBuf::from(raw)
}

/// Every path the set will create or rename for `final_path`, staging scratch included.
pub fn reserved_paths(final_path: &Path) -> [PathBuf; 3] {
    [
        final_path.to_path_buf(),
        with_suffix(final_path, ".tmp"),
        with_suffix(final_path, ".bak"),
    ]
}

/// What is sitting at a destination right now.
enum Occupant {
    Vacant,
    /// A file or symlink, which `rename` can move aside and put back.
    Movable,
    /// A directory: not backed up, so the place step fails naming it.
    Directory,
}

/// An unreadable destination is an error, never a vacancy the commit could overwrite.
fn occupant(path: &Path) -> io::Result<Occupant> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(Occupant::Directory),
        Ok(_) => Ok(Occupant::Movable),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Occupant::Vacant),
        Err(err) => Err(err),
    }
}

impl StagedSet {
    /// Write one file beside `final_path`.
    ///
    /// # Errors
    ///
    /// The I/O failure, after removing the partial scratch file.
    pub fn stage(
        &mut self,
        final_path: &Path,
        kind: Kind,
        write: impl FnOnce(&mut io::BufWriter<fs::File>) -> io::Result<()>,
    ) -> io::Result<()> {
        use std::io::Write as _;

        let tmp_path = with_suffix(final_path, ".tmp");
        let result = (|| {
            let mut file = io::BufWriter::new(fs::File::create(&tmp_path)?);
            write(&mut file)?;
            file.flush()?;
            // Unsynced, a crash after the rename can leave a correctly named, half-written file.
            file.get_ref().sync_all()
        })();
        if let Err(err) = result {
            remove_scratch(&tmp_path);
            return Err(err);
        }
        self.entries.push(Staged {
            kind,
            final_path: final_path.to_path_buf(),
            tmp_path,
        });
        Ok(())
    }

    /// Throw the staged files away without touching any destination.
    pub fn discard(self) {
        self.discard_scratch();
    }

    /// Move every staged file into place, or leave the directory as it was, and return the
    /// paths written.
    ///
    /// # Errors
    ///
    /// The first rename that failed, after undoing the ones before it.
    pub fn commit(self) -> Result<Vec<PathBuf>, CommitFailure> {
        let mut displaced: Vec<Displaced> = Vec::new();
        let mut committed: Vec<PathBuf> = Vec::new();

        for entry in &self.entries {
            match occupant(&entry.final_path) {
                Ok(Occupant::Movable) => {
                    let backup_path = with_suffix(&entry.final_path, ".bak");
                    if let Err(err) = fs::rename(&entry.final_path, &backup_path) {
                        undo(&displaced, &committed);
                        self.discard_scratch();
                        return Err(CommitFailure {
                            kind: entry.kind,
                            step: Step::Displace,
                            path: backup_path,
                            source: err,
                        });
                    }
                    displaced.push(Displaced {
                        final_path: entry.final_path.clone(),
                        backup_path,
                    });
                }
                Ok(Occupant::Vacant | Occupant::Directory) => {}
                Err(err) => {
                    undo(&displaced, &committed);
                    self.discard_scratch();
                    return Err(CommitFailure {
                        kind: entry.kind,
                        step: Step::Displace,
                        path: entry.final_path.clone(),
                        source: err,
                    });
                }
            }
            if let Err(err) = fs::rename(&entry.tmp_path, &entry.final_path) {
                undo(&displaced, &committed);
                self.discard_scratch();
                return Err(CommitFailure {
                    kind: entry.kind,
                    step: Step::Place,
                    path: entry.final_path.clone(),
                    source: err,
                });
            }
            committed.push(entry.final_path.clone());
        }

        for entry in &displaced {
            if let Err(err) = fs::remove_file(&entry.backup_path) {
                eprintln!(
                    "warning: `{}` was left behind after replacing `{}`: {err}",
                    entry.backup_path.display(),
                    entry.final_path.display(),
                );
            }
        }
        Ok(committed)
    }

    fn discard_scratch(&self) {
        for entry in &self.entries {
            remove_scratch(&entry.tmp_path);
        }
    }
}

/// Remove a scratch file, warning when it is left behind.
pub fn remove_scratch(tmp_path: &Path) {
    match fs::remove_file(tmp_path) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => eprintln!(
            "warning: scratch file `{}` was left behind: {err}",
            tmp_path.display(),
        ),
    }
}

/// Put the directory back the way the commit found it, reporting every step that fails.
fn undo(displaced: &[Displaced], committed: &[PathBuf]) {
    for path in committed {
        if let Err(err) = fs::remove_file(path) {
            eprintln!(
                "error: `{}` was written by a build that then failed, and could not be \
                 removed: {err}",
                path.display(),
            );
        }
    }
    for entry in displaced {
        let _ = fs::remove_file(&entry.final_path);
        if let Err(err) = fs::rename(&entry.backup_path, &entry.final_path) {
            eprintln!(
                "error: `{}` could not be restored from `{}`: {err}",
                entry.final_path.display(),
                entry.backup_path.display(),
            );
        }
    }
}
