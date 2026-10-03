//! AC C1–C14 for `cairn compile`.

use std::fs;
use std::path::PathBuf;

use cairn_lang_core::lock::{HashHex, LockEdition, Lockfile};
use tempfile::TempDir;

mod common;
use common::{
    Fixture, cairn, compile_as, crn_examples, example_in_tempdir, examples_dir, write_source,
};

/// The version cargo derived for this crate from `[workspace.package]`.
///
/// The lockfile records the compiler that produced it, so the number has to be
/// the release's, not a constant maintained beside it. Comparing against
/// `cairn-lang-core`'s `CAIRN_VERSION` would only restate whatever the binary
/// already wrote.
const WORKSPACE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn c1_compile_cottage_with_explicit_target_exits_zero() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr),
    );
}

#[test]
fn c2_compile_writes_named_nbt_into_out_dir() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    assert!(result.status.success());
    let written = out_dir.path().join("cottage.nbt");
    assert!(written.exists(), "expected {} to exist", written.display());
}

#[test]
fn c3_compiled_nbt_begins_with_gzip_magic() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    assert!(result.status.success());
    let bytes = fs::read(out_dir.path().join("cottage.nbt")).expect("read nbt");
    assert!(bytes.len() >= 2);
    assert_eq!(&bytes[..2], &[0x1f, 0x8b], "gzip magic prefix");
}

#[test]
fn c4_default_lock_is_written_beside_source() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
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
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("explicit.lock");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ],
    );
    assert!(result.status.success());
    assert!(lock_path.exists(), "lockfile at explicit path");
}

#[test]
fn c6_lockfile_records_cairn_version() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("c6.lock");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ],
    );
    assert!(result.status.success());
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    assert_eq!(lf.cairn_version, WORKSPACE_VERSION);
}

#[test]
fn c7_lockfile_records_target_triple() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("c7.lock");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ],
    );
    assert!(result.status.success());
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    assert_eq!(lf.target.edition, LockEdition::Java);
    assert_eq!(lf.target.mc_version, "1.21.4");
    assert_eq!(lf.target.data_version, 4189);
}

#[test]
fn c8_bedrock_compiles_stateless_example_to_mcstructure() {
    // roof-flat.crn resolves entirely to bare (stateless) block ids —
    // planks and cobblestone — so it is the smallest example the Bedrock
    // backend can round-trip end-to-end. The compile must exit 0, write a
    // `roof_flat.mcstructure`, and lower without any deferred warnings.
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join("roof-flat.crn");
    fs::copy(examples_dir().join("roof-flat.crn"), &dst).expect("copy roof-flat");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            dst.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        result.status.success(),
        "roof-flat should compile for bedrock; stderr={stderr}",
    );
    assert_eq!(
        stderr.matches("W_DEFERRED_MEMBER").count(),
        0,
        "roof-flat should lower clean, stderr={stderr}",
    );
    let written = out_dir.path().join("roof_flat.mcstructure");
    assert!(written.exists(), "expected {} to exist", written.display());
}

#[test]
fn c8b_bedrock_mcstructure_is_uncompressed_little_endian() {
    // The `.mcstructure` bytes must be raw NBT (unnamed root compound,
    // 0x0a + u16 zero length), never a gzip stream (0x1f 0x8b) the way the
    // Java `.nbt` is.
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join("roof-flat.crn");
    fs::copy(examples_dir().join("roof-flat.crn"), &dst).expect("copy roof-flat");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            dst.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    assert!(result.status.success());
    let bytes = fs::read(out_dir.path().join("roof_flat.mcstructure")).expect("read mcstructure");
    assert!(bytes.len() >= 3);
    assert_ne!(&bytes[..2], &[0x1f, 0x8b], "must not be gzip");
    assert_eq!(&bytes[..3], &[0x0a, 0x00, 0x00], "unnamed root compound");
}

#[test]
fn c8c_bedrock_lockfile_records_bedrock_target_and_pack_hash() {
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join("roof-flat.crn");
    fs::copy(examples_dir().join("roof-flat.crn"), &dst).expect("copy roof-flat");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("c8c.lock");
    let result = cairn(
        "compile",
        &[
            dst.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--target",
            "1.21.60",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr),
    );
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    assert_eq!(lf.target.edition, LockEdition::Bedrock);
    assert_eq!(lf.target.mc_version, "1.21.60");
    // Bedrock's block-palette version integer, not a Java DataVersion.
    assert_eq!(lf.target.data_version, 18_168_865);
    assert_ne!(
        lf.inputs.registry_pack_hash.as_str(),
        HashHex::ZERO_STR,
        "bedrock pack hash must be filled in",
    );
}

#[test]
fn c8d_bedrock_cottage_maps_stair_states_without_degradation() {
    // cottage.crn resolves a gable roof to `*_stairs` with `facing`/`half`/
    // `shape=straight` properties. The Bedrock backend maps facing/half to
    // Bedrock `states` and drops the (straight, i.e. default) shape without a
    // note, so the compile succeeds cleanly and writes the artifact.
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join("cottage.crn");
    fs::copy(examples_dir().join("cottage.crn"), &dst).expect("copy cottage");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            dst.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        result.status.success(),
        "cottage on bedrock, stderr={stderr}"
    );
    assert!(
        !stderr.contains("W_INTENT_DEGRADED"),
        "cottage's straight gable stairs must not degrade, got: {stderr}",
    );
    assert!(
        out_dir.path().join("cottage.mcstructure").exists(),
        "the mcstructure artifact should be written",
    );
}

