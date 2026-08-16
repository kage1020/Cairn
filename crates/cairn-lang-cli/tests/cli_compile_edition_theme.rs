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
fn ac6_bedrock_compile_writes_the_bedrock_wall_sign_and_not_the_java_one() {
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
    // Bedrock has no `oak_wall_sign` at all — it spells the block
    // `wall_sign`, and the pack used to hand the Java spelling to the
    // `.mcstructure` writer, which produced a file the game loads as air.
    // `oak_wall_sign` contains `wall_sign` as a substring, so a plain
    // `wall_sign` hit does not by itself prove the Java id is gone;
    // counting `oak_` separately is what distinguishes them.
    let java_spelling_hits = bytes
        .windows(b"oak_wall_sign".len())
        .filter(|w| *w == b"oak_wall_sign")
        .count();
    let bedrock_spelling_hits = bytes
        .windows(b"wall_sign".len())
        .filter(|w| *w == b"wall_sign")
        .count();
    assert!(
        bedrock_spelling_hits > 0,
        "Bedrock palette must include wall_sign (from shop_bedrock's floating_text slot); got {bedrock_spelling_hits} hits",
    );
    assert_eq!(
        java_spelling_hits, 0,
        "Bedrock palette must not include the Java-only oak_wall_sign; got {java_spelling_hits} bytewise hits",
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

// ----------------------------------------------------------------------
// The pin reaches the site placement path, and a pin the module cannot
// satisfy stops the build instead of building it out of air.
// ----------------------------------------------------------------------

/// A module whose logical theme `shop` exists in `variants`, placed through
/// a site under `theme={reference}`. Each variant binds a different block so
/// the artifact says which one was bound.
fn placed_source(reference: &str, variants: &[&str]) -> String {
    use std::fmt::Write as _;

    let mut declarations = String::new();
    for variant in variants {
        let block = match *variant {
            "_bedrock" => "dark_oak_planks",
            _ => "spruce_planks",
        };
        write!(
            declarations,
            "theme shop{variant}:\n\x20\x20slot floor -> @{block}\n\n"
        )
        .expect("writing to a String cannot fail");
    }
    format!(
        "@cairn 2026.06\n\n{declarations}\
         def hut size=4x4:\n\x20\x20floor mat_slot=floor\n\nsite s:\n\
         \x20\x20place id=home use=hut theme={reference} at=origin\n"
    )
}

fn write_source(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).expect("write source");
    path
}

#[test]
fn a_java_build_writes_the_java_variants_block_whatever_the_place_named() {
    // The defect, end to end: `theme=shop_bedrock` bound verbatim on the
    // site path, so a `--edition java` artifact carried the Bedrock
    // variant's material. The guard that was supposed to prevent it
    // (`pick_variant`) was never reached from here.
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(
        tmp.path(),
        "placed.crn",
        &placed_source("shop_bedrock", &["_java", "_bedrock"]),
    );
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
    let nbt = gunzip(&read_bytes(&out_dir.path().join("home.nbt")));
    assert!(
        bytes_contain(&nbt, "minecraft:spruce_planks"),
        "the Java variant's material must be the one written",
    );
    assert!(
        !bytes_contain(&nbt, "minecraft:dark_oak_planks"),
        "the Bedrock variant's material must not reach a Java artifact",
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("W_THEME_VARIANT_REBOUND"),
        "swapping the named variant is worth saying out loud; stderr={stderr}",
    );
}

#[test]
fn a_build_whose_pin_has_no_variant_stops_before_writing_anything() {
    // Every `mat_slot=` fell back to air here and the build still wrote a
    // structure plus a `verified: true` lockfile. The refusal now happens
    // in the resolver, so nothing is written at all.
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(
        tmp.path(),
        "bedrock-only.crn",
        &placed_source("shop_bedrock", &["_bedrock"]),
    );
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
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("E_THEME_VARIANT_MISSING"),
        "stderr={stderr}",
    );
    assert!(
        fs::read_dir(out_dir.path())
            .expect("out dir readable")
            .next()
            .is_none(),
        "no artifact may be written for a build that cannot bind its theme",
    );
    assert!(
        !tmp.path().join("bedrock-only.crn.lock").exists(),
        "no lockfile may certify a build that did not happen",
    );
}

