//! `cairn compile` end to end: the artifacts each edition writes, the lockfile that certifies
//! them, and the findings that stop a build or ride along.

use std::fs;
use std::path::PathBuf;

use cairn_lang_core::Edition;
use cairn_lang_core::lock::{HashHex, Lockfile};
use tempfile::TempDir;

use crate::common::{
    Fixture, assert_stream, bytes_contain, cairn, compile_as, crn_examples, example_in_tempdir,
    examples_dir, nbt_body, write_source,
};

/// From cargo, not `CAIRN_VERSION`, which would only restate what the binary wrote.
const WORKSPACE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn compile_cottage_with_explicit_target_exits_zero() {
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
fn compile_writes_named_nbt_into_out_dir() {
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
fn default_lock_is_written_beside_source() {
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
fn explicit_lock_path_is_honored() {
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
fn lockfile_records_cairn_version() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("version.lock");
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
fn lockfile_records_target_triple() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("target.lock");
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
    assert_eq!(lf.target.edition, Edition::Java);
    assert_eq!(lf.target.mc_version, "1.21.4");
    assert_eq!(lf.target.data_version, 4189);
}

/// Java's `1.21` row reached as `1.21.0`, and Bedrock's `1.21.0` as `1.21`.
#[test]
fn either_spelling_of_a_target_builds_the_row_it_names() {
    let (_tmp_src, src) = example_in_tempdir("roof-flat.crn");
    let lock_target = |edition: &str, target: &str| {
        let out_dir = TempDir::new().expect("out tempdir");
        let lock_path = out_dir.path().join("spelling.lock");
        let result = cairn(
            "compile",
            &[
                src.to_str().unwrap(),
                "--edition",
                edition,
                "--target",
                target,
                "--out",
                out_dir.path().to_str().unwrap(),
                "--lock",
                lock_path.to_str().unwrap(),
            ],
        );
        assert!(
            result.status.success(),
            "{edition} `--target {target}`: {}",
            String::from_utf8_lossy(&result.stderr),
        );
        Lockfile::read_from_path(&lock_path)
            .expect("read lock")
            .target
    };

    let java_as_table = lock_target("java", "1.21");
    let java_padded = lock_target("java", "1.21.0");
    assert_eq!(
        java_padded, java_as_table,
        "java `1.21.0` must build the `1.21` row"
    );
    assert_eq!(java_padded.mc_version, "1.21");

    let bedrock_as_table = lock_target("bedrock", "1.21.0");
    let bedrock_trimmed = lock_target("bedrock", "1.21");
    assert_eq!(
        bedrock_trimmed, bedrock_as_table,
        "bedrock `1.21` must build the `1.21.0` row"
    );
    assert_eq!(bedrock_trimmed.mc_version, "1.21.0");
}

#[test]
fn bedrock_compiles_stateless_example_to_mcstructure() {
    // `roof-flat.crn` resolves entirely to stateless block ids.
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
fn bedrock_mcstructure_is_uncompressed_little_endian() {
    // Raw NBT (`0x0a`, then a zero `u16` name length), never gzip (`0x1f 0x8b`) as `.nbt` is.
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
fn bedrock_lockfile_records_bedrock_target_and_pack_hash() {
    let tmp = TempDir::new().expect("tempdir");
    let dst = tmp.path().join("roof-flat.crn");
    fs::copy(examples_dir().join("roof-flat.crn"), &dst).expect("copy roof-flat");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("bedrock.lock");
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
    assert_eq!(lf.target.edition, Edition::Bedrock);
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
fn bedrock_cottage_maps_stair_states_without_degradation() {
    // Bedrock drops the gable stairs' `shape=straight`, its default, without a note.
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
fn bedrock_themed_tower_degrades_stair_shape_but_compiles() {
    // The eave's one `shape=outer_left` stair has no Bedrock state.
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
    let degraded: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with("warning[W_INTENT_DEGRADED]"))
        .collect();
    let quoted = "warning[W_INTENT_DEGRADED]: struct::keep: \
        minecraft:dark_oak_stairs[facing=south,half=top,shape=outer_left]: Bedrock has no `shape` \
        state; its stairs render straight, so corners show visual gaps";
    assert_eq!(
        degraded,
        [quoted],
        "the line `spec/versioning-editions` \"Java / Bedrock portability\" quotes: {stderr}",
    );
    assert!(
        out_dir.path().join("keep.mcstructure").exists(),
        "the mcstructure artifact should still be written on a degradation",
    );
}

#[test]
fn bedrock_unknown_target_names_bedrock_versions() {
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
fn missing_edition_flag_exits_two_with_usage() {
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let result = cairn("compile", &[src.to_str().unwrap()]);
    // clap reports missing required args as exit 2.
    assert_eq!(result.status.code(), Some(2));
    let stderr = String::from_utf8(result.stderr).expect("utf-8");
    assert!(stderr.contains("--edition"), "expected --edition mention");
}

#[test]
fn unknown_target_exits_one_with_supported_list() {
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
fn parse_error_surfaces_file_line_col() {
    let tmp = TempDir::new().expect("tempdir");
    let bad = tmp.path().join("bad.crn");
    // `;;;` is no Cairn token.
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
    // The `file:line:col:` shape gcc and clang print.
    let mut found_position = false;
    for line in stderr.lines() {
        if !line.contains(&bad_display) {
            continue;
        }
        let rest = &line[line.find(&bad_display).unwrap() + bad_display.len()..];
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
fn default_out_dir_is_source_parent() {
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
    assert_eq!(
        lock_bytes_a,
        lock_bytes_b,
        "regenerating the lockfile is not a no-op:\n{}\n{}",
        String::from_utf8_lossy(&lock_bytes_a),
        String::from_utf8_lossy(&lock_bytes_b),
    );

    let lf_a = Lockfile::read_from_path(&lock_a).expect("read a");
    let lf_b = Lockfile::read_from_path(&lock_b).expect("read b");
    assert_eq!(lf_a, lf_b);
}

#[test]
fn compile_rolls_back_on_lockfile_failure() {
    // `--lock` at an existing directory forces the lockfile write to fail.
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

/// `(example, the artifact its one struct writes)` for every example with no `W_DEFERRED_MEMBER`.
const DEFER_FREE_EXAMPLES: &[(&str, &str)] = &[
    ("cottage.crn", "cottage.nbt"),
    ("themed-tower.crn", "keep.nbt"),
    ("redstone-door.crn", "gatehouse.nbt"),
    ("roof-shed.crn", "roof_shed.nbt"),
    ("roof-hip.crn", "roof_hip.nbt"),
    ("roof-flat.crn", "roof_flat.nbt"),
];

#[test]
fn covered_examples_compile_without_deferred_warnings() {
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
fn lockfile_hashes_are_populated() {
    // `constraint_catalog_hash` is zero while there is no catalog.
    let (_tmp_src, src) = example_in_tempdir("cottage.crn");
    let out_dir = TempDir::new().expect("out tempdir");
    let lock_path = out_dir.path().join("pack.lock");
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
    assert_ne!(
        lf.resolved_ir_hash,
        HashHex::zero(),
        "resolved_ir_hash must be the hash of the IR the compile lowered",
    );
    assert_eq!(
        lf.inputs.constraint_catalog_hash.as_str(),
        HashHex::ZERO_STR,
        "constraint_catalog_hash stays zero until its own ingest lands",
    );
}

#[test]
fn unknown_abstract_token_compile_exits_nonzero() {
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
    assert_stream(&stderr, &[(4, 17, "error", "E_UNKNOWN_ABSTRACT_TOKEN")]);
    assert!(
        stderr.contains("floor.wood.broadleaf"),
        "stderr should surface the nearest declared token; got: {stderr}",
    );
}

#[test]
fn compile_village_emits_three_nbt() {
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
fn village_lockfile_records_placements() {
    // home2: home1's inflated width 11 plus gap=4; home3: minus its depth 9 minus gap=5.
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
fn village_lockfile_round_trips_through_yaml() {
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
fn village_lowers_connect_rows_into_walkway_artifacts() {
    // The home1↔home3 row detours round home1's floor, so nothing is blocked.
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
fn unknown_def_in_place_errors_with_suggestion() {
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
    assert_stream(
        &reported,
        &[
            (3, 1, "warning", "W_UNUSED_DEF"),
            (10, 3, "error", "E_UNRESOLVED_PLACE_REF"),
        ],
    );
    assert!(
        reported.contains("did you mean `cottage`?"),
        "reported should suggest the closest def name; got: {reported}",
    );
}

#[test]
fn unknown_theme_in_place_errors_with_suggestion() {
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
    assert_stream(&reported, &[(10, 3, "error", "E_UNRESOLVED_THEME_REF")]);
    assert!(
        reported.contains("did you mean `medieval`?"),
        "reported should suggest the closest theme name; got: {reported}",
    );
}

#[test]
fn east_of_unknown_ref_errors_with_suggestion() {
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
    assert_stream(&reported, &[(11, 46, "error", "E_UNRESOLVED_PLACE_REF")]);
    assert!(
        reported.contains("did you mean `home1`?"),
        "reported should suggest the closest prior place id; got: {reported}",
    );
}

/// One `def hut`, one theme and a site whose rows are `rows`, the first on line 10.
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

/// Every finding in `reported` as `(line, code)`, in print order.
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

/// The last row references an id no row declares, which stays unresolved.
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
            (12, "E_DUPLICATE_PLACE_ID"),
            (13, "W_DEFERRED_PLACE"),
        ]),
        "reported={reported}",
    );
    assert_eq!(
        note_rows(&reported),
        [11, 11],
        "`E_DUPLICATE_PLACE_ID` and the cascade both point at the first row; reported={reported}",
    );
}

/// `place id=ID` refused on `shown` by `check`, and by `compile`, which writes nothing.
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
    assert_stream(&check_err, &[(10, 3, "error", "E_INVALID_PLACE_ID")]);
    assert!(
        check_err.contains(&refusal),
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

// Characters a Windows file name cannot carry are refused on every host. A source string literal
// cannot carry `"`, so the lockfile reader is where that one is reached.

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

// Controls past U+001F and whitespace other than a space print wrongly, so they are refused too.

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
fn bare_def_without_place_emits_w_unused_def_and_no_nbt() {
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
    assert_stream(&stderr, &[(3, 1, "warning", "W_UNUSED_DEF")]);
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
fn unknown_def_in_place_compile_exits_nonzero() {
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
    assert_stream(
        &stderr,
        &[
            (3, 1, "warning", "W_UNUSED_DEF"),
            (10, 3, "error", "E_UNRESOLVED_PLACE_REF"),
        ],
    );
    assert!(
        stderr.contains("did you mean `cottage`?"),
        "stderr should propagate the nearest-match note; got: {stderr}",
    );
}

#[test]
fn home2_nbt_uses_resolved_theme_palette() {
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
    let body = nbt_body(&out_dir.path().join("home2.nbt"));
    for id in [
        "minecraft:oak_planks",
        "minecraft:cobblestone",
        "minecraft:spruce_stairs",
        "minecraft:glass_pane",
    ] {
        assert!(
            bytes_contain(&body, id),
            "home2.nbt has no `{id}` in its palette"
        );
    }
}

#[test]
fn compile_overwrites_existing_output() {
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
fn a_note_that_points_at_a_second_line_is_printed_with_its_position() {
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
    // A port called `_` would lower to a scope key the artifact namer cannot split back.
    let fixture = Fixture::new(concat!(
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
    ));
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
    // `a.p_ → b.p` and `a.p → _b.p` would share one scope key; the third row is sound.
    let fixture = Fixture::new(concat!(
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
    ));
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
    // A walkway is judged by the pair its row names, so a duplicate of a laid pair loses nothing.
    let hut = concat!(
        "def hut size=5x5:\n",
        "  walls id=w mat_slot=wall height=3\n",
        "  door  id=p side=front at=center\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
    );
    let laid = Fixture::new(&format!(
        "{hut}site s:\n  \
             place id=a use=hut theme=t at=origin\n  \
             place id=b use=hut theme=t east_of=a gap=4\n  \
             connect a.p to b.p path=@gravel\n  \
             connect b.p to a.p path=@gravel\n",
    ));
    let result = compile_as(&laid, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "stderr={stderr}");
    assert_stream(&stderr, &[(12, 3, "warning", "W_DUPLICATE_WALKWAY")]);
    assert!(!stderr.contains("E_PARTIAL_BUILD"), "stderr={stderr}");
    let lf = Lockfile::read_from_path(&laid.lock()).expect("read lock");
    assert_eq!(lf.walkways.len(), 1);

    // Neither row lays: the walkway ident rule refuses the trailing `_` on `a_`.
    let lost = Fixture::new(&format!(
        "{hut}site s:\n  \
             place id=a_ use=hut theme=t at=origin\n  \
             place id=b  use=hut theme=t east_of=a_ gap=4\n  \
             connect a_.p to b.p path=@gravel\n  \
             connect b.p to a_.p path=@gravel\n",
    ));
    let counted_once = "1 of 3 requested scopes did not lower";
    let named = "  note: `site::s::a_.p ↔ b.p` produced no voxels\n";
    let result = compile_as(&lost, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "stderr={stderr}");
    assert_stream(
        &stderr,
        &[
            (11, 3, "warning", "W_INVALID_WALKWAY_IDENT"),
            (12, 3, "warning", "W_INVALID_WALKWAY_IDENT"),
            (0, 0, "error", "E_PARTIAL_BUILD"),
        ],
    );
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
    // Two huts touching, each door port buried under the other's floor: the walkway is all air.
    let fixture = Fixture::new(concat!(
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
    ));
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
    // `nada`'s one member cannot voxelise, so `b`'s array holds only air.
    let fixture = Fixture::new(concat!(
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
    ));
    let result = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "stderr={stderr}");
    assert_stream(
        &stderr,
        &[
            (9, 3, "warning", "W_DEFERRED_MEMBER"),
            (0, 0, "error", "E_PARTIAL_BUILD"),
        ],
    );
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
    // Both walkways flatten to `s_walkway_a_b_c__d_e_f.nbt`, though their scope keys differ.
    let fixture = Fixture::new(concat!(
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
    ));
    let result = compile_as(&fixture, "java", "1.21.4");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(1), "stderr={stderr}");
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

/// Without the rule each row would build: the absolute id's directory exists, and `out/` holds
/// the `sub/` and `a/` the others name. The tempdir is named without a `.`, so the absolute id
/// carries no other refused character.
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
