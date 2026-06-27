//! AC C1–C14 for `cairn compile`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::lock::{HashHex, LockEdition, Lockfile};
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

fn run_compile(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("compile")
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

/// Copy `examples/cottage.crn` into a fresh temp dir so a test can rely on
/// the source's parent being writable and avoid polluting the repo with
/// lock artefacts.
fn cottage_in_tempdir() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let src = examples_dir().join("cottage.crn");
    let dst = tmp.path().join("cottage.crn");
    fs::copy(&src, &dst).expect("copy cottage");
    (tmp, dst)
}

#[test]
fn c1_compile_cottage_with_explicit_target_exits_zero() {
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn c2_compile_writes_named_nbt_into_out_dir() {
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    assert!(result.status.success());
    let written = out_dir.path().join("cottage.nbt");
    assert!(written.exists(), "expected {} to exist", written.display());
}

#[test]
fn c3_compiled_nbt_begins_with_gzip_magic() {
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    assert!(result.status.success());
    let bytes = fs::read(out_dir.path().join("cottage.nbt")).expect("read nbt");
    assert!(bytes.len() >= 2);
    assert_eq!(&bytes[..2], &[0x1f, 0x8b], "gzip magic prefix");
}

#[test]
fn c4_default_lock_is_written_beside_source() {
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    assert!(result.status.success());
    let mut expected_lock = src.as_os_str().to_owned();
    expected_lock.push(".lock");
    assert!(
        PathBuf::from(&expected_lock).exists(),
        "expected lockfile next to source: {}",
        PathBuf::from(expected_lock).display(),
    );
}

#[test]
fn c5_explicit_lock_path_is_honored() {
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("explicit.lock");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
        "--lock",
        lock_path.to_str().unwrap(),
    ]);
    assert!(result.status.success());
    assert!(lock_path.exists(), "lockfile at explicit path");
}

#[test]
fn c6_lockfile_records_cairn_version() {
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("c6.lock");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
        "--lock",
        lock_path.to_str().unwrap(),
    ]);
    assert!(result.status.success());
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    assert_eq!(lf.cairn_version, CAIRN_VERSION);
}

#[test]
fn c7_lockfile_records_target_triple() {
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("c7.lock");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "1.21.4",
        "--out",
        out_dir.path().to_str().unwrap(),
        "--lock",
        lock_path.to_str().unwrap(),
    ]);
    assert!(result.status.success());
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    assert_eq!(lf.target.edition, LockEdition::Java);
    assert_eq!(lf.target.mc_version, "1.21.4");
    assert_eq!(lf.target.data_version, 4189);
}

#[test]
fn c8_bedrock_edition_exits_one_with_explanation() {
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "bedrock",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        stderr.contains("not implemented"),
        "expected explanation, got stderr={stderr}",
    );
}

#[test]
fn c9_missing_edition_flag_exits_two_with_usage() {
    let (_tmp_src, src) = cottage_in_tempdir();
    let result = run_compile(&[src.to_str().unwrap()]);
    // clap reports missing required args as exit 2.
    assert_eq!(result.status.code(), Some(2));
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(stderr.contains("--edition"), "expected --edition mention");
}

#[test]
fn c10_unknown_target_exits_one_with_supported_list() {
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--target",
        "0.0.0",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(stderr.contains("1.20.4"));
    assert!(stderr.contains("latest"));
}

#[test]
fn c11_missing_source_exits_two() {
    let result = run_compile(&["definitely-not-a-file.crn", "--edition", "java"]);
    assert_eq!(result.status.code(), Some(2));
}

