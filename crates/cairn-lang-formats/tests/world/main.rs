//! The Java world a structure file is installed into: its `level.dat`, the structure's stamp, the
//! id it is installed under and how the two versions stand.

#[path = "../common/mod.rs"]
pub mod common;

mod cross_version;
mod level_dat;
mod structure_file;
mod structure_id;

use cairn_lang_nbt::tag::{Compound, List, Tag};
use cairn_lang_nbt::{write_bedrock_uncompressed, write_java_gzip};

/// A Java `level.dat`'s bytes, its root holding `data` as `Data`.
fn java_level_dat(data: Compound) -> Vec<u8> {
    let mut root = Compound::new();
    root.insert("Data", Tag::Compound(data));
    let mut bytes = Vec::new();
    write_java_gzip(&mut bytes, "", &root).expect("write");
    bytes
}

/// The `Data` the game saves at `data_version`, naming its release `name`.
fn saved_data(data_version: i32, name: &str) -> Compound {
    let mut version = Compound::new();
    version
        .insert("Id", Tag::Int(data_version))
        .insert("Name", Tag::String(name.to_owned()))
        .insert("Snapshot", Tag::Byte(0));
    let mut data = Compound::new();
    data.insert("LevelName", Tag::String("New World".to_owned()))
        .insert("DataVersion", Tag::Int(data_version))
        .insert("Version", Tag::Compound(version))
        .insert("Player", Tag::Compound(Compound::new()));
    data
}

/// A Bedrock `level.dat`'s bytes: the 8-byte header, then `root` as little-endian NBT.
fn bedrock_level_dat(root: &Compound) -> Vec<u8> {
    let mut payload = Vec::new();
    write_bedrock_uncompressed(&mut payload, "", root).expect("write");
    let len = u32::try_from(payload.len()).expect("small");
    [&10_i32.to_le_bytes()[..], &len.to_le_bytes(), &payload].concat()
}

/// The root a Bedrock world last opened by `release` saves.
fn bedrock_root(release: [i32; 5]) -> Compound {
    let mut root = Compound::new();
    root.insert("LevelName", Tag::String("Bedrock level".to_owned()))
        .insert("lastOpenedWithVersion", Tag::List(List::of_ints(release)));
    root
}
