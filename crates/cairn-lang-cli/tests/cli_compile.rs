//! AC C1–C14 for `cairn compile`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use cairn_lang_core::CAIRN_VERSION;
use cairn_lang_core::lock::Lockfile;
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
    assert_eq!(lf.target.edition, "java");
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
    assert!(
        stderr.contains(&bad_display) && stderr.contains(':'),
        "expected gcc-style position prefix, got stderr={stderr}",
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
fn c14_deferred_member_warning_does_not_fail_compile() {
    // cottage.crn already exercises floor + walls + non-lowered roles, so
    // the lowering pass writes W_DEFERRED_MEMBER warnings — `cairn compile`
    // must still exit 0 in that case (matching `cairn lower`).
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
    assert!(
        stderr.contains("W_DEFERRED_MEMBER") || stderr.contains("warning[") || stderr.is_empty(),
        "stderr should carry diagnostics or be empty, got: {stderr}",
    );
}