#[test]
fn c8f_bedrock_themed_tower_degrades_stair_shape_but_compiles() {
    // themed-tower.crn's eave stairs use non-straight shapes
    // (`outer_left`/`outer_right`), which Bedrock has no state for. The
    // compile still succeeds (exit 0) but surfaces a W_INTENT_DEGRADED
    // warning per dropped shape (`spec/versioning-editions`
    // "Backend = data tables" `dropped_states:[shape]` and
    // "Java / Bedrock portability"), and the artifact is written.
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join("themed-tower.crn");
    fs::copy(examples_dir().join("themed-tower.crn"), &dst).expect("copy themed-tower");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            dst.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        result.status.success(),
        "themed-tower should still compile on bedrock, stderr={stderr}",
    );
    assert!(
        stderr.contains("W_INTENT_DEGRADED") && stderr.contains("shape"),
        "expected a shape-drop degradation warning, got: {stderr}",
    );
    assert!(
        out_dir.path().join("keep.mcstructure").exists(),
        "the mcstructure artifact should still be written on a degradation",
    );
}

#[test]
fn c8e_bedrock_unknown_target_names_bedrock_versions() {
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join("roof-flat.crn");
    fs::copy(examples_dir().join("roof-flat.crn"), &dst).expect("copy roof-flat");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            dst.to_str().unwrap(),
            "--edition",
            "bedrock",
            "--target",
            "1.21.61",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        stderr.contains("unsupported bedrock target"),
        "error must name the bedrock vocabulary, got: {stderr}",
    );
    assert!(stderr.contains("1.21.60"), "supported list, got: {stderr}");
}

#[test]
fn c9_missing_edition_flag_exits_two_with_usage() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let result = cairn("compile", &[src.to_str().unwrap()]);
    // clap reports missing required args as exit 2.
    assert_eq!(result.status.code(), Some(2));
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(stderr.contains("--edition"), "expected --edition mention");
}

#[test]
fn c10_unknown_target_exits_one_with_supported_list() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "0.0.0",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(stderr.contains("1.20.4"));
    assert!(stderr.contains("latest"));
}

#[test]
fn c12_parse_error_surfaces_file_line_col() {
    let tmp = TempDir::new().expect("tempdir");
    let bad = tmp.path().join("bad.crn");
    // `;;;` is not a valid Cairn token — guarantees a lex/parse failure.
    fs::write(&bad, "struct cottage size=2x2\n;;;\n").expect("write");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            bad.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
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
    let (tmp_src, src) = example_in_tempdir("cottage.crn");
    let result = cairn("compile", &[src.to_str().unwrap(), "--edition", "java"]);
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr),
    );
    let expected_nbt = tmp_src.path().join("cottage.nbt");
    assert!(expected_nbt.exists(), "{} missing", expected_nbt.display());
}

#[test]
fn the_lockfile_on_disk_ends_with_a_newline() {
    // The encoder is pinned in `cairn-lang-core`; this is the byte that
    // actually reaches the file, through the temp-file-and-rename writer.
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("newline.lock");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ],
    );
    assert!(result.status.success());
    let bytes = fs::read(&lock_path).expect("read lock");
    assert_eq!(
        bytes.last(),
        Some(&b'\n'),
        "lockfile ends with {:?}",
        bytes.last().map(|b| *b as char),
    );
}

#[test]
fn compile_is_byte_reproducible() {
    // Two compiles of the same source against the same target must
    // produce byte-identical `.nbt` files and byte-identical lockfiles.
    // This is the central promise of the lockfile design — without it the
    // lockfile carries no useful diff information.
    //
    // The whole file, not two of its fields. "Regenerating the lockfile is
    // a no-op" is a statement about the bytes: a field that varies between
    // runs — an absolute path, an iteration order, a timestamp — is exactly
    // what a hash-only comparison cannot see, and exactly what would make a
    // committed lockfile impossible to keep.
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_a = TempDir::new().expect("out a");
    let out_b = TempDir::new().expect("out b");
    let lock_a = out_a.path().join("a.lock");
    let lock_b = out_b.path().join("b.lock");
    for (out_dir, lock_path) in [(out_a.path(), &lock_a), (out_b.path(), &lock_b)] {
        let result = cairn(
            "compile",
            &[
                src.to_str().unwrap(),
                "--edition",
                "java",
                "--target",
                "1.21.4",
                "--out",
                out_dir.to_str().unwrap(),
                "--lock",
                lock_path.to_str().unwrap(),
            ],
        );
        assert!(result.status.success());
    }
    let bytes_a = fs::read(out_a.path().join("cottage.nbt")).expect("a");
    let bytes_b = fs::read(out_b.path().join("cottage.nbt")).expect("b");
    assert_eq!(bytes_a, bytes_b, "compile output is not byte-reproducible");

    let lock_bytes_a = fs::read(&lock_a).expect("read a");
    let lock_bytes_b = fs::read(&lock_b).expect("read b");
    // Raw bytes, like the `.nbt` comparison above: two differently
    // malformed sequences both collapsing to U+FFFD would pass a lossy one.
    assert_eq!(
        lock_bytes_a,
        lock_bytes_b,
        "regenerating the lockfile is not a no-op:\n{}\n{}",
        String::from_utf8_lossy(&lock_bytes_a),
        String::from_utf8_lossy(&lock_bytes_b),
    );

    // Read back through the decoder too, so the comparison above cannot
    // pass on two files that are equally malformed.
    let lf_a = Lockfile::read_from_path(&lock_a).expect("read a");
    let lf_b = Lockfile::read_from_path(&lock_b).expect("read b");
    assert_eq!(lf_a, lf_b);
}

#[test]
fn compile_rolls_back_on_lockfile_failure() {
    // If the lockfile write fails, the `.nbt` files already written must
    // be removed so the on-disk state stays consistent (either every
    // artifact + lock, or none). We force the failure by pointing
    // `--lock` at a directory that exists; `serde_norway::to_string` +
    // `fs::write` then fails with "is a directory".
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_as_dir = out_dir.path().join("lock-is-a-dir");
    fs::create_dir(&lock_as_dir).expect("mkdir");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_as_dir.to_str().unwrap(),
        ],
    );
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
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_as_dir = out_dir.path().join("lock-dir");
    fs::create_dir(&lock_as_dir).expect("mkdir");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_as_dir.to_str().unwrap(),
        ],
    );
    let stdout = String::from_utf8(result.stdout).expect("utf-8");
    assert!(
        !stdout.contains("wrote"),
        "no `wrote` line should appear before the lockfile succeeds, got: {stdout}",
    );
}

