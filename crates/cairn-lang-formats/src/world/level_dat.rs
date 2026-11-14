//! The version a Java world's `level.dat` records, with a Bedrock world's told apart from it.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use cairn_lang_nbt::tag::Tag;
use cairn_lang_nbt::{NbtReadError, ReadOptions, read_bedrock_uncompressed, read_java_gzip};
use thiserror::Error;

/// The file in a world's folder that records the world, on either edition.
pub const LEVEL_DAT: &str = "level.dat";

/// The most a `level.dat`'s `Data` compound may build, the quota the game reads one under.
const LEVEL_DAT_TREE_BYTES: usize = 100 * 1024 * 1024;

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
    Io(io::Error),
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
    Nbt(NbtReadError),
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
    let options = ReadOptions {
        keep: Some(&["Data"]),
        max_tree_bytes: LEVEL_DAT_TREE_BYTES,
    };
    let root = read_java_gzip(gzip, &options).map_err(LevelDatError::Nbt)?;
    let Some(Tag::Compound(data)) = root.compound.entries.get("Data") else {
        return Err(LevelDatError::NoDataVersion);
    };
    let Some(&Tag::Int(data_version)) = data.entries.get("DataVersion") else {
        return Err(LevelDatError::NoDataVersion);
    };
    let name = match data.entries.get("Version") {
        Some(Tag::Compound(version)) => match version.entries.get("Name") {
            Some(Tag::String(name)) => Some(name.clone()),
            _ => None,
        },
        _ => None,
    };
    Ok(WorldVersion { data_version, name })
}

/// [`LevelDatError::Bedrock`] for a `level.dat` whose first bytes are `head`, `None` when the
/// file is not one: its header gives the length of the NBT after it, and that NBT decodes.
fn bedrock(head: &[u8], file_len: u64, payload: impl Read) -> Option<LevelDatError> {
    let declared = head.get(4..8)?;
    let declared = u32::from_le_bytes(declared.try_into().ok()?);
    if u64::from(declared) != file_len.checked_sub(8)? {
        return None;
    }
    let options = ReadOptions {
        keep: Some(&["lastOpenedWithVersion"]),
        max_tree_bytes: LEVEL_DAT_TREE_BYTES,
    };
    let root = read_bedrock_uncompressed(payload, &options).ok()?;
    let last_opened_with = match root.compound.entries.get("lastOpenedWithVersion") {
        Some(Tag::List(list)) => {
            let parts: Option<Vec<String>> = list
                .items
                .iter()
                .take(3)
                .map(|part| match part {
                    Tag::Int(part) => Some(part.to_string()),
                    _ => None,
                })
                .collect();
            parts
                .filter(|parts| !parts.is_empty())
                .map(|parts| parts.join("."))
        }
        _ => None,
    };
    Some(LevelDatError::Bedrock { last_opened_with })
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
