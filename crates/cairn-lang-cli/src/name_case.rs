//! Whether a directory takes two file names that differ only in letter case for one entry, asked
//! of the directory itself.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write as _};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::staging::remove_scratch;

/// How a directory compares file names that differ only in letter case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameCase {
    /// `hut.nbt` and `HUT.nbt` are two entries.
    Sensitive,
    /// `hut.nbt` and `HUT.nbt` are one entry.
    Insensitive,
}

impl NameCase {
    /// How `dir` compares names, probed in `dir`, or `Insensitive` when the probe fails.
    pub(crate) fn of_dir(dir: &Path) -> Self {
        // A wrong `Insensitive` only refuses a build; a wrong `Sensitive` loses an artifact.
        Self::probe(dir).unwrap_or(Self::Insensitive)
    }

    /// Write a scratch file into `dir`, read it back under its upper-case spelling, and remove it.
    ///
    /// # Errors
    ///
    /// The I/O failure writing the scratch file, or reading it back other than as `NotFound`.
    fn probe(dir: &Path) -> io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let name = format!(".cairn-case-probe-{}-{nanos}", std::process::id());
        let path = dir.join(&name);
        let written = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .write_all(name.as_bytes());
        // The body tells this file apart from another entry already spelled the other way.
        let answer = written.and_then(|()| match fs::read(dir.join(name.to_ascii_uppercase())) {
            Ok(body) if body == name.as_bytes() => Ok(Self::Insensitive),
            Ok(_) => Ok(Self::Sensitive),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Self::Sensitive),
            Err(err) => Err(err),
        });
        remove_scratch(&path);
        answer
    }

    /// `name` as a key, lowercased where names fold, so `hut.nbt` and `HUT.nbt` share one.
    pub(crate) fn key(self, name: &OsStr) -> OsString {
        match self {
            Self::Sensitive => name.to_os_string(),
            Self::Insensitive => name.to_string_lossy().to_lowercase().into(),
        }
    }
}

#[cfg(test)]
mod tests;
