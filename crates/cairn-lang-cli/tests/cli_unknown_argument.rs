//! `E_UNKNOWN_ARGUMENT` end to end: what the two commands do with a
//! misspelled `key=`.
//!
//! The severity is what makes the report readable, and it is the ordering
//! rather than a suppression that does it. `run_compile` lowers even when
//! `check` found an error — the lowering warnings are printed after the
//! check findings and the error decides the exit code — so a misspelled
//! argument now reads: the typo first, then the `W_DEFERRED_MEMBER` it
//! caused, which names the argument that is *absent*. Before this pass, the
//! second line was the only line.

use std::path::PathBuf;
use std::process::Command;

fn cargo_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn"))
}

/// The issue's repro: one letter, and the wall is built without the height
/// it asked for.
const TYPO: &str =
    "@cairn 2026.06\n\nstruct s size=5x5\n  walls class=outer mat_slot=wall hieght=3\n";

fn fixture(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("typo.crn");
    std::fs::write(&path, TYPO).expect("write fixture");
    path
}

#[test]
fn check_refuses_a_source_carrying_a_key_nothing_reads() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path());
    let out = Command::new(cargo_bin())
        .args(["check", src.to_str().unwrap()])
        .output()
        .expect("run cairn");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    assert!(stderr.contains("E_UNKNOWN_ARGUMENT"), "got: {stderr}");
    assert!(stderr.contains("did you mean `height`?"), "got: {stderr}");
}

#[test]
fn compile_reports_the_misspelling_before_the_absence_it_causes() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let src = fixture(tmp.path());
    let out_dir = tmp.path().join("out");
    let out = Command::new(cargo_bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "--edition",
            "java",
            "--target",
            "1.21",
            "--out",
            out_dir.to_str().unwrap(),
            "--lock",
            out_dir.join("build.crn.lock").to_str().unwrap(),
        ])
        .output()
        .expect("run cairn");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr={stderr}");
    // The typo and the deferral it causes are both printed, and the order
    // is what makes the pair readable: the repair first, its consequence
    // after. Reversed, the author is told to add a `height=` that is
    // already on the line.
    let typo = stderr
        .find("E_UNKNOWN_ARGUMENT")
        .unwrap_or_else(|| panic!("the misspelling must be reported: {stderr}"));
    let deferral = stderr
        .find("W_DEFERRED_MEMBER")
        .unwrap_or_else(|| panic!("premise: the typo still defers the member: {stderr}"));
    assert!(typo < deferral, "got: {stderr}");
    assert!(
        !out_dir.join("s.nbt").exists(),
        "nothing is written for a source the check gate refused",
    );
}