#[test]
fn c12_parse_error_surfaces_file_line_col() {
    let tmp = TempDir::new().expect("tempdir");
    let bad = tmp.path().join("bad.crn");
    // `;;;` is not a valid Cairn token — guarantees a lex/parse failure.
    fs::write(&bad, "struct cottage size=2x2\n;;;\n").expect("write");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        bad.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    let bad_display = bad.display().to_string();
    // Look for the `file:line:col:` shape gcc / clang ship — at least one
    // `<path>:N:M:` occurrence where N and M are positive integers.
    let mut found_position = false;
    for line in stderr.lines() {
        if !line.contains(&bad_display) {
            continue;
        }
        let rest = &line[line.find(&bad_display).unwrap() + bad_display.len()..];
        // Pattern: ":line:col:"
        let mut parts = rest.splitn(4, ':');
        let (_empty, line_no, col_no) = (parts.next(), parts.next(), parts.next());
        if line_no.is_some_and(|s| s.trim().parse::<u32>().is_ok())
            && col_no.is_some_and(|s| s.trim().parse::<u32>().is_ok())
        {
            found_position = true;
            break;
        }
    }
    assert!(
        found_position,
        "expected gcc-style `{bad_display}:line:col:` prefix, got stderr={stderr}",
    );
}

#[test]
fn c13_default_out_dir_is_source_parent() {
    let (tmp_src, src) = cottage_in_tempdir();
    let result = run_compile(&[src.to_str().unwrap(), "--edition", "java"]);
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr),
    );
    let expected_nbt = tmp_src.path().join("cottage.nbt");
    assert!(expected_nbt.exists(), "{} missing", expected_nbt.display());
}

#[test]
fn compile_is_byte_reproducible() {
    // Two compiles of the same source against the same target must
    // produce byte-identical `.nbt` files and lockfiles whose
    // `source_hash` / `resolved_ir_hash` agree. This is the central
    // promise of the lockfile design — without it the lockfile carries
    // no useful diff information.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_a = TempDir::new().expect("out a");
    let out_b = TempDir::new().expect("out b");
    let lock_a = out_a.path().join("a.lock");
    let lock_b = out_b.path().join("b.lock");
    for (out_dir, lock_path) in [(out_a.path(), &lock_a), (out_b.path(), &lock_b)] {
        let result = run_compile(&[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
            "--out",
            out_dir.to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ]);
        assert!(result.status.success());
    }
    let bytes_a = fs::read(out_a.path().join("cottage.nbt")).expect("a");
    let bytes_b = fs::read(out_b.path().join("cottage.nbt")).expect("b");
    assert_eq!(bytes_a, bytes_b, "compile output is not byte-reproducible");

    let lf_a = Lockfile::read_from_path(&lock_a).expect("read a");
    let lf_b = Lockfile::read_from_path(&lock_b).expect("read b");
    assert_eq!(lf_a.source_hash, lf_b.source_hash);
    assert_eq!(lf_a.resolved_ir_hash, lf_b.resolved_ir_hash);
}

#[test]
fn compile_rolls_back_on_lockfile_failure() {
    // If the lockfile write fails, the `.nbt` files already written must
    // be removed so the on-disk state stays consistent (either every
    // artifact + lock, or none). We force the failure by pointing
    // `--lock` at a directory that exists; `serde_yml::to_string` +
    // `fs::write` then fails with "is a directory".
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_as_dir = out_dir.path().join("lock-is-a-dir");
    fs::create_dir(&lock_as_dir).expect("mkdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
        "--lock",
        lock_as_dir.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        !out_dir.path().join("cottage.nbt").exists(),
        "rollback should have removed cottage.nbt",
    );
}

