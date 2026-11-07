//! A canonical token's block-state literal end to end: Java writes `@oak_log[axis=q]` as given,
//! and Bedrock writes `@cherry_wood[axis=x]` at each version whose state map carries it and
//! refuses it at the others.

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
fn bedrock_builds_a_state_only_at_the_versions_whose_map_carries_it() {
    let tmp = TempDir::new().expect("tempdir");
    // 1.21.40's palette lacks the states GeyserMC maps a cherry wood to; the others carry them.
    let src = floored(tmp.path(), "@cherry_wood[axis=x]");

    let path = src.to_str().unwrap();
    let info = cairn("info", &[path, "--editions", "bedrock"]);
    let stdout = String::from_utf8_lossy(&info.stdout);
    assert_eq!(info.status.code(), Some(0), "{}", stderr_of(&info));
    let row = |label: &str| stdout.lines().find(|line| line.starts_with(label));
    assert_eq!(
        row("edition portability:"),
        Some("edition portability:     Bedrock: portable: 1  degraded: 0  unsupported: 0"),
    );
    assert_eq!(
        row("buildable targets:"),
        Some("buildable targets:       Bedrock: 1.21.0, 1.21.60, 1.26.60 (1.21.40 refuses)"),
    );
    let refusal = stderr_of(&info)
        .split_once("note: bedrock 1.21.40 refuses this source\n")
        .map(|(_, refusal)| refusal.to_owned());
    assert_eq!(
        refusal,
        Some(format!(
            "{path}:5:3: error[E_STATE_UNMAPPED]: `minecraft:cherry_wood[axis=x]` has no form at \
             bedrock 1.21.40: its palette does not declare the states GeyserMC maps \
             `minecraft:cherry_wood` to, so the pack carries no form for them\n  \
             note: pass `--target` 1.21.0, 1.21.60 or 1.26.60, whose state maps carry these \
             states, bind the material to a block bedrock 1.21.40 has a form for, or build for \
             the other edition\n"
        )),
    );
    let json = cairn("info", &[path, "--editions", "bedrock", "--format", "json"]);
    let axes: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("info prints one JSON document");
    assert_eq!(
        axes["buildable_targets"],
        serde_json::json!([{
            "edition": "bedrock",
            "buildable": ["1.21.0", "1.21.60", "1.26.60"],
            "considered": ["1.21.0", "1.21.40", "1.21.60", "1.26.60"],
        }]),
    );

    let out = TempDir::new().expect("out tempdir");
    let compiled = compile(&src, "bedrock", "1.21.40", out.path(), None);
    let stderr = stderr_of(&compiled);
    assert_eq!(compiled.status.code(), Some(1), "{stderr}");
    assert_stream(
        &stderr,
        &[
            (2, 13, "warning", "W_STATE_LITERAL_UNCHECKED"),
            (5, 3, "error", "E_STATE_UNMAPPED"),
        ],
    );
    assert!(
        stderr.contains(
            "`minecraft:cherry_wood[axis=x]` has no form at bedrock 1.21.40: its palette does not \
             declare the states GeyserMC maps `minecraft:cherry_wood` to, so the pack carries no \
             form for them"
        ),
        "{stderr}",
    );
    assert!(
        fs::read_dir(out.path())
            .expect("out dir readable")
            .next()
            .is_none(),
        "no artifact may be written: {stderr}",
    );

    let out = TempDir::new().expect("out tempdir");
    let compiled = compile(&src, "bedrock", "1.21.60", out.path(), None);
    let stderr = stderr_of(&compiled);
    assert_eq!(compiled.status.code(), Some(0), "{stderr}");
    assert!(out.path().join("s.mcstructure").exists(), "{stderr}");
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
    let src = src.to_str().unwrap();

    // The warning needs no target, so a check with none reports it as a pinned one does.
    for args in [
        vec![src],
        vec![src, "--edition", "java", "--target", "1.21.4"],
    ] {
        let out = cairn("check", &args);
        let stderr = stderr_of(&out);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {stderr}");
        assert_stream(&stderr, &[(2, 13, "warning", "W_STATE_LITERAL_UNCHECKED")]);
    }
}
