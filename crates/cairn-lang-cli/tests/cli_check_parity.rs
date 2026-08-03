//! Parity between `cairn check` and the commands that build on it.
//!
//! `check` is the gate the other subcommands are supposed to sit behind:
//! a source it rejects with an `Error`-severity diagnostic must not lower,
//! must not report portability figures, and must not compile. Before this
//! suite existed only `cairn synth` ran the check pass, so `cairn compile`
//! happily wrote artifacts — and a `verified: true` lockfile — for a file
//! `cairn check` had just rejected with exit 1.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Shared prologue so each fixture below differs only in the construct
/// that trips its diagnostic.
const HEADER: &str = "@cairn 2026.06\n\
@requires version>=1.20\n\
\n\
theme t:\n\
\x20\x20slot floor -> @oak_planks\n\
\x20\x20slot wall  -> @cobblestone\n\
\n";

/// One fixture per `Error`-severity code the syntactic check passes emit,
/// as `(code, source)`.
///
/// `cairn_lang_core::check`'s `syntactic_error_codes_match_the_cli_parity_fixtures`
/// unit test holds the authoritative list behind an exhaustive `match`, so a
/// code cannot join that set without a compile error pointing back here.
const FIXTURES: &[(&str, &str)] = &[
    (
        "E_DUPLICATE_SIZE",
        "struct s size=5x5 size=6x6\n  floor mat_slot=floor\n",
    ),
    (
        "E_DUPLICATE_SLOT",
        // Declares its own theme: the shared header binds each slot once.
        "theme dup:\n  slot wall -> @cobblestone\n  slot wall -> @stone_bricks\n\
         \nstruct s size=5x5\n  walls mat_slot=wall height=3\n",
    ),
    (
        "E_DUPLICATE_ARG",
        "struct s size=5x5\n  floor id=a id=b mat_slot=floor\n",
    ),
    (
        "E_DUPLICATE_ID",
        "struct s size=5x5\n  floor id=x mat_slot=floor\n\
         \x20\x20walls id=x class=outer mat_slot=wall height=3\n",
    ),
    (
        "E_UNKNOWN_KEYWORD",
        "struct s size=5x5\n  torch mat_slot=wall\n",
    ),
    (
        "E_TYPE_MISMATCH_LABEL",
        "struct s size=5x5\n  floor id=1 mat_slot=floor\n",
    ),
    (
        "E_TYPE_MISMATCH_SIZE",
        "struct s size=abc\n  floor mat_slot=floor\n",
    ),
    (
        "E_CONNECT_ARITY",
        "def hut size=3x3:\n  floor id=floor mat_slot=floor\n\
         \x20\x20door id=entry side=front at=center\n\
         \nsite s:\n  place id=a use=hut theme=t at=origin\n\
         \x20\x20place id=b use=hut theme=t east_of=a gap=4\n\
         \x20\x20connect a.entry b.entry path=@gravel\n",
    ),
];

/// Write one fixture into its own directory, so a `compile` run that
/// (incorrectly) emits artifacts cannot be confused with another's.
fn write_fixture(root: &Path, code: &str, body: &str) -> PathBuf {
    let dir = root.join(code);
    fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join("fixture.crn");
    fs::write(&path, format!("{HEADER}{body}")).expect("write fixture");
    path
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(cargo_bin())
        .args(args)
        .output()
        .expect("failed to invoke cairn binary")
}

fn exit_code(out: &std::process::Output) -> i32 {
    out.status.code().expect("process exited via a signal")
}

