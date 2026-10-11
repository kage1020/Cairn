//! Per-edition theme variants end to end (`spec/versioning-editions`
//! "Java / Bedrock portability"): one source writes each edition's own
//! variant without degrading, a variant the pinned edition lacks is
//! refused or rebound out loud, and `cairn check` accepts the shipped
//! example whichever edition is pinned.

use std::fs;
use std::process::Command;

use tempfile::TempDir;

use crate::common::{
    assert_stream, bytes_contain, cargo_bin, example_in_tempdir, examples_dir, nbt_body, stderr_of,
    write_source,
};

#[test]
fn java_compile_writes_oak_sign_and_not_wall_sign() {
    let (_tmp_src, src) = example_in_tempdir("edition-fallback.crn");
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
    let bytes = nbt_body(&nbt);
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
fn bedrock_compile_writes_the_bedrock_wall_sign_and_not_the_java_one() {
    let (_tmp_src, src) = example_in_tempdir("edition-fallback.crn");
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
    let bytes = fs::read(&structure).expect("read artifact");
    // Bedrock spells the block `wall_sign` and loads the Java spelling as
    // air. `oak_wall_sign` contains `wall_sign`, so the two are counted
    // separately.
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
    // Resolver unit tests call `resolve` directly, so only this catches a
    // dispatch that forwards `None` whatever `--edition` says.
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

    let out = Command::new(cargo_bin())
        .args(["check", "--edition", "bedrock", path_str])
        .output()
        .expect("run cairn");
    assert!(
        out.status.success(),
        "check --edition bedrock must succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    let out = Command::new(cargo_bin())
        .args(["check", path_str])
        .output()
        .expect("run cairn");
    assert!(
        out.status.success(),
        "check (no --edition) must succeed via sibling-slot union; stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    // A pin disables the sibling-slot union.
    let out = Command::new(cargo_bin())
        .args(["check", "--edition", "java", path_str])
        .output()
        .expect("run cairn");
    assert_eq!(
        out.status.code(),
        Some(1),
        "check --edition java must exit 1 on Bedrock-only slot; stdout={}; stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let reported = String::from_utf8_lossy(&out.stderr);
    assert_stream(&reported, &[(12, 3, "error", "E_UNRESOLVED_SLOT")]);
}

#[test]
fn check_exits_zero_across_all_three_edition_states() {
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
fn cottage_still_compiles_under_both_editions() {
    // An unsuffixed theme with no per-edition variants is not gated by
    // the sibling-slot path.
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

// The pin reaches the site placement path, and a pin the module cannot
// satisfy stops the build instead of building it out of air.

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

#[test]
fn a_java_build_writes_the_java_variants_block_whatever_the_place_named() {
    // The site path has to reach `pick_variant` too, or
    // `theme=shop_bedrock` binds verbatim into a Java artifact.
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
    let nbt = nbt_body(&out_dir.path().join("home.nbt"));
    assert!(
        bytes_contain(&nbt, "minecraft:spruce_planks"),
        "the Java variant's material must be the one written",
    );
    assert!(
        !bytes_contain(&nbt, "minecraft:dark_oak_planks"),
        "the Bedrock variant's material must not reach a Java artifact",
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_stream(&stderr, &[(13, 3, "warning", "W_THEME_VARIANT_REBOUND")]);
}

#[test]
fn check_reports_the_missing_variant_only_when_an_edition_is_pinned() {
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
    let report = String::from_utf8_lossy(&pinned.stderr);
    assert_eq!(pinned.status.code(), Some(1), "report={report}");
    assert_stream(&report, &[(3, 1, "error", "E_THEME_VARIANT_MISSING")]);

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
    // The spelling `spec/versioning-editions` "Java / Bedrock portability"
    // prescribes, though no theme carries the bare logical name.
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
        assert_stream(&stderr, &[]);
        let artifact = out_dir.path().join(if edition == "java" {
            "home.nbt"
        } else {
            "home.mcstructure"
        });
        let body = if edition == "java" {
            nbt_body(&artifact)
        } else {
            fs::read(&artifact).expect("read artifact")
        };
        assert!(bytes_contain(&body, expected), "--edition {edition}");
        assert!(!bytes_contain(&body, absent), "--edition {edition}");
    }
}

#[test]
fn a_misspelled_variant_is_a_diagnostic_and_not_a_crash() {
    // Variant selection reaches a rebind with no edition to name. `lower`
    // has no `--edition`, and the language server resolves with `None`.
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
        // Not `assert_ne!(code, Some(101))`: under the release profile's
        // `panic = "abort"` a panic is a signal and `code` is `None`.
        assert!(
            matches!(code, Some(c) if c != 101),
            "`cairn {}` must not panic; exit={code:?} stderr={}",
            args[0],
            String::from_utf8_lossy(&result.stderr),
        );
        assert_stream(
            &stderr_of(&result),
            &[(10, 3, "error", "E_UNRESOLVED_THEME_REF")],
        );
    }
}

#[test]
fn the_lockfile_names_the_theme_the_artifact_was_built_from() {
    // A rebind's only other trace is one warning on stderr, and the lock
    // outlives the terminal.
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
    let nbt = nbt_body(&out_dir.path().join("home.nbt"));
    assert!(bytes_contain(&nbt, "minecraft:spruce_planks"));
}
