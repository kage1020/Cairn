//! Staging a set of output files and committing them together.
//!
//! A module of its own so that privacy, not convention, enforces what
//! makes a commit recoverable: a staged file is only reachable through
//! the scratch path chosen here.

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

/// Which half of the commit failed.
///
/// The two call for different fixes at different paths: a stale `X.bak`
/// directory blocks the displace step, and a message naming `X` would
/// send the operator after the wrong file.
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
    /// The path the failing rename touched — the backup for a displace,
    /// the destination for a place.
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
    /// Where it belongs once every file in the set has been written.
    final_path: PathBuf,
    /// Where it is now.
    tmp_path: PathBuf,
}

/// A destination that already held something, moved aside for the
/// duration of the commit so a failure part-way can put it back.
struct Displaced {
    final_path: PathBuf,
    backup_path: PathBuf,
}

/// Output files written but not yet in place.
#[derive(Default)]
pub struct StagedSet {
    entries: Vec<Staged>,
}

/// `path` with `suffix` appended to the whole file name.
///
/// Not `Path::with_extension`, which replaces the extension: a lockfile at
/// `village.crn.lock` would stage to `village.crn.tmp`, colliding with any
/// other `village.crn.*` scratch.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut raw = path.as_os_str().to_owned();
    raw.push(suffix);
    PathBuf::from(raw)
}

/// Every path this set will create or rename, staging scratch included.
///
/// Callers check these against each other before staging starts: two
/// entries sharing a destination would stage over each other's bytes and
/// then fight over the same backup during the commit.
pub fn reserved_paths(final_path: &Path) -> [PathBuf; 3] {
    [
        final_path.to_path_buf(),
        with_suffix(final_path, ".tmp"),
        with_suffix(final_path, ".bak"),
    ]
}

/// What is sitting at a destination right now.
enum Occupant {
    /// Nothing — the commit will create the file.
    Vacant,
    /// A file or symlink, which `rename` can move aside and put back.
    Movable,
    /// A directory, which `rename` cannot replace with a file. It is not
    /// backed up, so the place step fails naming it.
    Directory,
}

/// `symlink_metadata` rather than `Path::is_file`, which folds every error
/// into `false`, meaning "no backup needed": a transient stat failure
/// would let the commit overwrite a file it could not restore. An
/// unreadable destination is a hard error instead.
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
    /// Propagates the I/O failure. The partial scratch file is removed
    /// before returning: a failed entry is never added to the set, so
    /// [`Self::discard`] has no way to reach it afterwards.
    pub fn stage(
        &mut self,
        final_path: &Path,
        kind: Kind,
        write: impl FnOnce(&mut io::BufWriter<fs::File>) -> io::Result<()>,
    ) -> io::Result<()> {
        use std::io::Write as _;

        let tmp_path = with_suffix(final_path, ".tmp");
        let result = (|| {
            // Buffered, because an uncompressed `.mcstructure` is two
            // 4-byte writes per voxel and each would otherwise be its
            // own system call.
            let mut file = io::BufWriter::new(fs::File::create(&tmp_path)?);
            write(&mut file)?;
            // Flushed first, so the buffer's tail is in the file that
            // `sync_all` then makes durable.
            file.flush()?;
            // The commit's rename is atomic only for the directory entry;
            // unsynced, a crash can leave a correctly named, half-written
            // file.
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

    /// Move every staged file into place, or leave the directory as it
    /// was, and return the paths written.
    ///
    /// A rename consumes whatever is at the destination, so each one moves
    /// that file aside first, and the backups are deleted only once the
    /// whole set has landed: deleting eagerly would let a failure on the
    /// last file take the previous build with it.
    ///
    /// Consuming `self` so the set cannot be committed twice, nor
    /// discarded after its scratch files have been renamed away.
    ///
    /// # Errors
    ///
    /// Returns the first rename that failed, after undoing the ones
    /// before it.
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
                // Not a failure, but nothing will come back for a copy of
                // the previous build under a scratch-looking name.
                eprintln!(
                    "warning: `{}` was left behind after replacing `{}`: {err}",
                    entry.backup_path.display(),
                    entry.final_path.display(),
                );
            }
        }
        Ok(committed)
    }

    /// `discard` by reference, for the undo paths that still need the
    /// entry list afterwards.
    fn discard_scratch(&self) {
        for entry in &self.entries {
            remove_scratch(&entry.tmp_path);
        }
    }
}

/// Delete a scratch file the build is abandoning, warning when one is
/// left behind. A file that was never created is not a leftover.
fn remove_scratch(tmp_path: &Path) {
    match fs::remove_file(tmp_path) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => eprintln!(
            "warning: scratch file `{}` was left behind: {err}",
            tmp_path.display(),
        ),
    }
}

/// Put the directory back the way the commit found it.
///
/// Every failed step is reported rather than swallowed. The delete loop
/// has no later step to surface its failure: a created rather than
/// replaced entry is in `committed` but never in `displaced`, so nothing
/// touches its path again. On Windows deleting a file another process
/// holds open fails, while renaming onto a vacant name succeeds.
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