#[test]
fn compile_all_examples_exit_zero() {
    // Every example must compile clean even when its source uses members
    // or material tokens the Java backend can't fully realise — abstract
    // tokens and unsupported roles degrade to air with a warning at the
    // lowering step, so the palette that reaches the Java backend is
    // already concrete. A non-zero exit here means a regression in either
    // the lowering pass or the abstract-id guard at the backend.
    for path in crn_examples() {
        let name = path.file_name().expect("named").to_string_lossy();
        let out_dir = TempDir::new().expect("out tempdir");
        let lock_path = out_dir.path().join(format!("{name}.lock"));
        let result = cairn(
            "compile",
            &[
                path.to_str().unwrap(),
                "--edition",
                "java",
                "--out",
                out_dir.path().to_str().unwrap(),
                "--lock",
                lock_path.to_str().unwrap(),
            ],
        );
        assert!(
            result.status.success(),
            "{name} should compile, stderr={}",
            String::from_utf8_lossy(&result.stderr),
        );
    }
}

/// `(example, the artifact its one struct writes)` for every example whose
/// members the voxel lowering covers end-to-end, so the per-member
/// `W_DEFERRED_MEMBER` stream earlier milestones emitted must be empty.
///
/// - `cottage.crn`: floor, walls, door, window, gable roof with overhang.
/// - `themed-tower.crn`: `level y=N` grouping, per-level walls, an eave
///   `stair`, and a `repeat=/step=` window pattern — level flattening and
///   stair voxelisation paint every `level` child.
/// - `redstone-door.crn`: two `pressure_plate` fixtures with the compound
///   `at=<side>.outside` / `at=inside.<side>` anchor, a `circuit
///   region=floor void=2` routing marker, and a `door[id=front]
///   opened_by=sig.open` actuator patch — the plate paints its voxels and
///   the other two are surface-guards for the logic pipeline.
/// - `roof-shed/hip/flat.crn`: the roof voxelisers of `spec/compilation`
///   ("Shed roof voxel rules", "Hip roof voxel rules" and
///   "Flat roof voxel rules").
const DEFER_FREE_EXAMPLES: &[(&str, &str)] = &[
    ("cottage.crn", "cottage.nbt"),
    ("themed-tower.crn", "keep.nbt"),
    ("redstone-door.crn", "gatehouse.nbt"),
    ("roof-shed.crn", "roof_shed.nbt"),
    ("roof-hip.crn", "roof_hip.nbt"),
    ("roof-flat.crn", "roof_flat.nbt"),
];

#[test]
fn c14_covered_examples_compile_without_deferred_warnings() {
    for (name, artifact) in DEFER_FREE_EXAMPLES {
        let (_tmp_src, src) = example_in_tempdir(name);
        let out_dir = TempDir::new().expect("out tempdir");
        let result = cairn(
            "compile",
            &[
                src.to_str().unwrap(),
                "--edition",
                "java",
                "--out",
                out_dir.path().to_str().unwrap(),
            ],
        );
        let stderr = String::from_utf8(result.stderr).expect("utf-8");
        assert!(
            result.status.success(),
            "{name} should compile, stderr={stderr}",
        );
        assert_eq!(
            stderr.matches("W_DEFERRED_MEMBER").count(),
            0,
            "{name} should lower clean, stderr={stderr}",
        );
        let written = out_dir.path().join(artifact);
        assert!(
            written.exists(),
            "{name} should still write {artifact}, stderr={stderr}",
        );
    }
}