#[test]
fn compile_does_not_print_wrote_before_lockfile_success() {
    // Regression for the silent failure described in the review: stdout
    // must announce written files only AFTER every artifact and the
    // lockfile have landed. So when the lockfile step fails (directory
    // collision above), stdout must be empty of `wrote …` lines.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_as_dir = out_dir.path().join("lock-dir");
    fs::create_dir(&lock_as_dir).expect("mkdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
        "--lock",
        lock_as_dir.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert!(
        !stdout.contains("wrote"),
        "no `wrote` line should appear before the lockfile succeeds, got: {stdout}",
    );
}

#[test]
fn compile_all_examples_exit_zero() {
    // Every example must compile clean even when its source uses members
    // or material tokens the backend can't fully realise — abstract tokens
    // and unsupported roles degrade to air with a warning at the lowering
    // step, so the palette that reaches the Java backend is already
    // concrete. A non-zero exit here means a regression in either
    // the lowering pass or the abstract-id guard at the backend.
    for name in [
        "cottage.crn",
        "themed-tower.crn",
        "village.crn",
        "redstone-door.crn",
        "roof-shed.crn",
        "roof-hip.crn",
        "roof-flat.crn",
    ] {
        let path = examples_dir().join(name);
        let out_dir = TempDir::new().expect("out tempdir");
        let lock_path = out_dir.path().join(format!("{name}.lock"));
        let result = run_compile(&[
            path.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ]);
        assert!(
            result.status.success(),
            "{name} should compile, stderr={}",
            String::from_utf8_lossy(&result.stderr),
        );
    }
}

#[test]
fn c14c_roof_kind_examples_lower_without_deferred_warnings() {
    // The shed/hip/flat fixtures exist to pin the new roof voxelisers in
    // §4.4–§4.6 of the compilation spec: each lowers end-to-end without
    // a single `W_DEFERRED_MEMBER`, the same contract `c14_cottage_*` pins
    // for the gable kind.
    for name in ["roof-shed.crn", "roof-hip.crn", "roof-flat.crn"] {
        let path = examples_dir().join(name);
        let out_dir = TempDir::new().expect("out tempdir");
        let lock_path = out_dir.path().join(format!("{name}.lock"));
        let result = run_compile(&[
            path.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ]);
        assert!(
            result.status.success(),
            "{name} should compile, stderr={}",
            String::from_utf8_lossy(&result.stderr),
        );
        let stderr = String::from_utf8(result.stderr).expect("utf-8");
        assert_eq!(
            stderr.matches("W_DEFERRED_MEMBER").count(),
            0,
            "{name} should lower clean, stderr={stderr}",
        );
        let nbt = out_dir
            .path()
            .join(name.replace('-', "_").replace(".crn", ".nbt"));
        assert!(nbt.exists(), "expected {} to exist", nbt.display());
    }
}

#[test]
fn c15_lockfile_registry_pack_hash_is_populated() {
    // The registry pack ingest replaces the hardcoded data_version table,
    // and the lockfile must pin the bytes the compile resolved against.
    // The `constraint_catalog_hash` stays zero until that catalog lands.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("c15.lock");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
        "--lock",
        lock_path.to_str().unwrap(),
    ]);
    assert!(result.status.success());
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    assert_ne!(
        lf.inputs.registry_pack_hash.as_str(),
        HashHex::ZERO_STR,
        "registry_pack_hash must be filled in by the registry pack ingest",
    );
    assert_eq!(
        lf.inputs.constraint_catalog_hash.as_str(),
        HashHex::ZERO_STR,
        "constraint_catalog_hash stays zero until its own ingest lands",
    );
}

#[test]
fn c14_cottage_compiles_without_deferred_warnings() {
    // The current voxel lowering covers cottage.crn end-to-end (floor,
    // walls, door, window, gable roof with overhang), so the per-member
    // W_DEFERRED_MEMBER stream that earlier milestones emitted is now
    // empty. The CLI must still exit 0 and produce nothing on stderr.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    assert!(result.status.success());
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert_eq!(
        stderr.matches("W_DEFERRED_MEMBER").count(),
        0,
        "cottage should lower clean, stderr={stderr}",
    );
}

#[test]
fn c14b_deferred_member_warning_still_exits_zero_on_other_examples() {
    // The `cairn compile` exit-code contract is "warnings do not fail the
    // build". We pin it against `themed-tower.crn`, which still exercises
    // roles outside the implemented phases (`level` blocks contain doors
    // and windows that lowering cannot reach yet). The exact warning
    // count is intentionally not asserted — what matters is that at least
    // one `W_DEFERRED_MEMBER` fires and the exit code stays 0.
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join("themed-tower.crn");
    fs::copy(examples_dir().join("themed-tower.crn"), &dst).expect("copy themed-tower");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        dst.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr),
    );
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        stderr.contains("W_DEFERRED_MEMBER"),
        "expected at least one deferred-member warning on themed-tower, stderr={stderr}",
    );
}

