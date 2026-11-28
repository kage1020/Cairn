//! Staging a set of output files and committing them together; privacy keeps a staged file
//! reachable only through the scratch path chosen here.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cairn_lang_formats::write_path::{Scratch, scratch_paths};

/// What a staged file is going to become, so a failure can name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Artifact,
    Lockfile,
    Source,
    Structure,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Artifact => f.write_str("artifact"),
            Self::Lockfile => f.write_str("lockfile"),
            Self::Source => f.write_str("source"),
            Self::Structure => f.write_str("structure file"),
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

/// What is sitting at a destination right now.
enum Occupant {
    Vacant,
    /// A file or symlink, which `rename` can move aside and put back.
    Movable,
    /// A directory: not backed up, so the place step fails naming it.
    Directory,
}

/// A scratch path beside a destination that already holds an entry, or that the file system cannot
/// look at, which a write would go through or delete.
#[derive(Debug)]
pub struct ScratchBlocked {
    kind: Kind,
    final_path: PathBuf,
    scratch: PathBuf,
    /// Why the path could not be looked at, or `None` for an entry there.
    unlookable: Option<io::Error>,
}

impl ScratchBlocked {
    /// Print what is in the way and how to clear it, naming `command` as the one to run again.
    pub fn report(&self, command: &str) {
        let Self {
            kind,
            final_path,
            scratch,
            unlookable,
        } = self;
        let (shown, at) = (final_path.display(), scratch.display());
        match unlookable {
            None => {
                eprintln!(
                    "error: {kind} `{shown}` is written through `{at}`, which already exists"
                );
                eprintln!(
                    "  note: a file is written at its path with `.tmp` added and renamed into \
                     place, and one it replaces is moved aside to its path with `.bak` added, so \
                     the entry may be one an earlier run left behind"
                );
                eprintln!("  fix: move `{at}` aside, then {command} again");
            }
            Some(err) => {
                eprintln!(
                    "error: cannot tell whether `{at}`, which {kind} `{shown}` is written \
                     through, is free: {err}"
                );
                eprintln!("  fix: check that `{at}` can be read, then {command} again");
            }
        }
    }
}

/// Each scratch path beside `final_path` that holds an entry, a symbolic link included, or that
/// the file system cannot look at.
pub fn blocked_scratch(kind: Kind, final_path: &Path) -> Vec<ScratchBlocked> {
    scratch_paths(final_path)
        .into_iter()
        .filter_map(|scratch| {
            let unlookable = match fs::symlink_metadata(&scratch) {
                // Under a file where a directory should be, the write reports the real fault.
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                    ) =>
                {
                    return None;
                }
                Ok(_) => None,
                Err(err) => Some(err),
            };
            Some(ScratchBlocked {
                kind,
                final_path: final_path.to_path_buf(),
                scratch,
                unlookable,
            })
        })
        .collect()
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
    /// Write one file beside `final_path`, at a scratch path that holds nothing yet.
    ///
    /// # Errors
    ///
    /// The I/O failure, after removing the scratch file if this call created it.
    pub fn stage(
        &mut self,
        final_path: &Path,
        kind: Kind,
        write: impl FnOnce(&mut io::BufWriter<fs::File>) -> io::Result<()>,
    ) -> io::Result<()> {
        use std::io::Write as _;

        let tmp_path = Scratch::Staged.of(final_path);
        // `create_new` neither truncates an entry there nor follows a symbolic link.
        let created = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        let result = (|| {
            let mut file = io::BufWriter::new(created);
            write(&mut file)?;
            file.flush()?;
            // Unsynced, a crash after the rename can leave a correctly named, half-written file.
            file.get_ref().sync_all()
        })();
        if let Err(err) = result {
            remove_staged(&tmp_path);
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
    /// The first step that failed, after undoing the ones before it.
    pub fn commit(self) -> Result<Vec<PathBuf>, CommitFailure> {
        let mut displaced: Vec<Displaced> = Vec::new();
        let mut committed: Vec<PathBuf> = Vec::new();

        for entry in &self.entries {
            match occupant(&entry.final_path) {
                Ok(Occupant::Movable) => {
                    let backup_path = Scratch::Backup.of(&entry.final_path);
                    let moved = match occupant(&backup_path) {
                        Ok(Occupant::Vacant) => fs::rename(&entry.final_path, &backup_path),
                        Ok(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
                        Err(err) => Err(err),
                    };
                    if let Err(err) = moved {
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
                fix_left_behind(&entry.backup_path);
            }
        }
        Ok(committed)
    }

    fn discard_scratch(&self) {
        for entry in &self.entries {
            remove_staged(&entry.tmp_path);
        }
    }
}

/// Remove a scratch file, warning when it is left behind, and say whether it was.
pub fn remove_scratch(tmp_path: &Path) -> bool {
    match fs::remove_file(tmp_path) {
        Ok(()) => false,
        Err(err) if err.kind() == io::ErrorKind::NotFound => false,
        Err(err) => {
            eprintln!(
                "warning: scratch file `{}` was left behind: {err}",
                tmp_path.display(),
            );
            true
        }
    }
}

/// [`remove_scratch`] for a staged file, which refuses the next run while it is left there.
fn remove_staged(tmp_path: &Path) {
    if remove_scratch(tmp_path) {
        fix_left_behind(tmp_path);
    }
}

/// Say that `path`, left behind, has to go before the next run, which refuses to write through it.
fn fix_left_behind(path: &Path) {
    eprintln!(
        "  fix: remove `{}` before running again, since a run refuses to write through a \
         scratch path that holds an entry",
        path.display(),
    );
}

/// Put the directory back the way the commit found it, reporting every step that fails.
fn undo(displaced: &[Displaced], committed: &[PathBuf]) {
    for path in committed {
        if let Err(err) = fs::remove_file(path) {
            eprintln!(
                "error: `{}` was written before a later file failed, and could not be \
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

#[cfg(test)]
mod tests;
