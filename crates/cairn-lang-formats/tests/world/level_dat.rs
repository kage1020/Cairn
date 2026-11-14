//! A world folder's `level.dat`: the Java version it records, and each way it fails to be a Java
//! world's.

use cairn_lang_formats::world::{
    LEVEL_DAT, LevelDatError, WorldVersion, read_level_dat, worlds_in,
};
use cairn_lang_nbt::tag::{Compound, Tag};
use tempfile::TempDir;

use crate::{bedrock_level_dat, bedrock_root, java_level_dat, saved_data};

/// A world folder whose `level.dat` holds `bytes`.
fn world_with(bytes: &[u8]) -> TempDir {
    let world = tempfile::tempdir().expect("tempdir");
    std::fs::write(world.path().join(LEVEL_DAT), bytes).expect("write");
    world
}

fn read(bytes: &[u8]) -> Result<WorldVersion, LevelDatError> {
    read_level_dat(world_with(bytes).path())
}

#[test]
fn reads_the_data_version_and_release_name() {
    assert_eq!(
        read(&java_level_dat(saved_data(3953, "1.21"))).expect("read"),
        WorldVersion {
            data_version: 3953,
            name: Some("1.21".to_owned()),
        }
    );
}

#[test]
fn a_data_version_without_a_release_name() {
    let mut data = Compound::new();
    data.insert("DataVersion", Tag::Int(4189))
        .insert("Version", Tag::String("1.21.4".to_owned()));
    assert_eq!(
        read(&java_level_dat(data)).expect("read"),
        WorldVersion {
            data_version: 4189,
            name: None,
        }
    );
}

#[test]
fn a_data_compound_without_an_integer_data_version() {
    let mut as_string = Compound::new();
    as_string.insert("DataVersion", Tag::String("3953".to_owned()));
    for data in [Compound::new(), as_string] {
        let err = read(&java_level_dat(data)).expect_err("refused");
        assert!(matches!(err, LevelDatError::NoDataVersion), "{err:?}");
    }
    let mut root = Compound::new();
    root.insert("Data", Tag::Int(3953));
    let mut bytes = Vec::new();
    cairn_lang_nbt::write_java_gzip(&mut bytes, "", &root).expect("write");
    let err = read(&bytes).expect_err("refused");
    assert!(matches!(err, LevelDatError::NoDataVersion), "{err:?}");
}

#[test]
fn a_folder_without_one() {
    let world = tempfile::tempdir().expect("tempdir");
    let err = read_level_dat(world.path()).expect_err("refused");
    assert!(matches!(err, LevelDatError::Missing), "{err:?}");
    let err = read_level_dat(&world.path().join("absent")).expect_err("refused");
    assert!(matches!(err, LevelDatError::Missing), "{err:?}");
}

#[test]
fn a_file_named_as_the_world() {
    let world = world_with(b"");
    let err = read_level_dat(&world.path().join(LEVEL_DAT)).expect_err("refused");
    assert!(matches!(err, LevelDatError::Missing), "{err:?}");
}

#[test]
fn a_level_dat_that_is_a_folder() {
    let world = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(world.path().join(LEVEL_DAT)).expect("mkdir");
    let err = read_level_dat(world.path()).expect_err("refused");
    assert!(matches!(err, LevelDatError::Io(_)), "{err:?}");
}

#[test]
fn bytes_of_neither_edition() {
    for bytes in [&b""[..], b"level", &[0, 0, 0, 0, 0, 0, 0, 0, 0x0a, 0, 0, 0]] {
        let err = read(bytes).expect_err("refused");
        assert!(matches!(err, LevelDatError::NotGzip), "{bytes:?}: {err:?}");
    }
}

#[test]
fn gzip_that_does_not_decode() {
    let mut cut = java_level_dat(saved_data(3953, "1.21"));
    cut.truncate(cut.len() - 12);
    let err = read(&cut).expect_err("refused");
    assert!(matches!(err, LevelDatError::Nbt(_)), "{err:?}");
}

#[test]
fn a_bedrock_level_dat_names_the_release_that_last_opened_it() {
    let err = read(&bedrock_level_dat(&bedrock_root([1, 21, 60, 2, 0]))).expect_err("refused");
    assert!(
        matches!(&err, LevelDatError::Bedrock { last_opened_with: Some(release) } if release == "1.21.60"),
        "{err:?}"
    );
}

#[test]
fn a_bedrock_level_dat_without_a_release() {
    let mut root = Compound::new();
    root.insert("LevelName", Tag::String("Bedrock level".to_owned()));
    let err = read(&bedrock_level_dat(&root)).expect_err("refused");
    assert!(
        matches!(
            err,
            LevelDatError::Bedrock {
                last_opened_with: None
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_header_whose_length_is_not_the_files() {
    let mut bytes = bedrock_level_dat(&bedrock_root([1, 21, 60, 2, 0]));
    bytes.push(0);
    let err = read(&bytes).expect_err("refused");
    assert!(matches!(err, LevelDatError::NotGzip), "{err:?}");
}

#[test]
fn worlds_in_lists_the_folders_holding_a_level_dat() {
    let saves = tempfile::tempdir().expect("tempdir");
    for (folder, has_level_dat) in [("Zeta", true), ("alpha", true), ("screenshots", false)] {
        let path = saves.path().join(folder);
        std::fs::create_dir(&path).expect("mkdir");
        if has_level_dat {
            std::fs::write(path.join(LEVEL_DAT), b"").expect("write");
        }
    }
    std::fs::write(saves.path().join(LEVEL_DAT), b"").expect("write");
    assert_eq!(worlds_in(saves.path()), ["Zeta", "alpha"]);
    assert_eq!(
        worlds_in(&saves.path().join("absent")),
        Vec::<String>::new()
    );
}