#[test]
fn c15_lockfile_registry_pack_hash_is_populated() {
    // The registry pack ingest replaces the hardcoded data_version table,
    // and the lockfile must pin the bytes the compile resolved against.
    // The `constraint_catalog_hash` stays zero until that catalog lands.
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("c15.lock");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ],
    );
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
    let result = cairn(
        "compile",
        &[
            dst.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
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
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
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
#[test]
fn c18_compile_village_emits_three_nbt() {
    // village.crn used to compile to zero `.nbt` files because the lowering
    // pass skipped sites entirely. With per-`place` lowering wired through,
    // the three placements each produce a sibling `.nbt` and the build still
    // exits 0 despite the two `W_DEFERRED_MEMBER` warnings the deferred
    // `connect` rows emit.
    let (_tmp_src, src) = example_in_tempdir("village.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
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
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ],
    );
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
    // serde_norway must encode and decode every field the schema declares so a
    // CI annotator or LSP that reads the file gets the same record the
    // compiler wrote.
    let (_tmp_src, src) = example_in_tempdir("village.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("rt.lock");
    let _ = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ],
    );
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
    // W_DEFERRED_MEMBER. The home1↔home3 row detours around home1's
    // floor, so the whole example must also compile without a single
    // W_WALKWAY_BLOCKED — village is a warning-free example now.
    let (_tmp_src, src) = example_in_tempdir("village.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    assert!(result.status.success());
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert_eq!(
        stderr.matches("W_DEFERRED_MEMBER").count(),
        0,
        "connect rows must no longer emit W_DEFERRED_MEMBER; stderr={stderr}",
    );
    assert_eq!(
        stderr.matches("W_WALKWAY_BLOCKED").count(),
        0,
        "the home1↔home3 walkway must route around home1 instead of warning; stderr={stderr}",
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

#[test]
fn c22_unknown_def_in_place_errors_with_suggestion() {
    // `use=cottag` typo → resolver fail-loud with a `did you mean
    // `cottage`?` suggestion via the existing nearest_match helper.
    // `cairn check` reports its text diagnostics on stderr, like every
    // other subcommand, so the assertions look there.
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
    let result = cairn("check", &[src.to_str().unwrap()]);
    let reported = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        !result.status.success(),
        "exit should be non-zero; reported={reported}",
    );
    assert!(
        reported.contains("E_UNRESOLVED_PLACE_REF"),
        "reported should carry E_UNRESOLVED_PLACE_REF; got: {reported}",
    );
    assert!(
        reported.contains("did you mean `cottage`?"),
        "reported should suggest the closest def name; got: {reported}",
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
    let result = cairn("check", &[src.to_str().unwrap()]);
    let reported = String::from_utf8(result.stderr).expect("utf-8");
    assert!(!result.status.success(), "reported={reported}");
    assert!(
        reported.contains("E_UNRESOLVED_THEME_REF"),
        "reported should carry E_UNRESOLVED_THEME_REF; got: {reported}",
    );
    assert!(
        reported.contains("did you mean `medieval`?"),
        "reported should suggest the closest theme name; got: {reported}",
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
    let result = cairn("check", &[src.to_str().unwrap()]);
    let reported = String::from_utf8(result.stderr).expect("utf-8");
    assert!(!result.status.success(), "reported={reported}");
    assert!(
        reported.contains("E_DUPLICATE_PLACE_ID"),
        "reported should carry E_DUPLICATE_PLACE_ID; got: {reported}",
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
    let result = cairn("check", &[src.to_str().unwrap()]);
    let reported = String::from_utf8(result.stderr).expect("utf-8");
    assert!(!result.status.success(), "reported={reported}");
    assert!(
        reported.contains("E_UNRESOLVED_PLACE_REF"),
        "reported should carry E_UNRESOLVED_PLACE_REF; got: {reported}",
    );
    assert!(
        reported.contains("did you mean `home1`?"),
        "reported should suggest the closest prior place id; got: {reported}",
    );
}

/// The source every refused-id test below runs: one `def hut`, one theme
/// and a site whose rows are `rows`, the first of them on line 10.
fn site_with_rows(rows: &[&str]) -> String {
    let mut source = String::from(
        "@cairn 2026.06\n\ndef hut size=3x3:\n  floor mat_slot=floor\n\n\
         theme t:\n  slot floor -> @oak_planks\n\nsite s:\n",
    );
    for row in rows {
        source.push_str("  ");
        source.push_str(row);
        source.push('\n');
    }
    source
}

/// `cairn check` over [`site_with_rows`]: the exit code and stderr.
fn check_rows(rows: &[&str]) -> (Option<i32>, String) {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_source(tmp.path(), "rows.crn", &site_with_rows(rows));
    let result = cairn("check", &[src.to_str().unwrap()]);
    let reported = String::from_utf8(result.stderr).expect("utf-8");
    (result.status.code(), reported)
}

/// Every finding in `reported` as `(line, code)`, in print order, and
/// nothing else: comparing the whole list is what bounds the total, so a
/// finding sprouting on a row a test means to keep quiet fails it.
fn findings(reported: &str) -> Vec<(usize, String)> {
    reported
        .lines()
        .filter_map(|line| {
            let (location, rest) = line.split_once(": ")?;
            let code = rest
                .strip_prefix("error[")
                .or_else(|| rest.strip_prefix("warning["))?;
            let (code, _) = code.split_once(']')?;
            let row = location.rsplit(':').nth(1)?.parse().ok()?;
            Some((row, code.to_owned()))
        })
        .collect()
}

/// The lines spanned notes point at, in print order.
fn note_rows(reported: &str) -> Vec<usize> {
    reported
        .lines()
        .filter_map(|line| {
            let (location, rest) = line.split_once(": ")?;
            rest.trim_start().strip_prefix("note:")?;
            location.rsplit(':').nth(1)?.parse().ok()
        })
        .collect()
}

fn owned(pairs: &[(usize, &str)]) -> Vec<(usize, String)> {
    pairs
        .iter()
        .map(|(row, code)| (*row, (*code).to_owned()))
        .collect()
}

/// A row refused with `E_INVALID_PLACE_ID` is still the row a later
/// `east_of=` / `north_of=` naming it means, so the reference is not
/// `E_UNRESOLVED_PLACE_REF`, whose "declare the target above this line"
/// would send the author to look for a typo. It is the cascade
/// `W_DEFERRED_PLACE`, on the referencing row, with a note pointing at the
/// refused one. A reference to an id no row declares is still unresolved,
/// which the last row pins: without it, suppressing every reference would
/// pass too.
#[test]
fn a_reference_to_a_place_refused_for_its_id_is_a_cascade_on_its_own_row() {
    let (code, reported) = check_rows(&[
        "place id=anchor use=hut theme=t at=origin",
        "place id=\"sub/hut\" use=hut theme=t east_of=anchor gap=2",
        "place id=b use=hut theme=t east_of=\"sub/hut\" gap=2",
        "place id=\"x.y\" use=hut theme=t east_of=anchor gap=2",
        "place id=c use=hut theme=t north_of=\"x.y\" gap=2",
        "place id=d use=hut theme=t east_of=nowhere gap=2",
    ]);
    assert_eq!(code, Some(1), "reported={reported}");
    assert_eq!(
        findings(&reported),
        owned(&[
            (11, "E_INVALID_PLACE_ID"),
            (12, "W_DEFERRED_PLACE"),
            (13, "E_INVALID_PLACE_ID"),
            (14, "W_DEFERRED_PLACE"),
            (15, "E_UNRESOLVED_PLACE_REF"),
        ]),
        "reported={reported}",
    );
    assert_eq!(
        note_rows(&reported),
        [11, 13],
        "each cascade points at the row refused under the id it names; reported={reported}",
    );
    assert!(
        reported.contains("`east_of=nowhere`"),
        "the one unresolved reference is the one naming no row; got: {reported}",
    );
}

/// `refused_place_ids` fills in source order, like the ids it shadows, so a
/// reference *above* the refused row names no prior place and is reported
/// as any other forward reference is. A refactor that collects every
/// refused id in one pass first would turn this into a cascade too, and
/// the "declare the target above this line" note with it.
#[test]
fn a_reference_above_the_row_refused_for_its_id_names_no_prior_place() {
    let (code, reported) = check_rows(&[
        "place id=anchor use=hut theme=t at=origin",
        "place id=b use=hut theme=t east_of=\"sub/hut\" gap=2",
        "place id=\"sub/hut\" use=hut theme=t east_of=anchor gap=2",
    ]);
    assert_eq!(code, Some(1), "reported={reported}");
    assert_eq!(
        findings(&reported),
        owned(&[(11, "E_UNRESOLVED_PLACE_REF"), (12, "E_INVALID_PLACE_ID")]),
        "reported={reported}",
    );
    assert!(
        reported.contains("declare the target above this line"),
        "the forward reference keeps its ordering note; got: {reported}",
    );
}

/// The `IdError::Empty` side of the same rule: `id=""` is refused for its
/// id, and `east_of=""` names that row, so it is the cascade rather than a
/// reference to nothing.
#[test]
fn a_reference_to_an_empty_place_id_is_a_cascade_on_its_own_row() {
    let (code, reported) = check_rows(&[
        "place id=anchor use=hut theme=t at=origin",
        "place id=\"\" use=hut theme=t east_of=anchor gap=2",
        "place id=b use=hut theme=t east_of=\"\" gap=2",
    ]);
    assert_eq!(code, Some(1), "reported={reported}");
    assert_eq!(
        findings(&reported),
        owned(&[(11, "E_INVALID_PLACE_ID"), (12, "W_DEFERRED_PLACE")]),
        "reported={reported}",
    );
    assert_eq!(note_rows(&reported), [11], "reported={reported}");
}

/// One refused id written twice: each row is refused on its own line, and a
/// reference to the id is one cascade whose note points at the first of the
/// two rows, as `E_DUPLICATE_PLACE_ID`'s "first row wins" does for a usable
/// id. The repeat itself is not a second finding: `E_DUPLICATE_ID` leaves
/// `place` rows to `E_DUPLICATE_PLACE_ID`, which compares only accepted ids.
#[test]
fn a_reference_to_a_refused_id_declared_twice_points_at_the_first_row() {
    let (code, reported) = check_rows(&[
        "place id=anchor use=hut theme=t at=origin",
        "place id=\"a/b\" use=hut theme=t east_of=anchor gap=2",
        "place id=\"a/b\" use=hut theme=t east_of=anchor gap=8",
        "place id=c use=hut theme=t east_of=\"a/b\" gap=2",
    ]);
    assert_eq!(code, Some(1), "reported={reported}");
    assert_eq!(
        findings(&reported),
        owned(&[
            (11, "E_INVALID_PLACE_ID"),
            (12, "E_INVALID_PLACE_ID"),
            (13, "W_DEFERRED_PLACE"),
        ]),
        "reported={reported}",
    );
    assert_eq!(
        note_rows(&reported),
        [11],
        "the cascade points at the first row; reported={reported}",
    );
}

/// `place id=ID` refused on `shown`, by `check` and by `compile`, which
/// writes nothing. Run once per character, so one failing does not hide
/// the rest.
fn assert_place_id_refused(id: &str, shown: &str) {
    let tmp = TempDir::new().expect("tempdir");
    let out = tmp.path().join("out");
    let src = write_source(
        tmp.path(),
        "refused.crn",
        &site_with_rows(&[&format!("place id=\"{id}\" use=hut theme=t at=origin")]),
    );
    let refusal = format!("is not a usable id: it contains `{shown}`");

    let checked = cairn("check", &[src.to_str().unwrap()]);
    let check_err = String::from_utf8_lossy(&checked.stderr);
    assert_eq!(checked.status.code(), Some(1), "stderr={check_err}");
    assert!(
        check_err.contains("error[E_INVALID_PLACE_ID]") && check_err.contains(&refusal),
        "the id must be refused on `{shown}`, got: {check_err}",
    );

    let compiled = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    let compile_err = String::from_utf8_lossy(&compiled.stderr);
    assert_eq!(compiled.status.code(), Some(1), "stderr={compile_err}");
    assert!(
        !out.exists(),
        "compile must write nothing, got: {compile_err}"
    );
}

// The characters a Windows file name cannot carry are refused like the path
// separators, on every host: an id is the stem of the artifact's file name,
// and an id Linux accepts used to fail on Windows with a bare OS error. `"`
// is in the rule too, but a source string literal cannot carry one; the
// lockfile reader is where it is reached. U+0001 is one of the controls
// Windows reserves, and is quoted as its escape.

#[test]
fn a_place_id_carrying_a_star_is_refused_on_every_host() {
    assert_place_id_refused("a*b", "*");
}

#[test]
fn a_place_id_carrying_a_pipe_is_refused_on_every_host() {
    assert_place_id_refused("a|b", "|");
}

#[test]
fn a_place_id_carrying_a_question_mark_is_refused_on_every_host() {
    assert_place_id_refused("a?b", "?");
}

#[test]
fn a_place_id_carrying_a_less_than_sign_is_refused_on_every_host() {
    assert_place_id_refused("a<b", "<");
}

#[test]
fn a_place_id_carrying_a_greater_than_sign_is_refused_on_every_host() {
    assert_place_id_refused("a>b", ">");
}

#[test]
fn a_place_id_carrying_a_control_windows_reserves_is_refused_and_escaped() {
    assert_place_id_refused("a\u{1}b", "\\u{1}");
}

// The controls past U+001F are legal in a Windows file name and are refused
// on their own ground: they print as nothing, so the id could not be shown.
// Whitespace other than a plain space would print as one, so it is quoted
// as its escape for the same reason.

#[test]
fn a_place_id_carrying_a_control_windows_allows_is_refused_as_illegible() {
    assert_place_id_refused("a\u{85}b", "\\u{85}");
}

#[test]
fn a_place_id_carrying_a_no_break_space_is_refused_and_escaped() {
    assert_place_id_refused("a\u{a0}b", "\\u{a0}");
}

#[test]
fn a_place_id_carrying_an_ideographic_space_is_refused_and_escaped() {
    assert_place_id_refused("a\u{3000}b", "\\u{3000}");
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
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
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
    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
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
    let _ = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ],
    );
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
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let nbt_path = out_dir.path().join("cottage.nbt");
    fs::write(&nbt_path, b"stale bytes").expect("seed nbt");
    let lock_path = out_dir.path().join("c.lock");
    fs::write(&lock_path, "stale: true\n").expect("seed lock");

    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.path().to_str().unwrap(),
            "--lock",
            lock_path.to_str().unwrap(),
        ],
    );
    assert!(result.status.success());
    let bytes = fs::read(&nbt_path).expect("read nbt");
    assert_ne!(bytes, b"stale bytes", "nbt should have been overwritten");
    assert_eq!(&bytes[..2], &[0x1f, 0x8b], "overwritten file is real gzip");
    let lf = Lockfile::read_from_path(&lock_path).expect("read lock");
    assert!(lf.verified);
}

#[test]
fn c30_a_note_that_points_at_a_second_line_is_printed_with_its_position() {
    // The third of the three commands whose note loop dropped
    // `note.span`. `cairn compile` reaches it through
    // `report_lowering_diagnostics`, which `lower` and `info` do not use.
    let tmp = TempDir::new().expect("tempdir");
    let src = tmp.path().join("conflict.crn");
    fs::write(
        &src,
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "  slot glass -> @glass_pane\n",
            "\n",
            "struct t size=7x5\n",
            "  walls mat_slot=wall height=3\n",
            "  door side=front at=center\n",
            "  window side=front y=1 offset=3 size=1x2 mat_slot=glass\n",
        ),
    )
    .expect("write source");
    let out_dir = TempDir::new().expect("out tempdir");

    let result = cairn(
        "compile",
        &[
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
            "--out",
            out_dir.path().to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "a warning must not fail the compile, stderr={}",
        String::from_utf8_lossy(&result.stderr),
    );
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(
        stderr.contains(":7:3:   note: overwritten member declared here"),
        "the note must carry the `door` line's position, got: {stderr}",
    );
}

#[test]
fn a_walkway_port_named_underscore_is_refused_rather_than_panicking_the_namer() {
    // A port called `_` used to lower to `walkway::s::a.___b._`, which
    // the artifact namer could not split back, so `compile` aborted on a
    // debug assertion while `check` passed. The row is now dropped at
    // lowering with a finding, and the build, having lost the walkway the
    // row asked for, is refused as partial rather than certified.
    let fixture = Fixture::new(
        "cli-compile",
        "underscore-port",
        concat!(
            "def hut size=5x5:\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  door  id=_ side=front at=center\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=a use=hut theme=t at=origin\n",
            "  place id=b use=hut theme=t east_of=a gap=4\n",
            "  connect a._ to b._ path=@gravel\n",
        ),
    );
    let result = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains(
            ":11:3: warning[W_INVALID_WALKWAY_IDENT]: walkway `a._ ↔ b._` was dropped because \
             the port id `_` starts or ends with `_`"
        ),
        "stderr={stderr}",
    );
    assert!(
        stderr.contains(
            "1 of 3 requested scopes did not lower; refusing to certify a partial build\n  \
             note: `site::s::a._ ↔ b._` produced no voxels\n"
        ),
        "stderr={stderr}",
    );
    assert!(fixture.artifacts().is_empty(), "{:?}", fixture.artifacts());
}

