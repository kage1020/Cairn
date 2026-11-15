//! The `DataVersion` a structure file is stamped with, and the files that carry none.

use std::io::Write;

use cairn_lang_core::Edition;
use cairn_lang_formats::bedrock_structure::{build_mcstructure_tag, write_mcstructure};
use cairn_lang_formats::data_version::{resolve_bedrock_target, resolve_java_target};
use cairn_lang_formats::java_structure::{build_structure_tag, write_compound_gzip};
use cairn_lang_formats::world::{StructureFileError, read_structure_data_version};
use cairn_lang_nbt::NbtReadError;
use cairn_lang_nbt::tag::{Compound, List, Tag};

use crate::common::{lower_example, states_for};
use crate::{java_gzip_entries, java_level_dat, saved_data};

/// The cottage example's structure at the Java `mc_version`.
fn cottage(mc_version: &str) -> Compound {
    let ir = lower_example("cottage.crn", Edition::Java, Some(mc_version));
    let target = resolve_java_target(mc_version).expect("targetable");
    build_structure_tag(&ir.structures["struct::cottage"], &target).expect("build")
}

fn gzip(root: &Compound) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_compound_gzip(&mut bytes, root).expect("write");
    bytes
}

#[test]
fn reads_the_stamp_the_writer_writes() {
    for (mc_version, data_version) in [("1.20.4", 3700), ("1.21.4", 4189), ("26.3", 5023)] {
        assert_eq!(
            read_structure_data_version(&gzip(&cottage(mc_version))).expect("read"),
            data_version,
            "{mc_version}"
        );
    }
}

#[test]
fn a_structure_with_a_palettes_list() {
    let mut root = cottage("1.21.4");
    let palette = root.entries.shift_remove("palette").expect("palette");
    root.insert("palettes", Tag::List(List::of_tags(9, vec![palette])));
    assert_eq!(
        read_structure_data_version(&gzip(&root)).expect("read"),
        4189
    );
}

#[test]
fn a_structure_without_an_integer_data_version() {
    let mut unstamped = cottage("1.21.4");
    unstamped.entries.shift_remove("DataVersion");
    let mut as_string = cottage("1.21.4");
    as_string.insert("DataVersion", Tag::String("4189".to_owned()));
    for root in [unstamped, as_string] {
        let err = read_structure_data_version(&gzip(&root)).expect_err("refused");
        assert!(matches!(err, StructureFileError::NoDataVersion), "{err:?}");
    }
}

#[test]
fn gzip_nbt_that_is_not_a_structure_names_the_lists_it_lacks() {
    let err = read_structure_data_version(&java_level_dat(saved_data(3953, "1.21")))
        .expect_err("refused");
    assert!(
        matches!(&err, StructureFileError::NotAStructure { missing } if missing == &["`size`", "`blocks`", "`palette`"]),
        "{err:?}"
    );
    let mut root = cottage("1.21.4");
    root.insert("blocks", Tag::Int(0));
    let err = read_structure_data_version(&gzip(&root)).expect_err("refused");
    assert!(
        matches!(&err, StructureFileError::NotAStructure { missing } if missing == &["`blocks`"]),
        "{err:?}"
    );
}

#[test]
fn a_bedrock_mcstructure() {
    let target = resolve_bedrock_target("1.21.60").expect("targetable");
    let ir = lower_example("cottage.crn", Edition::Bedrock, Some("1.21.60"));
    let (root, _) = build_mcstructure_tag(
        &ir.structures["struct::cottage"],
        &target,
        &states_for(&target),
    )
    .expect("build");
    let mut bytes = Vec::new();
    write_mcstructure(&mut bytes, &root).expect("write");
    let err = read_structure_data_version(&bytes).expect_err("refused");
    assert!(matches!(err, StructureFileError::Bedrock), "{err:?}");
}

#[test]
fn bytes_that_are_not_gzip() {
    for bytes in [&b""[..], b"{\"size\": [1, 1, 1]}", &[0x0a, 0, 0, 0]] {
        let err = read_structure_data_version(bytes).expect_err("refused");
        assert!(
            matches!(err, StructureFileError::NotGzip),
            "{bytes:?}: {err:?}"
        );
    }
}

#[test]
fn gzip_that_does_not_decode() {
    let mut cut = gzip(&cottage("1.21.4"));
    cut.truncate(cut.len() / 2);
    let err = read_structure_data_version(&cut).expect_err("refused");
    assert!(matches!(err, StructureFileError::Nbt(_)), "{err:?}");
}

#[test]
fn a_key_written_twice_is_read_as_its_last_entry() {
    let root = cottage("1.21.4");
    let mut entries: Vec<(&str, Tag)> = root
        .entries
        .iter()
        .map(|(name, tag)| (name.as_str(), tag.clone()))
        .collect();
    entries.push(("DataVersion", Tag::Int(3700)));
    assert_eq!(
        read_structure_data_version(&java_gzip_entries(&entries)).expect("read"),
        3700
    );
    entries.push(("blocks", Tag::Int(0)));
    let err = read_structure_data_version(&java_gzip_entries(&entries)).expect_err("refused");
    assert!(
        matches!(&err, StructureFileError::NotAStructure { missing } if *missing == ["`blocks`"]),
        "{err:?}"
    );
}

#[test]
fn an_array_longer_than_any_structure_holds_is_refused_before_its_elements_are_read() {
    // A root holding one long array, `junk`, that declares 2^24 + 1 elements and carries none.
    let mut nbt = vec![10, 0, 0, 12, 0, 4];
    nbt.extend_from_slice(b"junk");
    nbt.extend_from_slice(&((1_i32 << 24) + 1).to_be_bytes());
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&nbt).expect("write");
    let err = read_structure_data_version(&gz.finish().expect("finish")).expect_err("refused");
    assert!(
        matches!(
            err,
            StructureFileError::Nbt(NbtReadError::TooLong {
                context: "long_array",
                len: 16_777_217,
                limit: 16_777_216,
                offset: 10,
            })
        ),
        "{err:?}"
    );
}
