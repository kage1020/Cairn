//! A canonical token's block-state literal end to end: a pinned target refuses a pair outside its
//! block's states (`@oak_log[axis=q]`), Java writes one its block takes (`@oak_log[axis=x]`), and
//! Bedrock writes `@cherry_wood[axis=x]` at each version whose state map carries it and refuses it
//! at the others.

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
    assert_stream(&stderr, &[(5, 3, "error", "E_STATE_UNMAPPED")]);
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
fn java_writes_a_literal_its_block_takes_as_given() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@oak_log[axis=x]");
    let out = TempDir::new().expect("out tempdir");
    let compiled = compile(&src, "java", "1.21.4", out.path(), None);
    let stderr = stderr_of(&compiled);
    assert_eq!(compiled.status.code(), Some(0), "{stderr}");
    assert_stream(&stderr, &[]);
    let body = nbt_body(&out.path().join("s.nbt"));
    assert!(
        bytes_contain(&body, string_tag("axis", "x")),
        "the Properties compound carries `axis=x`",
    );
}

#[test]
fn a_pinned_target_refuses_a_value_outside_the_blocks_states_and_writes_nothing() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@oak_log[axis=q]");
    let out = TempDir::new().expect("out tempdir");
    let compiled = compile(&src, "java", "1.21.4", out.path(), None);
    let stderr = stderr_of(&compiled);
    assert_eq!(compiled.status.code(), Some(1), "{stderr}");
    assert_stream(&stderr, &[(2, 13, "error", "E_STATE_DOMAIN")]);
    assert!(
        stderr.contains(
            "`minecraft:oak_log` takes no `axis=q` at java 1.21.4; `axis` takes x, y, z\n  \
             note: did you mean `axis=x`?\n  \
             note: correct the state literal to a value java 1.21.4 takes, or leave `axis` out \
             for the block's default\n"
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

    // Unpinned, the literal is left to a build that pins a version.
    let unpinned = cairn("check", &[src.to_str().unwrap()]);
    assert_eq!(unpinned.status.code(), Some(0), "{}", stderr_of(&unpinned));
    assert_stream(&stderr_of(&unpinned), &[]);
}

#[test]
fn a_pair_an_older_version_takes_names_that_version() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@creaking_heart[active=true]");
    let src = src.to_str().unwrap();
    let pinned = cairn("check", &[src, "--edition", "java", "--target", "26.3"]);
    let stderr = stderr_of(&pinned);
    assert_eq!(pinned.status.code(), Some(1), "{stderr}");
    assert_stream(
        &stderr,
        &[(2, 13, "error", "E_STATE_DOMAIN"), PARTIAL_BUILD],
    );
    assert!(
        stderr.contains(
            "`minecraft:creaking_heart` has no state `active` at java 26.3; its states are axis, \
             creaking_heart_state, natural\n  \
             note: `active=true` is valid at 1.21.4: pass that version as `--target`, or correct \
             the state literal to a state java 26.3 takes\n"
        ),
        "{stderr}",
    );

    let older = cairn("check", &[src, "--edition", "java", "--target", "1.21.4"]);
    assert_eq!(older.status.code(), Some(0), "{}", stderr_of(&older));
    assert_stream(&stderr_of(&older), &[]);
}

/// The refusal `cairn check --target` adds after a scope that did not lower.
const PARTIAL_BUILD: (u64, u64, &str, &str) = (0, 0, "error", "E_PARTIAL_BUILD");

/// The primaries of `stderr`'s `E_STATE_DOMAIN` findings, in report order.
fn state_domain_primaries(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter_map(|line| line.split_once(": error[E_STATE_DOMAIN]: ").map(|(_, p)| p))
        .collect()
}

#[test]
fn a_pair_several_versions_take_offers_one_of_them() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@trial_spawner[ominous=true]");
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.20.4",
        ],
    );
    let stderr = stderr_of(&out);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_stream(
        &stderr,
        &[(2, 13, "error", "E_STATE_DOMAIN"), PARTIAL_BUILD],
    );
    assert!(
        stderr.contains(
            "`minecraft:trial_spawner` has no state `ominous` at java 1.20.4; its states are \
             trial_spawner_state\n  \
             note: `ominous=true` is valid at 1.21, 1.21.4, 26.3: pass one of them as \
             `--target`, or correct the state literal to a state java 1.20.4 takes\n"
        ),
        "{stderr}",
    );
}

#[test]
fn each_pair_outside_is_its_own_finding_in_the_states_order() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@oak_log[lit=true,axis=q]");
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ],
    );
    let stderr = stderr_of(&out);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_stream(
        &stderr,
        &[
            (2, 13, "error", "E_STATE_DOMAIN"),
            (2, 13, "error", "E_STATE_DOMAIN"),
            PARTIAL_BUILD,
        ],
    );
    assert_eq!(
        state_domain_primaries(&stderr),
        [
            "`minecraft:oak_log` takes no `axis=q` at java 1.21.4; `axis` takes x, y, z",
            "`minecraft:oak_log` has no state `lit` at java 1.21.4; its states are axis",
        ],
    );
}

#[test]
fn bedrock_checks_a_literal_against_the_canonical_states_its_map_reads() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@oak_stairs[facing=up]");
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--target",
            "1.21.60",
        ],
    );
    let stderr = stderr_of(&out);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_stream(
        &stderr,
        &[(2, 13, "error", "E_STATE_DOMAIN"), PARTIAL_BUILD],
    );
    assert!(
        stderr.contains(
            "`minecraft:oak_stairs` takes no `facing=up` at bedrock 1.21.60; `facing` takes \
             north, south, west, east\n  \
             note: correct the state literal to a value bedrock 1.21.60 takes, or leave \
             `facing` out for the block's default\n"
        ),
        "{stderr}",
    );
}

#[test]
fn a_bedrock_value_only_a_later_versions_map_reads_names_that_version() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@noteblock[instrument=trumpet]");
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--target",
            "1.21.60",
        ],
    );
    let stderr = stderr_of(&out);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_stream(
        &stderr,
        &[(2, 13, "error", "E_STATE_DOMAIN"), PARTIAL_BUILD],
    );
    assert!(
        stderr.contains(
            "\n  note: `instrument=trumpet` is valid at 1.26.60: pass that version as \
             `--target`, or correct the state literal to a value bedrock 1.21.60 takes\n"
        ),
        "{stderr}",
    );
}

#[test]
fn a_bedrock_block_no_java_block_maps_onto_is_left_to_its_state_map() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@frame[facing=north]");
    let out = cairn(
        "check",
        &[
            src.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--target",
            "1.21.60",
        ],
    );
    let stderr = stderr_of(&out);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert_stream(&stderr, &[(5, 3, "error", "E_STATE_UNMAPPED")]);
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

    for args in [
        vec![src],
        vec![src, "--edition", "java", "--target", "1.21.4"],
        vec![src, "--edition", "bedrock", "--target", "1.26.60"],
    ] {
        let out = cairn("check", &args);
        let stderr = stderr_of(&out);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {stderr}");
        assert_stream(&stderr, &[]);
    }
}