#[test]
fn check_reports_the_missing_variant_only_when_an_edition_is_pinned() {
    // `check --edition` is the gate a CI job runs; it has to see this, and
    // the same source without a pin has nothing to fail against.
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(
        tmp.path(),
        "bedrock-only.crn",
        &placed_source("shop_bedrock", &["_bedrock"]),
    );
    let pinned = Command::new(cargo_bin())
        .args(["check", src.to_str().unwrap(), "--edition", "java"])
        .output()
        .expect("run cairn");
    // `check` renders its diagnostic stream on stdout; the build commands
    // use stderr because their stdout carries the artifact report.
    let report = String::from_utf8_lossy(&pinned.stdout);
    assert_eq!(pinned.status.code(), Some(1), "report={report}");
    assert!(
        report.contains("E_THEME_VARIANT_MISSING"),
        "report={report}",
    );

    let unpinned = Command::new(cargo_bin())
        .args(["check", src.to_str().unwrap()])
        .output()
        .expect("run cairn");
    assert_eq!(
        unpinned.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&unpinned.stderr),
    );

    let matching = Command::new(cargo_bin())
        .args(["check", src.to_str().unwrap(), "--edition", "bedrock"])
        .output()
        .expect("run cairn");
    assert_eq!(
        matching.status.code(),
        Some(0),
        "the variant the module does declare must still bind; stderr={}",
        String::from_utf8_lossy(&matching.stderr),
    );
}

#[test]
fn a_place_naming_the_logical_theme_builds_under_both_editions() {
    // The spelling §10.7 prescribes was the one spelling the site path
    // rejected, because no theme is declared under the bare logical name.
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(
        tmp.path(),
        "logical.crn",
        &placed_source("shop", &["_java", "_bedrock"]),
    );
    for (edition, expected, absent) in [
        (
            "java",
            "minecraft:spruce_planks",
            "minecraft:dark_oak_planks",
        ),
        (
            "bedrock",
            "minecraft:dark_oak_planks",
            "minecraft:spruce_planks",
        ),
    ] {
        let out_dir = TempDir::new().expect("out tempdir");
        let result = Command::new(cargo_bin())
            .args([
                "compile",
                src.to_str().unwrap(),
                "--edition",
                edition,
                "--out",
                out_dir.path().to_str().unwrap(),
            ])
            .output()
            .expect("run cairn");
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(result.status.success(), "--edition {edition}: {stderr}");
        assert!(
            !stderr.contains("W_THEME_VARIANT_REBOUND"),
            "the neutral spelling names no variant, so none was swapped: {stderr}",
        );
        let artifact = out_dir.path().join(if edition == "java" {
            "home.nbt"
        } else {
            "home.mcstructure"
        });
        let bytes = read_bytes(&artifact);
        let body = if edition == "java" {
            gunzip(&bytes)
        } else {
            bytes
        };
        assert!(bytes_contain(&body, expected), "--edition {edition}");
        assert!(!bytes_contain(&body, absent), "--edition {edition}");
    }
}

#[test]
fn a_misspelled_variant_is_a_diagnostic_and_not_a_crash() {
    // `theme=shop_java` against a module that declares only `shop_bedrock`,
    // with no `--edition`. Routing the reference through variant selection
    // made this reach a rebind with no edition to name in the message, and
    // the resolver panicked — turning a typo into exit 101 on `check`, on
    // `lower` (which has no `--edition` at all), and inside the language
    // server, whose diagnostics pass calls the resolver with `None`.
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(
        tmp.path(),
        "typo.crn",
        &placed_source("shop_java", &["_bedrock"]),
    );
    for args in [
        vec!["check", src.to_str().unwrap()],
        vec!["lower", src.to_str().unwrap()],
    ] {
        let result = Command::new(cargo_bin())
            .args(&args)
            .output()
            .expect("run cairn");
        let code = result.status.code();
        assert_ne!(
            code,
            Some(101),
            "`cairn {}` must not panic; stderr={}",
            args[0],
            String::from_utf8_lossy(&result.stderr),
        );
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr),
        );
        assert!(
            combined.contains("E_UNRESOLVED_THEME_REF"),
            "`cairn {}` must name the undeclared theme; got: {combined}",
            args[0],
        );
    }
}

#[test]
fn the_lockfile_names_the_theme_the_artifact_was_built_from() {
    // A rebind leaves one warning on stderr and nothing else. The lockfile
    // is the record that survives the terminal, so naming the written
    // variant there would point a later reader at materials the artifact
    // does not contain.
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(
        tmp.path(),
        "rebound.crn",
        &placed_source("shop_bedrock", &["_java", "_bedrock"]),
    );
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

    let lock = fs::read_to_string(tmp.path().join("rebound.crn.lock")).expect("read lock");
    assert!(
        lock.contains("theme: shop_java"),
        "the lock must name the bound variant; got:\n{lock}",
    );
    assert!(
        !lock.contains("theme: shop_bedrock"),
        "the lock must not name the variant the build did not use; got:\n{lock}",
    );
    // And the artifact agrees with the lock, which is the claim that makes
    // the field worth anything.
    let nbt = gunzip(&read_bytes(&out_dir.path().join("home.nbt")));
    assert!(bytes_contain(&nbt, "minecraft:spruce_planks"));
}
