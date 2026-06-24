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
    // or material tokens the M2 backend can't fully realise — abstract
    // tokens and unsupported roles degrade to air with a warning at the
    // lowering step, so the palette that reaches the Java backend is
    // already concrete. A non-zero exit here means a regression in either
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