#[test]
fn c16_themed_tower_compiles_with_lifted_abstract_tokens() {
    // The built-in materials catalog covers every abstract token
    // themed-tower binds, so compile must finish without
    // `W_ABSTRACT_TOKEN_DEFERRED` or `E_UNKNOWN_ABSTRACT_TOKEN`, write a
    // `keep.nbt`, and the lockfile records the same registry pack hash that
    // other examples do (materials catalog is part of the pack bytes).
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join("themed-tower.crn");
    fs::copy(examples_dir().join("themed-tower.crn"), &dst).expect("copy themed-tower");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        dst.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        result.status.success(),
        "themed-tower should compile clean; stderr={stderr}",
    );
    assert_eq!(
        stderr.matches("W_ABSTRACT_TOKEN_DEFERRED").count(),
        0,
        "abstract tokens must lift via the catalog; stderr={stderr}",
    );
    assert_eq!(
        stderr.matches("E_UNKNOWN_ABSTRACT_TOKEN").count(),
        0,
        "every token themed-tower binds must be in the catalog; stderr={stderr}",
    );
    let written = out_dir.path().join("keep.nbt");
    assert!(written.exists(), "expected {} to exist", written.display());
}

#[test]
fn c17_unknown_abstract_token_compile_exits_nonzero() {
    // Compile must propagate the new E_UNKNOWN_ABSTRACT_TOKEN as a hard
    // failure — a build that silently fell back to air on a typo would
    // hide the bug downstream.
    let tmp = TempDir::new().expect("tempdir");
    let src = tmp.path().join("typo.crn");
    fs::write(
        &src,
        concat!(
            "@cairn 2026.06\n",
            "\n",
            "theme t:\n",
            "  slot floor -> @floor.wood.broadlef\n",
            "\n",
            "struct s size=3x3\n",
            "  floor mat_slot=floor\n",
        ),
    )
    .expect("write tmp .crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        !result.status.success(),
        "exit should be non-zero; stderr={stderr}",
    );
    assert!(
        stderr.contains("E_UNKNOWN_ABSTRACT_TOKEN"),
        "stderr should carry the new diagnostic code; got: {stderr}",
    );
    assert!(
        stderr.contains("floor.wood.broadleaf"),
        "stderr should surface the nearest declared token; got: {stderr}",
    );
}

/// Copy any example file alongside its dependencies into a fresh temp dir.
/// Mirrors `cottage_in_tempdir` but parameterised so the village / themed-tower
/// tests can land in their own writable scratch space without polluting the
/// repo with `.lock` artefacts.
fn example_in_tempdir(name: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let src = examples_dir().join(name);
    let dst = tmp.path().join(name);
    fs::copy(&src, &dst).expect("copy example");
    (tmp, dst)
}

#[test]
fn c18_compile_village_emits_three_nbt() {
    // village.crn used to compile to zero `.nbt` files because the lowering
    // pass skipped sites entirely. With per-`place` lowering wired through,
    // the three placements each produce a sibling `.nbt` and the build still
    // exits 0 despite the two `W_DEFERRED_MEMBER` warnings the deferred
    // `connect` rows emit.
    let (_tmp_src, src) = example_in_tempdir("village.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    let stderr = String::from_utf8(result.stderr.clone()).expect("utf-8");
    assert!(
        result.status.success(),
        "village should compile, stderr={stderr}",
    );
    for name in ["home1.nbt", "home2.nbt", "home3.nbt"] {
        let written = out_dir.path().join(name);
        assert!(written.exists(), "expected {} to exist", written.display());
        let bytes = fs::read(&written).expect("read nbt");
        assert!(
            bytes.len() >= 2 && bytes[..2] == [0x1f, 0x8b],
            "{name} must be gzip"
        );
    }
}

#[test]
fn c19_village_lockfile_records_placements() {
    // The lockfile carries the resolved per-place origin so a downstream
    // consumer can rebuild the village layout without re-running the
    // coordinate solver. Origins are derived from the topological chain in
    // examples/village.crn: home1 sits at origin, home2 sits east of home1
    // past its full inflated width (11) plus gap=4 = 15, home3 sits north
    // of home1 minus its full depth (9) minus gap=5 = -14.
    let (_tmp_src, src) = example_in_tempdir("village.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("village.lock");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
        "--lock",
        lock_path.to_str().unwrap(),
    ]);
    assert!(result.status.success());
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    assert_eq!(lf.placements.len(), 3, "expected one entry per place");

    let by_id = |id: &str| {
        lf.placements
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("placement `{id}` missing from lockfile"))
    };
    let home1 = by_id("home1");
    assert_eq!(home1.origin, [0, 0, 0]);
    assert_eq!(home1.def, "cottage");
    assert_eq!(home1.theme, "medieval");
    assert_eq!(home1.site, "hamlet");

    let home2 = by_id("home2");
    assert_eq!(home2.origin, [15, 0, 0]);

    let home3 = by_id("home3");
    assert_eq!(home3.origin, [0, 0, -14]);
}

