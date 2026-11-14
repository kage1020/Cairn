//! The `DataVersion` a Java structure file is stamped with, read without building its blocks.

use cairn_lang_nbt::tag::Tag;
use cairn_lang_nbt::{
    NbtReadError, NbtRoot, ReadOptions, read_bedrock_uncompressed, read_java_gzip,
};
use thiserror::Error;

use super::level_dat::GZIP_MAGIC;

/// The most the kept `DataVersion` and the skipped entries' names may build.
const STAMP_TREE_BYTES: usize = 1 << 16;

/// The tag id of a list.
const LIST: u8 = 9;

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
    let options = ReadOptions {
        keep: Some(&["DataVersion"]),
        max_tree_bytes: STAMP_TREE_BYTES,
    };
    let root = read_java_gzip(bytes, &options).map_err(StructureFileError::Nbt)?;
    let has_list = |name: &str| {
        root.skipped
            .iter()
            .any(|(key, id)| key == name && *id == LIST)
    };
    let mut missing = Vec::new();
    if !has_list("size") {
        missing.push("`size`");
    }
    if !has_list("blocks") {
        missing.push("`blocks`");
    }
    if !has_list("palette") && !has_list("palettes") {
        missing.push("`palette`");
    }
    if !missing.is_empty() {
        return Err(StructureFileError::NotAStructure { missing });
    }
    match root.compound.entries.get("DataVersion") {
        Some(&Tag::Int(data_version)) => Ok(data_version),
        _ => Err(StructureFileError::NoDataVersion),
    }
}

fn is_mcstructure(bytes: &[u8]) -> bool {
    let options = ReadOptions {
        keep: Some(&[]),
        max_tree_bytes: STAMP_TREE_BYTES,
    };
    read_bedrock_uncompressed(bytes, &options).is_ok_and(|root: NbtRoot| {
        ["format_version", "structure"]
            .iter()
            .all(|name| root.skipped.iter().any(|(key, _)| key == name))
    })
}
