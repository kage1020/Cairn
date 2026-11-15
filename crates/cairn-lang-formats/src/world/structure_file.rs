//! The `DataVersion` a Java structure file is stamped with, read without building its blocks.

use std::io::Read;

use cairn_lang_nbt::tag::Tag;
use cairn_lang_nbt::{NbtReadError, NbtReader};
use thiserror::Error;

use super::entries::{self, Body, for_each_entry, open_root};
use super::level_dat::GZIP_MAGIC;

/// Why bytes give no Java structure's `DataVersion`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StructureFileError {
    /// A Bedrock `.mcstructure`: uncompressed little-endian NBT with `format_version` and
    /// `structure`.
    #[error("is a Bedrock `.mcstructure`")]
    Bedrock,
    /// Not gzip, and not a `.mcstructure`.
    #[error("is not gzip-compressed")]
    NotGzip,
    /// Gzip whose NBT does not decode, or whose gzip stream does not.
    #[error("does not decode as NBT: {0}")]
    Nbt(NbtReadError),
    /// NBT whose root lacks a list a structure has.
    #[error("is not a structure file: its root has no {}", .missing.join(", "))]
    NotAStructure {
        /// The lists missing, of `size`, `blocks` and `palette` or `palettes`.
        missing: Vec<&'static str>,
    },
    /// A structure with no integer `DataVersion` at its root.
    #[error("has no integer `DataVersion`")]
    NoDataVersion,
}

/// The `DataVersion` at the root of the Java structure file `bytes`.
///
/// # Errors
///
/// [`StructureFileError`], naming which of its ways `bytes` fails to be a stamped structure.
pub fn read_structure_data_version(bytes: &[u8]) -> Result<i32, StructureFileError> {
    if !bytes.starts_with(&GZIP_MAGIC) {
        return Err(if is_mcstructure(bytes) {
            StructureFileError::Bedrock
        } else {
            StructureFileError::NotGzip
        });
    }
    let stamp = read_stamp(&mut entries::java_gzip(bytes)).map_err(StructureFileError::Nbt)?;
    let [size, blocks, palette, palettes] = stamp.lists;
    let mut missing = Vec::new();
    if !size {
        missing.push("`size`");
    }
    if !blocks {
        missing.push("`blocks`");
    }
    if !palette && !palettes {
        missing.push("`palette`");
    }
    if !missing.is_empty() {
        return Err(StructureFileError::NotAStructure { missing });
    }
    stamp.data_version.ok_or(StructureFileError::NoDataVersion)
}

/// The keys of the root lists a structure has: `size`, `blocks`, and `palette` or `palettes`.
const LIST_KEYS: [&str; 4] = ["size", "blocks", "palette", "palettes"];

/// What a structure file's root says of it, each entry the last under its key as the game keeps.
#[derive(Debug, Default)]
struct Stamp {
    /// An integer `DataVersion`.
    data_version: Option<i32>,
    /// Whether the entry under each of [`LIST_KEYS`] is a list.
    lists: [bool; 4],
}

fn read_stamp<R: Read>(nbt: &mut NbtReader<R>) -> Result<Stamp, NbtReadError> {
    open_root(nbt)?;
    let mut stamp = Stamp::default();
    for_each_entry(nbt, |_, entry| {
        if entry.name == "DataVersion" {
            stamp.data_version = match entry.body {
                Body::Value(Tag::Int(version)) => Some(version),
                _ => None,
            };
        } else if let Some(at) = LIST_KEYS.iter().position(|key| *key == entry.name) {
            stamp.lists[at] = matches!(entry.body, Body::List);
        }
        Ok(())
    })?;
    Ok(stamp)
}

fn is_mcstructure(bytes: &[u8]) -> bool {
    let mut nbt = entries::bedrock(bytes);
    let (mut format_version, mut structure) = (false, false);
    let read = open_root(&mut nbt).and_then(|()| {
        for_each_entry(&mut nbt, |_, entry| {
            format_version |= entry.name == "format_version";
            structure |= entry.name == "structure";
            Ok(())
        })
    });
    read.is_ok() && format_version && structure
}