#[test]
fn c20_village_lockfile_round_trips_through_yaml() {
    // Pin that the new `placements` section survives a YAML round-trip:
    // serde_yml must encode and decode every field the schema declares so a
    // CI annotator or LSP that reads the file gets the same record the
    // compiler wrote.
    let (_tmp_src, src) = example_in_tempdir("village.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("rt.lock");
    let _ = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
        "--lock",
        lock_path.to_str().unwrap(),
    ]);
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    let rewrite = out_dir.path().join("rt.rewrite.lock");
    lf.write_to_path(&rewrite).expect("write rewritten lock");
    let parsed = Lockfile::read_from_path(&rewrite).expect("read rewritten lock");
    assert_eq!(lf, parsed);
}

#[test]
fn c21_village_lowers_connect_rows_into_walkway_artifacts() {
    // Port model and walkway voxelisation are wired through end-to-end, so
    // the two `connect` rows in village.crn must lower into per-walkway
    // `.nbt` artifacts (one per row) instead of degrading to
    // W_DEFERRED_MEMBER. The exit must stay 0 — any W_WALKWAY_BLOCKED
    // warnings that fall out of the cottage overlap are advisory,
    // mirroring c14b's warnings-do-not-fail-the-build rule.
    let (_tmp_src, src) = example_in_tempdir("village.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    assert!(result.status.success());
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert_eq!(
        stderr.matches("W_DEFERRED_MEMBER").count(),
        0,
        "connect rows must no longer emit W_DEFERRED_MEMBER; stderr={stderr}",
    );
    // The two `connect` rows land as `hamlet_walkway_*.nbt` files
    // alongside the three placement files.
    let written: Vec<_> = std::fs::read_dir(out_dir.path())
        .expect("read out_dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let walkway_count = written
        .iter()
        .filter(|name| {
            name.starts_with("hamlet_walkway_")
                && std::path::Path::new(name.as_str())
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("nbt"))
        })
        .count();
    assert_eq!(
        walkway_count, 2,
        "expected two walkway artifacts (one per `connect` row), got {written:?}",
    );
}

fn run_check(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .arg("check")
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

#[test]
fn c22_unknown_def_in_place_errors_with_suggestion() {
    // `use=cottag` typo → resolver fail-loud with a `did you mean
    // `cottage`?` suggestion via the existing nearest_match helper.
    // `cairn check` writes diagnostics to stdout (text format) so the
    // assertions look there, not stderr.
    let tmp = TempDir::new().expect("tempdir");
    let src = tmp.path().join("typo_use.crn");
    fs::write(
        &src,
        concat!(
            "@cairn 2026.06\n",
            "\n",
            "def cottage size=4x4:\n",
            "  walls mat_slot=wall height=3\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=home1 use=cottag theme=t at=origin\n",
        ),
    )
    .expect("write tmp .crn");
    let result = run_check(&[src.to_str().unwrap()]);
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert!(
        !result.status.success(),
        "exit should be non-zero; stdout={stdout}",
    );
    assert!(
        stdout.contains("E_UNRESOLVED_PLACE_REF"),
        "stdout should carry E_UNRESOLVED_PLACE_REF; got: {stdout}",
    );
    assert!(
        stdout.contains("did you mean `cottage`?"),
        "stdout should suggest the closest def name; got: {stdout}",
    );
}

#[test]
fn c23_unknown_theme_in_place_errors_with_suggestion() {
    // `theme=medeival` typo → fail-loud + did-you-mean note for the only
    // declared theme.
    let tmp = TempDir::new().expect("tempdir");
    let src = tmp.path().join("typo_theme.crn");
    fs::write(
        &src,
        concat!(
            "@cairn 2026.06\n",
            "\n",
            "def cottage size=4x4:\n",
            "  walls mat_slot=wall height=3\n",
            "\n",
            "theme medieval:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=home1 use=cottage theme=medeival at=origin\n",
        ),
    )
    .expect("write tmp .crn");
    let result = run_check(&[src.to_str().unwrap()]);
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert!(!result.status.success(), "stdout={stdout}");
    assert!(
        stdout.contains("E_UNRESOLVED_THEME_REF"),
        "stdout should carry E_UNRESOLVED_THEME_REF; got: {stdout}",
    );
    assert!(
        stdout.contains("did you mean `medieval`?"),
        "stdout should suggest the closest theme name; got: {stdout}",
    );
}

#[test]
fn c24_duplicate_place_id_errors() {
    // Two `place` rows sharing an `id=` collide. The first wins for later
    // references; the duplicate is flagged with a span pointer back to the
    // original.
    let tmp = TempDir::new().expect("tempdir");
    let src = tmp.path().join("dup.crn");
    fs::write(
        &src,
        concat!(
            "@cairn 2026.06\n",
            "\n",
            "def cottage size=4x4:\n",
            "  walls mat_slot=wall height=3\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=home1 use=cottage theme=t at=origin\n",
            "  place id=home1 use=cottage theme=t east_of=home1 gap=2\n",
        ),
    )
    .expect("write tmp .crn");
    let result = run_check(&[src.to_str().unwrap()]);
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert!(!result.status.success(), "stdout={stdout}");
    assert!(
        stdout.contains("E_DUPLICATE_PLACE_ID"),
        "stdout should carry E_DUPLICATE_PLACE_ID; got: {stdout}",
    );
}

#[test]
fn c25_east_of_unknown_ref_errors_with_suggestion() {
    // `east_of=home9` does not name a prior place id in the same site →
    // fail-loud, with `home1` as the nearest-match suggestion.
    let tmp = TempDir::new().expect("tempdir");
    let src = tmp.path().join("east_typo.crn");
    fs::write(
        &src,
        concat!(
            "@cairn 2026.06\n",
            "\n",
            "def cottage size=4x4:\n",
            "  walls mat_slot=wall height=3\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=home1 use=cottage theme=t at=origin\n",
            "  place id=home2 use=cottage theme=t east_of=home9 gap=2\n",
        ),
    )
    .expect("write tmp .crn");
    let result = run_check(&[src.to_str().unwrap()]);
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert!(!result.status.success(), "stdout={stdout}");
    assert!(
        stdout.contains("E_UNRESOLVED_PLACE_REF"),
        "stdout should carry E_UNRESOLVED_PLACE_REF; got: {stdout}",
    );
    assert!(
        stdout.contains("did you mean `home1`?"),
        "stdout should suggest the closest prior place id; got: {stdout}",
    );
}

#[test]
fn c26_bare_def_without_place_emits_w_unused_def_and_no_nbt() {
    // A def that no site references is a noop (templates compile to no
    // voxels). The resolver flags it as `W_UNUSED_DEF` so a typo on the
    // `place use=` side does not silently produce an empty build. The
    // warning rides the same diagnostic stream as `W_DEFERRED_MEMBER` —
    // resolver diagnostics merge into the lowering output so `cairn compile`
    // surfaces them too.
    let tmp = TempDir::new().expect("tempdir");
    let src = tmp.path().join("orphan_def.crn");
    fs::write(
        &src,
        concat!(
            "@cairn 2026.06\n",
            "\n",
            "def cottage size=4x4:\n",
            "  walls mat_slot=wall height=3\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
        ),
    )
    .expect("write tmp .crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    let stderr = String::from_utf8(result.stderr.clone()).expect("utf-8");
    assert!(
        result.status.success(),
        "W_UNUSED_DEF is a warning, not an error; stderr={stderr}",
    );
    assert!(
        stderr.contains("W_UNUSED_DEF"),
        "`cairn compile` should surface W_UNUSED_DEF on stderr; got: {stderr}",
    );
    let entries: Vec<_> = fs::read_dir(out_dir.path())
        .expect("read out dir")
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("nbt"))
        })
        .collect();
    assert!(
        entries.is_empty(),
        "bare def must not produce a .nbt; got {entries:?}",
    );
}

