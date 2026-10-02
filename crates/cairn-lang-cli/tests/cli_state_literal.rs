//! A canonical token's block-state literal, `@oak_log[axis=x]`, end to end.
//!
//! The core tests pin what the parser folds and the palette holds; these
//! pin what each command does with it. Nothing checks a literal against
//! the target yet, so the commands are asked the questions that answer
//! differently: `check` and `compile --edition java` accept and announce
//! it, `info` counts what Bedrock cannot map, and `compile --edition
//! bedrock` refuses the same entry.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tempfile::TempDir;

mod common;
use common::{cairn, write_source};

/// A one-theme, one-struct source binding `target` to a floor.
fn floored(dir: &Path, target: &str) -> PathBuf {
    write_source(
        dir,
        "s.crn",
        &format!("theme t:\n  slot f -> {target}\n\nstruct s size=3x3\n  floor mat_slot=f\n"),
    )
}

fn stderr_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Compile `src` for `edition` at `target` into `out`.
fn compile(src: &Path, edition: &str, target: &str, out: &Path) -> std::process::Output {
    cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            edition,
            "--target",
            target,
            "--out",
            out.to_str().unwrap(),
        ],
    )
}

/// The Java writer gzips; the assertions read the NBT inside.
fn nbt_body(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).expect("read artifact");
    let mut body = Vec::new();
    GzDecoder::new(bytes.as_slice())
        .read_to_end(&mut body)
        .expect("gzip decode");
    body
}

/// The NBT bytes of a `String` tag named `key` holding `value`: tag id 8,
/// then each of the two as a big-endian `u16` length and its bytes.
fn string_tag(key: &str, value: &str) -> Vec<u8> {
    let mut tag = vec![8];
    for part in [key, value] {
        tag.extend(u16::try_from(part.len()).expect("short").to_be_bytes());
        tag.extend(part.as_bytes());
    }
    tag
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `info` answers with a figure, exit 0, where `compile --edition bedrock`
/// refuses the build, exit 1: the first counts an entry the backend cannot
/// map, the second is the backend. A backend that started dropping the
/// properties instead would leave `info`'s answer standing and turn this
/// one green-to-red.
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
    let compiled = compile(&src, "bedrock", "1.21.60", out.path());
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

/// Java writes a literal as given, a value no `oak_log` has included —
/// which is what `spec/syntax` says it does until `E_STATE_DOMAIN` lands —
/// and says so on the literal rather than in silence.
#[test]
fn java_writes_an_unchecked_literal_as_given_and_says_so() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@oak_log[axis=q]");
    let out = TempDir::new().expect("out tempdir");
    let compiled = compile(&src, "java", "1.21.4", out.path());
    let stderr = stderr_of(&compiled);
    assert_eq!(compiled.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("warning[W_STATE_LITERAL_UNCHECKED]")
            && stderr.contains("`minecraft:oak_log` is written with `axis=q`"),
        "{stderr}",
    );
    let body = nbt_body(&out.path().join("s.nbt"));
    assert!(
        contains(&body, &string_tag("axis", "q")),
        "the Properties compound carries `axis=q`",
    );
}

/// The `.nbt` is a function of the set of states a build holds, so the
/// order a literal spells its properties in cannot reach its bytes.
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
        let compiled = compile(&src, "java", "1.21.4", out.path());
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

/// `check` reads the literal the way the build does: no error with or
/// without a target, and with one, the lowering's announcement that the
/// literal went unchecked is the only finding.
#[test]
fn a_well_formed_literal_passes_check() {
    let tmp = TempDir::new().expect("tempdir");
    let src = floored(tmp.path(), "@oak_stairs[half=top, facing=north]");

    let plain = cairn("check", &[src.to_str().unwrap()]);
    let stderr = stderr_of(&plain);
    assert_eq!(plain.status.code(), Some(0), "{stderr}");
    assert!(!stderr.contains("error"), "{stderr}");

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
    assert!(!stderr.contains("error"), "{stderr}");
    assert_eq!(
        stderr.matches("warning[").count(),
        1,
        "only the unchecked literal: {stderr}",
    );
    assert!(
        stderr.contains("warning[W_STATE_LITERAL_UNCHECKED]"),
        "{stderr}"
    );
}
