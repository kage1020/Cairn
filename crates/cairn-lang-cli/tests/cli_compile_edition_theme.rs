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
    // `oak_wall_sign` = `o a k _ w a l l _ s i g n`, which does **not** contain
    // `oak_sign` = `o a k _ s i g n` as a substring (after `oak_` comes
    // `wall_`, not `sign`). So the two counts are independent bytewise —
    // a raw `oak_sign` hit implies the plain Java-variant block leaked
    // into the Bedrock palette. This replaces the earlier `!bytes_contain(
    // "minecraft:oak_sign,")` guard, which was vacuously true (literal
    // commas do not appear inside NBT-encoded strings).
    let raw_oak_sign_hits = bytes
        .windows(b"oak_sign".len())
        .filter(|w| *w == b"oak_sign")
        .count();
    let wall_sign_hits = bytes
        .windows(b"oak_wall_sign".len())
        .filter(|w| *w == b"oak_wall_sign")
        .count();
    assert!(
        wall_sign_hits > 0,
        "Bedrock palette must include oak_wall_sign (from shop_bedrock's floating_text slot); got {wall_sign_hits} hits",
    );
    assert_eq!(
        raw_oak_sign_hits, 0,
        "Bedrock palette must not include the Java variant's plain oak_sign; got {raw_oak_sign_hits} bytewise hits",
    );
}

#[test]
fn check_edition_flag_wires_strict_variant_pin_through_cli() {
    // Wiring guard: a file whose only `floating_text` binding lives in the
    // Bedrock variant must pass `check --edition bedrock` and pass
    // `check` (no pin, sibling-slot union), but fail `check --edition java`
    // with `E_UNRESOLVED_SLOT`. Without this pin, a CLI dispatch bug that
    // forwarded `None` regardless of the `--edition` flag would still pass
    // every resolver-level unit test (which invoke `resolve` directly) —
    // only the CLI wiring test catches it.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let path = tmp.path().join("bedrock_only_slot.crn");
    std::fs::write(
        &path,
        [
            "@cairn 2026.06",
            "@requires version>=1.20",
            "",
            "theme t_java:",
            "  slot floor -> @oak_planks",
            "",
            "theme t_bedrock:",
            "  slot floor -> @oak_planks",
            "  slot bedrock_only -> @dark_oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=bedrock_only",
            "",
        ]
        .join("\n"),
    )
    .expect("write test crn");
    let path_str = path.to_str().unwrap();

    // 1. `check --edition bedrock` succeeds: the Bedrock variant declares
    //    the slot.
    let out = Command::new(cargo_bin())
        .args(["check", "--edition", "bedrock", path_str])
        .output()
        .expect("run cairn");
    assert!(
        out.status.success(),
        "check --edition bedrock must succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    // 2. `check` (no pin) succeeds: the sibling-slot union covers the slot.
    let out = Command::new(cargo_bin())
        .args(["check", path_str])
        .output()
        .expect("run cairn");
    assert!(
        out.status.success(),
        "check (no --edition) must succeed via sibling-slot union; stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    // 3. `check --edition java` fails with `E_UNRESOLVED_SLOT`: the Java
    //    variant does not declare `bedrock_only`, and the strict-pin path
    //    disables the sibling-slot union.
    let out = Command::new(cargo_bin())
        .args(["check", "--edition", "java", path_str])
        .output()
        .expect("run cairn");
    // `cairn check` writes diagnostic lines to stdout (matching gcc's
    // convention for tool integration), reserving stderr for I/O and
    // parse-level errors that pre-empt the diagnostic pipeline. Look at
    // stdout for the code assertion.
    assert_eq!(
        out.status.code(),
        Some(1),
        "check --edition java must exit 1 on Bedrock-only slot; stdout={}; stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("E_UNRESOLVED_SLOT"),
        "expected E_UNRESOLVED_SLOT in diagnostics, got stdout={stdout}",
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