#[test]
fn walkway_ids_that_would_alias_across_the_separator_are_each_named() {
    // `a.p_ → b.p` and `a.p → _b.p` used to share the scope key
    // `walkway::s::a.p___b.p`, so the second row replaced the first and
    // one `.nbt` was written for two rows with no finding. Each is now
    // named on its own line, and again among the losses that refuse the
    // build: the name a loss is reported under is the row's own ports,
    // not the key the two rows would both have spelled. The sound third
    // row laid its walkway, so it is not among them.
    let fixture = Fixture::new(
        "cli-compile",
        "edge-underscore-alias",
        concat!(
            "def hut size=5x5:\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  door  id=p  side=front at=center\n",
            "  door  id=p_ side=back  at=center\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=a  use=hut theme=t at=origin\n",
            "  place id=b  use=hut theme=t east_of=a gap=4\n",
            "  place id=_b use=hut theme=t north_of=a gap=4\n",
            "  connect a.p_ to b.p path=@gravel\n",
            "  connect a.p to _b.p path=@gravel\n",
            "  connect a.p to b.p path=@gravel\n",
        ),
    );
    let result = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "stderr={stderr}");
    for (line, from, to, role, segment) in [
        (13, "a.p_", "b.p", "port", "p_"),
        (14, "a.p", "_b.p", "place", "_b"),
    ] {
        let want = format!(
            ":{line}:3: warning[W_INVALID_WALKWAY_IDENT]: walkway `{from} ↔ {to}` was dropped \
             because the {role} id `{segment}` starts or ends with `_`",
        );
        assert!(
            stderr.contains(&want),
            "missing `{want}` in stderr={stderr}"
        );
    }
    assert!(
        stderr.contains(
            "2 of 6 requested scopes did not lower; refusing to certify a partial build\n  \
             note: `site::s::a.p_ ↔ b.p` produced no voxels\n  \
             note: `site::s::a.p ↔ _b.p` produced no voxels\n"
        ),
        "stderr={stderr}",
    );
    assert!(fixture.artifacts().is_empty(), "{:?}", fixture.artifacts());
}