#[test]
fn parity_1_check_rejects_every_fixture_with_its_own_code() {
    let tmp = TempDir::new().expect("tempdir");
    for (code, body) in FIXTURES {
        let path = write_fixture(tmp.path(), code, body);
        let out = run(&["check", path.to_str().unwrap(), "--format", "json"]);
        assert_eq!(
            exit_code(&out),
            1,
            "{code}: check should reject the fixture; stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        let stdout = String::from_utf8(out.stdout).expect("utf-8");
        let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
        let reported: Vec<&str> = parsed
            .as_array()
            .expect("array of diagnostics")
            .iter()
            .filter(|d| d["severity"] == "error")
            .filter_map(|d| d["code"].as_str())
            .collect();
        assert!(
            reported.contains(code),
            "{code}: expected it among the reported error codes, got {reported:?}",
        );
    }
}

#[test]
fn parity_2_lower_rejects_every_check_error() {
    let tmp = TempDir::new().expect("tempdir");
    for (code, body) in FIXTURES {
        let path = write_fixture(tmp.path(), code, body);
        let out = run(&["lower", path.to_str().unwrap()]);
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(
            exit_code(&out),
            1,
            "{code}: lower accepted a source check rejects; stderr={stderr}",
        );
        assert!(
            stderr.contains(code),
            "{code}: lower must name the code on stderr, got {stderr}",
        );
    }
}

#[test]
fn parity_3_info_rejects_every_check_error() {
    let tmp = TempDir::new().expect("tempdir");
    for (code, body) in FIXTURES {
        let path = write_fixture(tmp.path(), code, body);
        let out = run(&["info", path.to_str().unwrap()]);
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(
            exit_code(&out),
            1,
            "{code}: info accepted a source check rejects; stderr={stderr}",
        );
        assert!(
            stderr.contains(code),
            "{code}: info must name the code on stderr, got {stderr}",
        );
    }
}

#[test]
fn parity_4_compile_rejects_every_check_error_and_writes_nothing() {
    let tmp = TempDir::new().expect("tempdir");
    for (code, body) in FIXTURES {
        let path = write_fixture(tmp.path(), code, body);
        let out_dir = path.parent().expect("fixture dir").join("out");
        let out = run(&[
            "compile",
            path.to_str().unwrap(),
            "--edition",
            "java",
            "--out",
            out_dir.to_str().unwrap(),
        ]);
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(
            exit_code(&out),
            1,
            "{code}: compile accepted a source check rejects; stderr={stderr}",
        );
        assert!(
            stderr.contains(code),
            "{code}: compile must name the code on stderr, got {stderr}",
        );
        let lock = path.with_extension("crn.lock");
        assert!(
            !lock.exists(),
            "{code}: a refused compile must not certify the source with {}",
            lock.display(),
        );
        let artifacts: Vec<_> = fs::read_dir(&out_dir)
            .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default();
        assert!(
            artifacts.is_empty(),
            "{code}: a refused compile must not leave artifacts, found {artifacts:?}",
        );
    }
}

#[test]
fn parity_5_compile_announces_a_build_that_produced_no_structures() {
    // Templates and themes alone legitimately compile to nothing, so this is
    // a warning and not a refusal — `c26_bare_def_without_place_emits_w_unused
    // _def_and_no_nbt` pins that contract. What it must not be is silent: the
    // exit code cannot distinguish a template library from a build whose
    // structures all fell out along the way, and the lockfile written beside
    // it reads as a certification either way.
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().join("empty.crn");
    fs::write(&path, "").expect("write empty source");
    let out_dir = tmp.path().join("out");
    let out = run(&[
        "compile",
        path.to_str().unwrap(),
        "--edition",
        "java",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(exit_code(&out), 0, "stderr={stderr}");
    assert!(
        stderr.contains("no structures were produced"),
        "expected a warning naming the empty result, got {stderr}",
    );
}

#[test]
fn parity_6_every_example_still_passes_all_four_commands() {
    let tmp = TempDir::new().expect("tempdir");
    let mut seen = 0usize;
    for entry in fs::read_dir(examples_dir()).expect("read examples") {
        let src = entry.expect("dir entry").path();
        if src.extension().and_then(|e| e.to_str()) != Some("crn") {
            continue;
        }
        seen += 1;
        let name = src.file_name().expect("file name").to_owned();
        // Copy into the tempdir: `compile` writes its lockfile next to the
        // source, and `examples/` is not ours to dirty.
        let work = tmp.path().join(&name);
        fs::copy(&src, &work).expect("copy example");
        let path = work.to_str().unwrap();

        for cmd in [vec!["check", path], vec!["lower", path], vec!["info", path]] {
            let out = run(&cmd);
            assert_eq!(
                exit_code(&out),
                0,
                "{name:?}: `{}` regressed; stderr={}",
                cmd[0],
                String::from_utf8_lossy(&out.stderr),
            );
        }

        let out_dir = tmp.path().join(format!("out-{}", name.to_string_lossy()));
        let out = run(&[
            "compile",
            path,
            "--edition",
            "java",
            "--out",
            out_dir.to_str().unwrap(),
        ]);
        assert_eq!(
            exit_code(&out),
            0,
            "{name:?}: compile regressed; stderr={}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
    assert!(
        seen >= 12,
        "expected the example corpus, found {seen} files"
    );
}
