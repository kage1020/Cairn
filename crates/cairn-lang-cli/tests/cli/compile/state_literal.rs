//! A canonical token's block-state literal, `@oak_log[axis=x]`, end to end: Java writes it,
//! `info` counts what Bedrock cannot map, and Bedrock refuses it.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::common::{
    assert_stream, bytes_contain, cairn, compile, nbt_body, stderr_of, write_source,
};

/// A one-theme, one-struct source binding `target` to a floor.
fn floored(dir: &Path, target: &str) -> PathBuf {
    write_source(
        dir,
        "s.crn",
        &format!("theme t:\n  slot f -> {target}\n\nstruct s size=3x3\n  floor mat_slot=f\n"),
    )
}

/// The NBT bytes of a `String` tag named `key` holding `value`.
fn string_tag(key: &str, value: &str) -> Vec<u8> {
    let mut tag = vec![8];
    for part in [key, value] {
        tag.extend(u16::try_from(part.len()).expect("short").to_be_bytes());
        tag.extend(part.as_bytes());
    }
    tag
}

#[test]
fn bedrock_refuses_to_compile_the_entry_info_counts_unsupported() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@oak_log[axis=x]");

    let info = cairn(
        "info",
        &[src.to_str().unwrap(), "--editions", "java,bedrock"],
    );
    let stdout = String::from_utf8_lossy(&info.stdout);
    assert_eq!(info.status.code(), Some(0), "{}", stderr_of(&info));
    assert!(stdout.contains("unsupported: 1"), "{stdout}");

    let out = TempDir::new().expect("out tempdir");
    let compiled = compile(&src, "bedrock", "1.21.60", out.path(), None);
    let stderr = stderr_of(&compiled);
    assert_eq!(compiled.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("`minecraft:oak_log[axis=x]`")
            && stderr.contains("carries blockstate properties the Bedrock backend cannot map"),
        "{stderr}",
    );
    assert!(
        fs::read_dir(out.path())
            .expect("out dir readable")
            .next()
            .is_none(),
        "no artifact may be written: {stderr}",
    );
}

#[test]
fn java_writes_an_unchecked_literal_as_given_and_says_so() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@oak_log[axis=q]");
    let out = TempDir::new().expect("out tempdir");
    let compiled = compile(&src, "java", "1.21.4", out.path(), None);
    let stderr = stderr_of(&compiled);
    assert_eq!(compiled.status.code(), Some(0), "{stderr}");
    assert_stream(&stderr, &[(2, 13, "warning", "W_STATE_LITERAL_UNCHECKED")]);
    assert!(
        stderr.contains("`minecraft:oak_log` is written with `axis=q`"),
        "{stderr}",
    );
    let body = nbt_body(&out.path().join("s.nbt"));
    assert!(
        bytes_contain(&body, string_tag("axis", "q")),
        "the Properties compound carries `axis=q`",
    );
}

#[test]
fn two_spellings_of_one_state_write_the_same_bytes() {
    let mut bodies = Vec::new();
    for target in [
        "@oak_stairs[half=top,facing=north]",
        "@oak_stairs[facing=north, half=top]",
    ] {
        let tmp = TempDir::new().expect("tempdir");
        let src = floored(tmp.path(), target);
        let out = TempDir::new().expect("out tempdir");
        let compiled = compile(&src, "java", "1.21.4", out.path(), None);
        assert_eq!(
            compiled.status.code(),
            Some(0),
            "{target}: {}",
            stderr_of(&compiled)
        );
        bodies.push(nbt_body(&out.path().join("s.nbt")));
    }
    assert!(
        bodies[0] == bodies[1],
        "the two spellings wrote different bytes"
    );
}

#[test]
fn a_well_formed_literal_passes_check() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@oak_stairs[half=top, facing=north]");

    let plain = cairn("check", &[src.to_str().unwrap()]);
    let stderr = stderr_of(&plain);
    assert_eq!(plain.status.code(), Some(0), "{stderr}");
    assert_stream(&stderr, &[]);

    let pinned = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ],
    );
    let stderr = stderr_of(&pinned);
    assert_eq!(pinned.status.code(), Some(0), "{stderr}");
    assert_stream(&stderr, &[(2, 13, "warning", "W_STATE_LITERAL_UNCHECKED")]);
}