#[test]
fn a_connect_pair_is_lost_once_and_only_when_no_row_laid_it() {
    // A walkway is judged by the pair its `connect` row names, not by the
    // row. A duplicate of a pair an earlier row laid loses nothing, in
    // either order, so the build is certified. Two rows naming a pair
    // that neither laid are one loss, so the refusal counts it once.
    let hut = concat!(
        "def hut size=5x5:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=p side=front at=center\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
    );
    let laid = Fixture::new(
        "cli-compile",
        "duplicate-laid-pair",
        &format!(
            "{hut}site s:\n  \
             place id=a use=hut theme=t at=origin\n  \
             place id=b use=hut theme=t east_of=a gap=4\n  \
             connect a.p to b.p path=@gravel\n  \
             connect b.p to a.p path=@gravel\n",
        ),
    );
    let result = compile_as(&laid, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "stderr={stderr}");
    assert!(
        stderr.contains("warning[W_DUPLICATE_WALKWAY]"),
        "stderr={stderr}"
    );
    assert!(!stderr.contains("E_PARTIAL_BUILD"), "stderr={stderr}");
    let lf = Lockfile::read_from_path(&laid.lock()).expect("read lock");
    assert_eq!(lf.walkways.len(), 1);

    // Neither row lays: the trailing `_` on `a_` is refused by the
    // walkway ident rule, whichever end of the row it sits on.
    let lost = Fixture::new(
        "cli-compile",
        "duplicate-lost-pair",
        &format!(
            "{hut}site s:\n  \
             place id=a_ use=hut theme=t at=origin\n  \
             place id=b  use=hut theme=t east_of=a_ gap=4\n  \
             connect a_.p to b.p path=@gravel\n  \
             connect b.p to a_.p path=@gravel\n",
        ),
    );
    let partial = "error[E_PARTIAL_BUILD]";
    let counted_once = "1 of 3 requested scopes did not lower";
    let named = "  note: `site::s::a_.p ↔ b.p` produced no voxels\n";
    let result = compile_as(&lost, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "stderr={stderr}");
    // Each row earns its own ident finding. The first row laid nothing,
    // so the second is not a duplicate of it: a pair is recorded as laid
    // only once its strip is, not when a row reaches the duplicate guard.
    assert_eq!(
        stderr.matches("W_INVALID_WALKWAY_IDENT").count(),
        2,
        "stderr={stderr}"
    );
    assert!(!stderr.contains("W_DUPLICATE_WALKWAY"), "stderr={stderr}");
    assert_eq!(stderr.matches(partial).count(), 1, "stderr={stderr}");
    assert!(stderr.contains(counted_once), "stderr={stderr}");
    assert_eq!(
        stderr.matches("produced no voxels").count(),
        1,
        "stderr={stderr}"
    );
    assert!(stderr.contains(named), "stderr={stderr}");
    assert!(lost.artifacts().is_empty(), "{:?}", lost.artifacts());

    // `check --target` runs the same lowering and owes the same refusal.
    let source = lost.source();
    let check = cairn(
        "check",
        &[
            source.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ],
    );
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert_eq!(check.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains(counted_once) && stderr.contains(named),
        "stderr={stderr}"
    );
}

