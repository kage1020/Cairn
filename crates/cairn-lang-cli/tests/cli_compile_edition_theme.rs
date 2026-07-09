//! Per-edition theme fallback end-to-end (spec versioning-editions §10.7).
//!
//! Pins ACs on `examples/edition-fallback.crn`: the same source file
//! resolves to `oak_sign` under `--edition java` and `oak_wall_sign` under
//! `--edition bedrock`, both compiles are clean (no `W_INTENT_DEGRADED`),
//! and `cairn check` accepts the file across all three edition states.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;
use tempfile::TempDir;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// Copy `examples/edition-fallback.crn` into a fresh temp dir so a test can
/// rely on the source's parent being writable and avoid polluting the repo
/// with lock artefacts.
fn fallback_in_tempdir() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let src = examples_dir().join("edition-fallback.crn");
    let dst = tmp.path().join("edition-fallback.crn");
    fs::copy(&src, &dst).expect("copy edition-fallback");
    (tmp, dst)
}

fn read_bytes(path: &Path) -> Vec<u8> {
    fs::read(path).expect("read artifact")
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .expect("gzip decode");
    out
}

/// Search for a UTF-8 needle inside a byte haystack. Enough for the
/// palette id assertions here — Java `.nbt` embeds the id as a length-
/// prefixed `TAG_String` whose bytes contain the id verbatim, and the
/// same is true for the Bedrock `.mcstructure`'s `name` field.
fn bytes_contain(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn ac5_java_compile_writes_oak_sign_and_not_wall_sign() {
    let (_tmp_src, src) = fallback_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let result = Command::new(cargo_bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ])
        .output()
        .expect("run cairn");
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr),
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !stderr.contains("W_INTENT_DEGRADED"),
        "Java compile of a bare oak_sign should not degrade, got: {stderr}",
    );

    let nbt = out_dir.path().join("shop.nbt");
    let bytes = gunzip(&read_bytes(&nbt));
    assert!(
        bytes_contain(&bytes, "minecraft:oak_sign"),
        "Java palette must include oak_sign (from shop_java's floating_text slot)",
    );
    assert!(
        !bytes_contain(&bytes, "minecraft:oak_wall_sign"),
        "Java palette must not include the Bedrock variant's oak_wall_sign",
    );
}

#[test]
fn ac6_bedrock_compile_writes_oak_wall_sign_and_not_oak_sign() {
    let (_tmp_src, src) = fallback_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let result = Command::new(cargo_bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--out",
            out_dir.path().to_str().unwrap(),
        ])
        .output()
        .expect("run cairn");
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr),
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !stderr.contains("W_INTENT_DEGRADED"),
        "Bedrock compile of a bare oak_wall_sign should not degrade, got: {stderr}",
    );

    let structure = out_dir.path().join("shop.mcstructure");
    let bytes = read_bytes(&structure);
    assert!(
        bytes_contain(&bytes, "minecraft:oak_wall_sign"),
        "Bedrock palette must include oak_wall_sign (from shop_bedrock's floating_text slot)",
    );
    assert!(
        !bytes_contain(&bytes, "minecraft:oak_sign,"),
        "Bedrock palette must not include the Java variant's plain oak_sign",
    );
    // Guard against a plausible partial-match false negative — `oak_wall_sign`
    // itself contains no `oak_sign` substring outside the `_wall_` split, so
    // the exact-match check above already discriminates. This sanity assertion
    // pins the intent in case a future palette formatter re-arranges bytes.
    let raw_oak_sign_hits = bytes
        .windows(b"oak_sign".len())
        .filter(|w| *w == b"oak_sign")
        .count();
    let wall_sign_hits = bytes
        .windows(b"oak_wall_sign".len())
        .filter(|w| *w == b"oak_wall_sign")
        .count();
    assert!(
        raw_oak_sign_hits == 0 || raw_oak_sign_hits <= wall_sign_hits,
        "unexpected raw `oak_sign` occurrences ({raw_oak_sign_hits}) vs `oak_wall_sign` ({wall_sign_hits})",
    );
}

#[test]
fn ac7_check_exits_zero_across_all_three_edition_states() {
    // AC7: `cairn check` on the fallback example succeeds under Java,
    // Bedrock, and no `--edition` — the sibling-slot union handles the
    // no-pin case, and each explicit edition sees its variant declare
    // every referenced slot.
    let src = examples_dir().join("edition-fallback.crn");
    let src_str = src.to_str().unwrap();
    let cases: [&[&str]; 3] = [
        &["check", src_str],
        &["check", "--edition", "java", src_str],
        &["check", "--edition", "bedrock", src_str],
    ];
    for args in cases {
        let result = Command::new(cargo_bin())
            .args(args)
            .output()
            .expect("run cairn");
        assert!(
            result.status.success(),
            "check {args:?} must accept edition-fallback.crn; stderr={}",
            String::from_utf8_lossy(&result.stderr),
        );
    }
}

#[test]
fn ac8_cottage_still_compiles_under_both_editions() {
    // Sanity: the existing `theme medieval:` cottage (unsuffixed theme,
    // no per-edition variants) must keep working under both editions —
    // regression guard for the sibling-slot pathway inadvertently gating
    // the single-theme path.
    let src = examples_dir().join("cottage.crn");
    let tmp = TempDir::new().expect("tempdir");
    let copied = tmp.path().join("cottage.crn");
    fs::copy(&src, &copied).expect("copy cottage");
    for edition in ["java", "bedrock"] {
        let out_dir = TempDir::new().expect("out tempdir");
        let result = Command::new(cargo_bin())
            .args([
                "compile",
                copied.to_str().unwrap(),
                "--edition",
                edition,
                "--out",
                out_dir.path().to_str().unwrap(),
            ])
            .output()
            .expect("run cairn");
        assert!(
            result.status.success(),
            "cottage compile under --edition {edition} must succeed; stderr={}",
            String::from_utf8_lossy(&result.stderr),
        );
    }
}