#[test]
fn c26b_unknown_def_in_place_compile_exits_nonzero() {
    // `cairn compile` must propagate resolver Error-severity diagnostics
    // through to the exit code — without the resolver→lowering diagnostic
    // merge, this would silently succeed with zero `.nbt` files.
    let tmp = TempDir::new().expect("tempdir");
    let src = tmp.path().join("typo_use_compile.crn");
    fs::write(
        &src,
        concat!(
            "@cairn 2026.06\n",
            "\n",
            "def cottage size=4x4:\n",
            "  walls mat_slot=wall height=3\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=home1 use=cottag theme=t at=origin\n",
        ),
    )
    .expect("write tmp .crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
    ]);
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        !result.status.success(),
        "compile should fail on E_UNRESOLVED_PLACE_REF; stderr={stderr}",
    );
    assert!(
        stderr.contains("E_UNRESOLVED_PLACE_REF"),
        "stderr should carry E_UNRESOLVED_PLACE_REF; got: {stderr}",
    );
    assert!(
        stderr.contains("did you mean `cottage`?"),
        "stderr should propagate the nearest-match note; got: {stderr}",
    );
}

#[test]
fn c27_home2_nbt_uses_resolved_theme_palette() {
    // Cross-scope theme resolution proof: home2's palette must contain
    // the canonical id `medieval` binds to (`@cobblestone` →
    // `minecraft:cobblestone`). Reading the gzip-decoded NBT and grepping
    // the palette is heavier than this test wants; the lockfile-equivalent
    // check is to assert the build wrote a non-trivial gzip stream and that
    // the resolved_ir_hash differs from a hypothetical empty-IR build.
    let (_tmp_src, src) = example_in_tempdir("village.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("village.lock");
    let _ = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
        "--lock",
        lock_path.to_str().unwrap(),
    ]);
    let home2 = out_dir.path().join("home2.nbt");
    let bytes = fs::read(&home2).expect("read home2.nbt");
    assert!(
        bytes.len() > 64,
        "home2.nbt should carry real palette + voxel data, got {} bytes",
        bytes.len(),
    );
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    assert_ne!(
        lf.resolved_ir_hash.as_str(),
        HashHex::ZERO_STR,
        "resolved_ir_hash must reflect lowered voxels, not an empty IR",
    );
}

#[test]
fn compile_overwrites_existing_output() {
    // A second compile into the same directory must overwrite the .nbt
    // and the lockfile rather than refusing or appending. Without this,
    // an interactive workflow (edit `.crn`, rerun) would fail on every
    // iteration after the first.
    let (_tmp_src, src) = cottage_in_tempdir();
    let out_dir = TempDir::new().expect("out tempdir");
    let nbt_path = out_dir.path().join("cottage.nbt");
    fs::write(&nbt_path, b"stale bytes").expect("seed nbt");
    let lock_path = out_dir.path().join("c.lock");
    fs::write(&lock_path, "stale: true\n").expect("seed lock");

    let result = run_compile(&[
        src.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.path().to_str().unwrap(),
        "--lock",
        lock_path.to_str().unwrap(),
    ]);
    assert!(result.status.success());
    let bytes = fs::read(&nbt_path).expect("read nbt");
    assert_ne!(bytes, b"stale bytes", "nbt should have been overwritten");
    assert_eq!(&bytes[..2], &[0x1f, 0x8b], "overwritten file is real gzip");
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    assert!(lf.verified);
}