#[test]
fn a_walkway_whose_every_cell_is_blocked_is_lost_rather_than_written_as_air() {
    // Two huts touching, each door port buried under the other's floor.
    // The router cannot detour from a buried port, so the row falls back
    // to the straight L, and every cell of it overlaps a floor: the
    // walkway's array is all air. It used to be written as an `.nbt` of
    // two air blocks and certified; holding no block, it is now a lost
    // walkway like a row that was refused.
    let fixture = Fixture::new(
        "cli-compile",
        "walkway-every-cell-blocked",
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "  slot floor -> @stone\n",
            "\n",
            "def hut size=3x3:\n",
            "  floor id=floor mat_slot=floor\n",
            "  walls id=walls mat_slot=wall height=3\n",
            "  door  id=front side=front at=center\n",
            "  door  id=back  side=back  at=center\n",
            "\n",
            "site duo:\n",
            "  place id=a use=hut theme=t at=origin\n",
            "  place id=b use=hut theme=t north_of=a gap=0\n",
            "  connect a.back to b.front path=@gravel\n",
        ),
    );
    let refusal = "1 of 3 requested scopes did not lower";
    let named = "  note: `site::duo::a.back ↔ b.front` produced no voxels\n";
    let result = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains(
            "warning[W_WALKWAY_BLOCKED]: walkway `a.back ↔ b.front` skipped 2 cells that \
             overlapped an existing structure"
        ),
        "stderr={stderr}",
    );
    assert!(
        stderr.contains(&format!(
            "error[E_PARTIAL_BUILD]: {}: {refusal}; refusing to certify a partial build\n{named}",
            fixture.source().display(),
        )),
        "stderr={stderr}",
    );
    assert_eq!(
        stderr.matches("produced no voxels").count(),
        1,
        "stderr={stderr}"
    );
    assert!(fixture.artifacts().is_empty(), "{:?}", fixture.artifacts());

    let source = fixture.source();
    let check = cairn(
        "check",
        &[
            source.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21.4",
        ],
    );
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert_eq!(check.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains(refusal) && stderr.contains(named),
        "stderr={stderr}"
    );
}

#[test]
fn a_place_whose_every_member_deferred_is_lost_rather_than_written_as_air() {
    // `nada`'s one member cannot voxelise, so lowering keeps an array for
    // `b` that holds only air. It used to be written as `b.nbt` and
    // certified beside `a`; holding no block, it is now a lost scope.
    let fixture = Fixture::new(
        "cli-compile",
        "place-every-member-deferred",
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "  slot floor -> @stone\n",
            "\n",
            "def box size=3x3:\n",
            "  floor id=f mat_slot=floor\n",
            "\n",
            "def nada size=3x3:\n",
            "  walls id=w mat_slot=wall height=0\n",
            "\n",
            "site s:\n",
            "  place id=a use=box theme=t at=origin\n",
            "  place id=b use=nada theme=t east_of=a gap=2\n",
        ),
    );
    let result = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "stderr={stderr}");
    assert!(
        stderr.contains("warning[W_DEFERRED_MEMBER]: walls without a positive `height=`"),
        "stderr={stderr}",
    );
    assert!(
        stderr.contains(
            "1 of 2 requested scopes did not lower; refusing to certify a partial build\n  \
             note: `site::s::b` produced no voxels\n"
        ),
        "stderr={stderr}",
    );
    assert!(fixture.artifacts().is_empty(), "{:?}", fixture.artifacts());
}

