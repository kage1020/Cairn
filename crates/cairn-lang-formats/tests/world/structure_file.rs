//! The `DataVersion` a structure file is stamped with, and the files that carry none.

use cairn_lang_core::Edition;
use cairn_lang_formats::bedrock_structure::{build_mcstructure_tag, write_mcstructure};
use cairn_lang_formats::data_version::{resolve_bedrock_target, resolve_java_target};
use cairn_lang_formats::java_structure::{build_structure_tag, write_compound_gzip};
use cairn_lang_formats::world::{StructureFileError, read_structure_data_version};
use cairn_lang_nbt::tag::{Compound, List, Tag};

use crate::common::{lower_example, states_for};
use crate::{java_level_dat, saved_data};

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
