//! The version a Java world's `level.dat` records, with a Bedrock world's told apart from it.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use cairn_lang_nbt::tag::Tag;
use cairn_lang_nbt::{Event, NbtReadError, NbtReader};
use thiserror::Error;

use super::entries::{self, Body, for_each_entry, open_root};

/// The file in a world's folder that records the world, on either edition.
pub const LEVEL_DAT: &str = "level.dat";

/// The two bytes every gzip stream opens with.
pub(super) const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// The version a Java world was last saved by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldVersion {
    /// `Data.DataVersion`.
    pub data_version: i32,
    /// `Data.Version.Name`, such as `1.21.4`, where the file carries one.
    pub name: Option<String>,
}

/// Why a world folder's `level.dat` gives no Java version.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LevelDatError {
    /// The folder holds no `level.dat`.
    #[error("holds no `level.dat`")]
    Missing,
    /// `level.dat` is there but cannot be read.
    #[error("has a `level.dat` that cannot be read: {0}")]
    Io(#[source] io::Error),
    /// A Bedrock world's: an 8-byte header ahead of little-endian NBT.
    #[error("is a Bedrock world")]
    Bedrock {
        /// `lastOpenedWithVersion`, as `1.21.60`, where the file carries it.
        last_opened_with: Option<String>,
    },
    /// Neither edition's: not gzip, and not Bedrock's header.
    #[error("has a `level.dat` that is not gzip-compressed")]
    NotGzip,
    /// Gzip whose NBT does not decode, or whose gzip stream does not.
    #[error("has a `level.dat` that does not decode as NBT: {0}")]
    Nbt(#[source] NbtReadError),
    /// No integer `DataVersion` in a `Data` compound.
    #[error("has a `level.dat` with no integer `Data.DataVersion`")]
    NoDataVersion,
}

/// The version recorded by the `level.dat` in the world folder `world`.
///
/// # Errors
///
/// [`LevelDatError`], naming which of its ways the file fails to be a Java world's.
pub fn read_level_dat(world: &Path) -> Result<WorldVersion, LevelDatError> {
    let mut file = File::open(world.join(LEVEL_DAT)).map_err(|err| match err.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => LevelDatError::Missing,
        _ => LevelDatError::Io(err),
    })?;
    let file_len = file.metadata().map_err(LevelDatError::Io)?.len();
    let mut head = Vec::with_capacity(8);
    (&mut file)
        .take(8)
        .read_to_end(&mut head)
        .map_err(LevelDatError::Io)?;
    if head.starts_with(&GZIP_MAGIC) {
        return read_java(head.as_slice().chain(file));
    }
    Err(bedrock(&head, file_len, file).unwrap_or(LevelDatError::NotGzip))
}

fn read_java(gzip: impl Read) -> Result<WorldVersion, LevelDatError> {
    let mut nbt = entries::java_gzip(gzip);
    let mut saved = None;
    open_root(&mut nbt)
        .and_then(|()| {
            for_each_entry(&mut nbt, |nbt, entry| {
                // The game keeps the last of two entries under one key.
                if entry.name == "Data" {
                    saved = match entry.body {
                        Body::Compound => saved_version(nbt)?,
                        Body::List | Body::Value(_) => None,
                    };
                }
                Ok(())
            })
        })
        // The gzip trailer is read past the root, and its checksum is what finds a corrupted body.
        .and_then(|()| nbt.finish().map(drop))
        .map_err(undecoded)?;
    saved.ok_or(LevelDatError::NoDataVersion)
}

/// [`LevelDatError::Nbt`] for bytes that do not decode, [`LevelDatError::Io`] for a failed read.
fn undecoded(err: NbtReadError) -> LevelDatError {
    match err {
        NbtReadError::Io(err)
            if !matches!(
                err.kind(),
                io::ErrorKind::InvalidInput
                    | io::ErrorKind::InvalidData
                    | io::ErrorKind::UnexpectedEof
            ) =>
        {
            LevelDatError::Io(err)
        }
        err => LevelDatError::Nbt(err),
    }
}

/// The version the `Data` compound `nbt` is in records, `None` without an integer `DataVersion`.
fn saved_version<R: Read>(nbt: &mut NbtReader<R>) -> Result<Option<WorldVersion>, NbtReadError> {
    let mut data_version = None;
    let mut name = None;
    for_each_entry(nbt, |nbt, entry| {
        match (entry.name.as_str(), entry.body) {
            ("DataVersion", Body::Value(Tag::Int(version))) => data_version = Some(version),
            ("DataVersion", _) => data_version = None,
            ("Version", Body::Compound) => name = release_name(nbt)?,
            ("Version", _) => name = None,
            _ => {}
        }
        Ok(())
    })?;
    Ok(data_version.map(|data_version| WorldVersion { data_version, name }))
}

/// The string `Name` of the `Version` compound `nbt` is in.
fn release_name<R: Read>(nbt: &mut NbtReader<R>) -> Result<Option<String>, NbtReadError> {
    let mut name = None;
    for_each_entry(nbt, |_, entry| {
        if entry.name == "Name" {
            name = match entry.body {
                Body::Value(Tag::String(release)) => Some(release),
                _ => None,
            };
        }
        Ok(())
    })?;
    Ok(name)
}

/// [`LevelDatError::Bedrock`] for a `level.dat` whose first bytes are `head`, `None` when the
/// file is not one: a Bedrock header gives the length of the NBT after it.
fn bedrock(head: &[u8], file_len: u64, payload: impl Read) -> Option<LevelDatError> {
    let declared = head.get(4..8)?;
    let declared = u32::from_le_bytes(declared.try_into().ok()?);
    if u64::from(declared) != file_len.checked_sub(8)? {
        return None;
    }
    let mut nbt = entries::bedrock(payload);
    let last_opened_with = last_opened_with(&mut nbt).ok().flatten();
    Some(LevelDatError::Bedrock { last_opened_with })
}

/// The release a Bedrock `level.dat`'s `lastOpenedWithVersion` names, as `1.21.60`, from the
/// first three of its integers.
fn last_opened_with<R: Read>(nbt: &mut NbtReader<R>) -> Result<Option<String>, NbtReadError> {
    open_root(nbt)?;
    let mut release = None;
    for_each_entry(nbt, |nbt, entry| {
        if entry.name == "lastOpenedWithVersion" {
            release = match entry.body {
                Body::List => release_parts(nbt)?,
                Body::Compound | Body::Value(_) => None,
            };
        }
        Ok(())
    })?;
    Ok(release)
}

/// The first three items of the list `nbt` is in, joined with `.`, `None` when one of them is
/// not an integer or the list is empty.
fn release_parts<R: Read>(nbt: &mut NbtReader<R>) -> Result<Option<String>, NbtReadError> {
    let mut parts = Vec::new();
    while parts.len() < 3 {
        match nbt.next_event()? {
            Some(Event::Value {
                tag: Tag::Int(part),
                ..
            }) => parts.push(part.to_string()),
            Some(Event::End) | None => break,
            Some(_) => return Ok(None),
        }
    }
    Ok((!parts.is_empty()).then(|| parts.join(".")))
}

/// The names of the folders in `dir` that hold a `level.dat`, sorted; none when `dir` cannot be
/// read.
#[must_use]
pub fn worlds_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut worlds: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join(LEVEL_DAT).is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    worlds.sort();
    worlds
}