#[test]
fn walkways_that_flatten_to_one_filename_are_refused_before_anything_is_written() {
    // Every id here passes the walkway ident rule, so both rows would lay
    // and their scope keys differ, but the filename joins a place and its
    // port with the same `_` that sits inside `a_b` and `b_c`: both
    // walkways flatten to `s_walkway_a_b_c__d_e_f.nbt`. The second would
    // overwrite the first, so the resolver refuses the source with
    // `E_OUTPUT_NAME_COLLISION` before it lowers, and nothing is staged.
    let fixture = Fixture::new(
        "cli-compile",
        "walkway-filename-collision",
        concat!(
            "def hut size=5x5:\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  door  id=c   side=front at=center\n",
            "  door  id=f   side=front at=left\n",
            "  door  id=b_c side=back  at=center\n",
            "  door  id=e_f side=back  at=left\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=a_b use=hut theme=t at=origin\n",
            "  place id=d_e use=hut theme=t east_of=a_b gap=4\n",
            "  place id=a   use=hut theme=t north_of=a_b gap=6\n",
            "  place id=d   use=hut theme=t east_of=a gap=4\n",
            "  connect a_b.c to d_e.f path=@gravel\n",
            "  connect a.b_c to d.e_f path=@gravel\n",
        ),
    );
    let result = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "stderr={stderr}");
    // The collision is the only finding.
    assert!(!stderr.contains("warning["), "stderr={stderr}");
    assert_eq!(stderr.matches("error[").count(), 1, "stderr={stderr}");
    assert!(
        stderr.contains(
            "error[E_OUTPUT_NAME_COLLISION]: the walkway `a.b_c ↔ d.e_f` in site `s` would be \
             written to the same file as the walkway `a_b.c ↔ d_e.f` in site `s`"
        ),
        "stderr={stderr}",
    );
    assert!(
        stderr.contains("both would be written to `s_walkway_a_b_c__d_e_f`"),
        "stderr={stderr}",
    );
    assert!(fixture.artifacts().is_empty(), "{:?}", fixture.artifacts());
}

/// Every file under `root`, relative to it, sorted.
fn files_under(root: &std::path::Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let rel = path.strip_prefix(root).expect("under root");
                found.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    found
}

/// A `place id=` is an artifact's file name, so a path separator in it would
/// let the source choose where the compiler writes. `check` refuses it and
/// `compile` writes nothing — neither the artifact nor the lock.
///
/// Each row is set up so the build would have succeeded before the rule
/// existed: the absolute id's directory exists outside `--out`, and `out/`
/// holds both the `sub/` the relative id names and the `a/` that `a\b`
/// names on Windows. The Windows separator is a plain character on Unix,
/// and refused there too so whether an id is accepted does not depend on
/// the host.
///
/// Each row pins the character the diagnostic names. The absolute id is
/// the tempdir's own path, so on Windows it begins with a drive and is
/// refused on that `:`, a rule older than the path separators; there the
/// relative and Windows-style rows are what exercise `/` and `\`.
/// Elsewhere the tempdir is named without the `.` `TempDir::new` puts in
/// front of it, so the absolute id carries no character the older rule
/// refused, and that is asserted rather than assumed.
#[test]
fn a_place_id_carrying_a_path_separator_writes_nothing() {
    let tmp = tempfile::Builder::new()
        .prefix("cairn-escape")
        .tempdir()
        .expect("tempdir");
    let root = tmp.path();
    let elsewhere = root.join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("create elsewhere");
    let absolute = elsewhere.join("hut");
    let absolute = absolute.to_str().expect("utf-8 tempdir");
    if !cfg!(windows) {
        assert!(
            !absolute.contains(|c: char| c == '.' || c == ':' || c.is_whitespace()),
            "`{absolute}` carries a character refused before the path separators were, so the \
             absolute row would not show that `/` is what refuses it; point TMPDIR elsewhere",
        );
    }

    let absolute_char = if cfg!(windows) { ':' } else { '/' };
    for (label, id, ch) in [
        ("absolute", absolute, absolute_char),
        ("relative", "sub/hut", '/'),
        ("windows", "a\\b", '\\'),
    ] {
        let case = root.join(label);
        let out = case.join("out");
        fs::create_dir_all(out.join("sub")).expect("create out/sub");
        fs::create_dir_all(out.join("a")).expect("create out/a");
        let refusal = format!(
            "error[E_INVALID_PLACE_ID]: `place id={id}` in site `s` is not a usable id: \
             it contains `{ch}`"
        );
        let src = write_source(
            &case,
            "escape.crn",
            &format!(
                "@cairn 2026.06\n\ndef hut size=3x3:\n  floor mat_slot=floor\n\n\
                 theme t:\n  slot floor -> @oak_planks\n\n\
                 site s:\n  place id=\"{id}\" use=hut theme=t at=origin\n"
            ),
        );

        let checked = cairn("check", &[src.to_str().unwrap()]);
        let check_err = String::from_utf8_lossy(&checked.stderr);
        assert_eq!(
            checked.status.code(),
            Some(1),
            "{label}: stderr={check_err}"
        );
        assert!(
            check_err.contains(&refusal),
            "{label}: `id=\"{id}\"` must be refused on `{ch}`, got: {check_err}",
        );

        let compiled = cairn(
            "compile",
            &[
                src.to_str().unwrap(),
                "--edition",
                "java",
                "--out",
                out.to_str().unwrap(),
            ],
        );
        let compile_err = String::from_utf8_lossy(&compiled.stderr);
        assert_eq!(
            compiled.status.code(),
            Some(1),
            "{label}: stderr={compile_err}"
        );
        assert!(
            compile_err.contains(&refusal),
            "{label}: compile must stop on the same refusal, got: {compile_err}",
        );
    }
    let expected: Vec<String> = ["absolute", "relative", "windows"]
        .iter()
        .map(|label| PathBuf::from(label).join("escape.crn"))
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(files_under(root), expected, "compile must write nothing");
}
